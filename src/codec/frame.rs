use crate::errors::{ProtocolError, WsError};
use crate::frame::{get_bit, parse_opcode, parse_payload_len, Frame, OpCode};
use crate::protocol::{cal_accept_key, standard_handshake_req_check};
use bytes::BytesMut;
use std::fmt::Debug;

#[derive(Debug, Clone)]
pub struct FrameConfig {
    pub check_rsv: bool,
    pub mask: bool,
    pub max_frame_payload_size: usize,
    pub auto_fragment_size: usize,
    pub merge_frame: bool,
}

impl Default for FrameConfig {
    fn default() -> Self {
        Self {
            check_rsv: true,
            mask: true,
            max_frame_payload_size: 0,
            auto_fragment_size: 0,
            merge_frame: true,
        }
    }
}
type IOResult<T> = std::io::Result<T>;

#[inline]
pub fn apply_mask(buf: &mut [u8], mask: [u8; 4]) {
    apply_mask_fast32(buf, mask)
}

#[inline]
fn apply_mask_fallback(buf: &mut [u8], mask: [u8; 4]) {
    for (i, byte) in buf.iter_mut().enumerate() {
        *byte ^= mask[i & 3];
    }
}

#[inline]
pub fn apply_mask_fast32(buf: &mut [u8], mask: [u8; 4]) {
    let mask_u32 = u32::from_ne_bytes(mask);

    let (prefix, words, suffix) = unsafe { buf.align_to_mut::<u32>() };
    apply_mask_fallback(prefix, mask);
    let head = prefix.len() & 3;
    let mask_u32 = if head > 0 {
        if cfg!(target_endian = "big") {
            mask_u32.rotate_left(8 * head as u32)
        } else {
            mask_u32.rotate_right(8 * head as u32)
        }
    } else {
        mask_u32
    };
    for word in words.iter_mut() {
        *word ^= mask_u32;
    }
    apply_mask_fallback(suffix, mask_u32.to_ne_bytes());
}

struct FrameReadState {
    fragmented: bool,
    config: FrameConfig,
    read_idx: usize,
    read_data: BytesMut,
    fragmented_data: BytesMut,
    fragmented_type: OpCode,
}

impl FrameReadState {
    pub fn new() -> Self {
        Self {
            config: FrameConfig::default(),
            fragmented: false,
            read_idx: 0,
            read_data: BytesMut::new(),
            fragmented_data: BytesMut::new(),
            fragmented_type: OpCode::default(),
        }
    }

    pub fn with_remain(config: FrameConfig, read_data: BytesMut) -> Self {
        Self {
            config,
            read_idx: read_data.len(),
            read_data,
            ..Self::new()
        }
    }

    pub fn with_config(config: FrameConfig) -> Self {
        Self {
            config,
            ..Self::new()
        }
    }

    pub fn leading_bits_ok(&self) -> bool {
        self.read_data.len() >= 2
    }

    pub fn get_leading_bits(&self) -> u8 {
        self.read_data[0] >> 4
    }

    fn parse_frame_header(&mut self) -> Result<usize, WsError> {
        // TODO check nonzero value according to extension negotiation
        let leading_bits = self.get_leading_bits();
        if self.config.check_rsv && !(leading_bits == 0b00001000 || leading_bits == 0b00000000) {
            return Err(WsError::ProtocolError {
                close_code: 1008,
                error: ProtocolError::InvalidLeadingBits(leading_bits),
            });
        }
        parse_opcode(self.read_data[0]).map_err(|e| WsError::ProtocolError {
            error: ProtocolError::InvalidOpcode(e),
            close_code: 1008,
        })?;
        let (payload_len, len_occ_bytes) =
            parse_payload_len(self.read_data.as_ref()).map_err(|e| WsError::ProtocolError {
                close_code: 1008,
                error: e,
            })?;
        let max_payload_size = self.config.max_frame_payload_size;
        if max_payload_size > 0 && payload_len > max_payload_size {
            return Err(WsError::ProtocolError {
                close_code: 1008,
                error: ProtocolError::PayloadTooLarge(max_payload_size),
            });
        }
        let mut expected_len = 1 + len_occ_bytes + payload_len;
        let mask = get_bit(&self.read_data, 1, 0);
        if mask {
            expected_len += 4;
        };
        Ok(expected_len)
    }

    fn consume_frame(&mut self, len: usize) -> Frame {
        let data = self.read_data.split_to(len);
        self.read_idx = self.read_data.len();
        let mut frame = Frame(data);
        if let Some(key) = frame.masking_key() {
            apply_mask_fast32(frame.payload_mut(), key)
        }
        frame
    }

    fn check_frame(&mut self, frame: Frame) -> Result<Option<Frame>, WsError> {
        let opcode = frame.opcode();
        match opcode {
            OpCode::Continue => {
                if !self.fragmented {
                    return Err(WsError::ProtocolError {
                        close_code: 1002,
                        error: ProtocolError::MissInitialFragmentedFrame,
                    });
                }
                self.fragmented_data
                    .extend_from_slice(&frame.payload_data_unmask());
                if frame.fin() {
                    // if String::from_utf8(fragmented_data.to_vec()).is_err() {
                    //     return Err(WsError::ProtocolError {
                    //         close_code: 1007,
                    //         error: ProtocolError::InvalidUtf8,
                    //     });
                    // }
                    let completed_frame = Frame::new_with_payload(
                        self.fragmented_type.clone(),
                        &self.fragmented_data,
                    );
                    Ok(Some(completed_frame))
                } else {
                    Ok(None)
                }
            }
            OpCode::Text | OpCode::Binary => {
                if self.fragmented {
                    return Err(WsError::ProtocolError {
                        close_code: 1002,
                        error: ProtocolError::NotContinueFrameAfterFragmented,
                    });
                }
                if !frame.fin() {
                    self.fragmented = true;
                    self.fragmented_type = opcode.clone();
                    let payload = frame.payload_data_unmask();
                    self.fragmented_data.extend_from_slice(&payload);
                    Ok(None)
                } else {
                    // if opcode == OpCode::Text
                    //     && String::from_utf8(frame.payload_data_unmask().to_vec()).is_err()
                    // {
                    //     return Err(WsError::ProtocolError {
                    //         close_code: 1007,
                    //         error: ProtocolError::InvalidUtf8,
                    //     });
                    // }
                    Ok(Some(frame))
                }
            }
            OpCode::Close | OpCode::Ping | OpCode::Pong => {
                if !frame.fin() {
                    return Err(WsError::ProtocolError {
                        close_code: 1002,
                        error: ProtocolError::FragmentedControlFrame,
                    });
                }
                let payload_len = frame.payload_len();
                if payload_len > 125 {
                    let error = ProtocolError::ControlFrameTooBig(payload_len as usize);
                    return Err(WsError::ProtocolError {
                        close_code: 1002,
                        error,
                    });
                }
                if opcode == OpCode::Close {
                    if payload_len == 1 {
                        let error = ProtocolError::InvalidCloseFramePayload;
                        return Err(WsError::ProtocolError {
                            close_code: 1002,
                            error,
                        });
                    }
                    if payload_len >= 2 {
                        let payload = frame.payload_data_unmask();

                        // check close code
                        let mut code_byte = [0u8; 2];
                        code_byte.copy_from_slice(&payload[..2]);
                        let code = u16::from_be_bytes(code_byte);
                        if code < 1000
                            || (1004..=1006).contains(&code)
                            || (1015..=2999).contains(&code)
                            || code >= 5000
                        {
                            let error = ProtocolError::InvalidCloseCode(code);
                            return Err(WsError::ProtocolError {
                                close_code: 1002,
                                error,
                            });
                        }

                        // utf-8 validation
                        if String::from_utf8(payload[2..].to_vec()).is_err() {
                            let error = ProtocolError::InvalidUtf8;
                            return Err(WsError::ProtocolError {
                                close_code: 1007,
                                error,
                            });
                        }
                    }
                }
                if opcode == OpCode::Close || !self.fragmented {
                    Ok(Some(frame))
                } else {
                    tracing::debug!("{:?} frame between self.fragmented data", opcode);
                    Ok(Some(frame))
                }
            }
            OpCode::ReservedNonControl | OpCode::ReservedControl => {
                return Err(WsError::UnsupportedFrame(opcode));
            }
        }
    }
}

struct FrameWriteState {
    config: FrameConfig,
}

impl FrameWriteState {
    pub fn new() -> Self {
        Self {
            config: FrameConfig::default(),
        }
    }

    pub fn with_config(config: FrameConfig) -> Self {
        Self { config }
    }
}

#[cfg(feature = "blocking")]
mod blocking {
    use super::{FrameConfig, FrameReadState, FrameWriteState, IOResult};
    use crate::{
        errors::WsError,
        frame::{Frame, OpCode, Payload},
        protocol::standard_handshake_resp_check,
    };
    use std::io::{Read, Write};

    impl FrameReadState {
        fn poll<S: Read>(&mut self, stream: &mut S) -> IOResult<usize> {
            self.read_data.resize(self.read_idx + 1024, 0);
            let count = stream.read(&mut self.read_data[self.read_idx..])?;
            self.read_idx += count;
            self.read_data.resize(self.read_idx, 0);
            if count == 0 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::ConnectionAborted,
                    "read eof",
                ));
            }
            Ok(count)
        }

        fn poll_one_frame<S: Read>(&mut self, stream: &mut S, size: usize) -> IOResult<usize> {
            let buf_len = self.read_data.len();
            if buf_len < size {
                self.read_data.resize(size, 0);
                stream.read_exact(&mut self.read_data[buf_len..size])?;
                Ok(size - buf_len)
            } else {
                Ok(0)
            }
        }

        fn read_one_frame<S: Read>(&mut self, stream: &mut S) -> Result<Frame, WsError> {
            while !self.leading_bits_ok() {
                self.poll(stream)?;
            }
            let len = self.parse_frame_header()?;
            self.poll_one_frame(stream, len)?;
            Ok(self.consume_frame(len))
        }

        pub fn receive<S: Read>(&mut self, stream: &mut S) -> Result<Frame, WsError> {
            loop {
                let frame = self.read_one_frame(stream)?;
                if let Some(frame) = self.check_frame(frame)? {
                    break Ok(frame);
                }
            }
        }
    }

    impl FrameWriteState {
        fn send_one<'a, S: Write>(
            &mut self,
            payload: Payload<'a>,
            fin: bool,
            mask: bool,
            code: OpCode,
            stream: &mut S,
        ) -> IOResult<()> {
            let frame = Frame::new(fin, code, mask, payload);
            stream.write_all(&frame.0)
        }

        pub fn send<'a, S: Write>(
            &mut self,
            payload: Payload,
            code: OpCode,
            stream: &mut S,
        ) -> IOResult<usize> {
            let split_size = self.config.auto_fragment_size;
            let len = payload.len();
            if split_size > 0 {
                let parts = payload.split_with(split_size);
                let total = parts.len();
                for (idx, part) in parts.into_iter().enumerate() {
                    let fin = idx + 1 == total;
                    self.send_one(part, fin, self.config.mask, code.clone(), stream)?;
                }
            } else {
                self.send_one(payload, true, self.config.mask, code, stream)?;
            }
            Ok(len)
        }
    }

    pub struct WsFrameCodec<S: Read + Write> {
        stream: S,
        read_state: FrameReadState,
        write_state: FrameWriteState,
    }

    impl<S: Read + Write> WsFrameCodec<S> {
        pub fn new(stream: S) -> Self {
            Self {
                stream,
                read_state: FrameReadState::new(),
                write_state: FrameWriteState::new(),
            }
        }

        pub fn new_with(stream: S, config: FrameConfig) -> Self {
            Self {
                stream,
                read_state: FrameReadState::with_config(config.clone()),
                write_state: FrameWriteState::with_config(config),
            }
        }

        pub fn stream_mut(&mut self) -> &mut S {
            &mut self.stream
        }

        pub fn factory(_req: http::Request<()>, stream: S) -> Result<Self, WsError> {
            let mut config = FrameConfig::default();
            config.mask = false;
            Ok(Self::new_with(stream, config))
        }

        pub fn check_fn(key: String, resp: http::Response<()>, stream: S) -> Result<Self, WsError> {
            standard_handshake_resp_check(key.as_bytes(), &resp)?;
            Ok(Self::new_with(stream, FrameConfig::default()))
        }

        pub fn receive(&mut self) -> Result<Frame, WsError> {
            self.read_state.receive(&mut self.stream)
        }

        pub fn send<'a, P: Into<Payload<'a>>>(
            &mut self,
            code: OpCode,
            payload: P,
        ) -> Result<usize, WsError> {
            self.write_state
                .send(payload.into(), code, &mut self.stream)
                .map_err(|e| WsError::IOError(Box::new(e)))
        }
    }
}

#[cfg(feature = "blocking")]
pub use blocking::WsFrameCodec;

#[cfg(feature = "async")]
mod non_block {
    use bytes::BytesMut;
    use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

    use super::{FrameConfig, FrameReadState, FrameWriteState, IOResult};
    use crate::{
        errors::WsError,
        frame::{Frame, OpCode, Payload},
        protocol::standard_handshake_resp_check,
    };

    impl FrameReadState {
        async fn async_poll<S: AsyncRead + Unpin>(&mut self, stream: &mut S) -> IOResult<usize> {
            self.read_data.resize(self.read_idx + 1024, 0);
            let count = stream.read(&mut self.read_data[self.read_idx..]).await?;
            self.read_idx += count;
            self.read_data.resize(self.read_idx, 0);
            if count == 0 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::ConnectionAborted,
                    "read eof",
                ));
            }
            Ok(count)
        }

        async fn async_poll_one_frame<S: AsyncRead + Unpin>(
            &mut self,
            stream: &mut S,
            size: usize,
        ) -> IOResult<usize> {
            let buf_len = self.read_data.len();
            if buf_len < size {
                self.read_data.resize(size, 0);
                stream
                    .read_exact(&mut self.read_data[buf_len..size])
                    .await?;
                Ok(size - buf_len)
            } else {
                Ok(0)
            }
        }

        async fn async_read_one_frame<S: AsyncRead + Unpin>(
            &mut self,
            stream: &mut S,
        ) -> Result<Frame, WsError> {
            while !self.leading_bits_ok() {
                self.async_poll(stream).await?;
            }
            let len = self.parse_frame_header()?;
            // while !self.body_ok(len) {
            //     self.async_poll(stream).await?;
            // }
            self.async_poll_one_frame(stream, len).await?;
            Ok(self.consume_frame(len))
        }

        pub async fn async_receive<S: AsyncRead + Unpin>(
            &mut self,
            stream: &mut S,
        ) -> Result<Frame, WsError> {
            loop {
                let frame = self.async_read_one_frame(stream).await?;
                if let Some(frame) = self.check_frame(frame)? {
                    break Ok(frame);
                }
            }
        }
    }

    impl FrameWriteState {
        async fn async_send_one<'a, S: AsyncWrite + Unpin>(
            &mut self,
            payload: Payload<'a>,
            fin: bool,
            mask: bool,
            code: OpCode,
            stream: &mut S,
        ) -> IOResult<usize> {
            let frame = Frame::new(fin, code, mask, payload);
            stream.write(&frame.0).await
        }

        pub async fn async_send<'a, S: AsyncWrite + Unpin>(
            &mut self,
            payload: Payload<'a>,
            code: OpCode,
            stream: &mut S,
        ) -> IOResult<usize> {
            let split_size = self.config.auto_fragment_size;
            if split_size > 0 {
                let parts = payload.split_with(split_size);
                let total = parts.len();
                for (idx, part) in parts.into_iter().enumerate() {
                    let fin = idx + 1 == total;
                    self.async_send_one(part, fin, self.config.mask, code.clone(), stream)
                        .await?;
                }
                Ok(payload.len())
            } else {
                self.async_send_one(payload, true, self.config.mask, code, stream)
                    .await
            }
        }
    }

    pub struct AsyncWsFrameCodec<S: AsyncRead + AsyncWrite> {
        stream: S,
        read_state: FrameReadState,
        write_state: FrameWriteState,
    }

    impl<S: AsyncRead + AsyncWrite + Unpin> AsyncWsFrameCodec<S> {
        pub fn new(stream: S) -> Self {
            Self {
                stream,
                read_state: FrameReadState::new(),
                write_state: FrameWriteState::new(),
            }
        }

        pub fn new_with(stream: S, config: FrameConfig, read_bytes: BytesMut) -> Self {
            Self {
                stream,
                read_state: FrameReadState::with_remain(config.clone(), read_bytes),
                write_state: FrameWriteState::with_config(config),
            }
        }

        pub fn factory(
            _req: http::Request<()>,
            remain: BytesMut,
            stream: S,
        ) -> Result<Self, WsError> {
            let mut config = FrameConfig::default();
            // do not mask server side frame
            config.mask = false;
            Ok(Self::new_with(stream, config, remain))
        }

        pub fn check_fn(
            key: String,
            resp: http::Response<()>,
            remain: BytesMut,
            stream: S,
        ) -> Result<Self, WsError> {
            standard_handshake_resp_check(key.as_bytes(), &resp)?;
            Ok(Self::new_with(stream, FrameConfig::default(), remain))
        }

        pub fn stream_mut(&mut self) -> &mut S {
            &mut self.stream
        }

        pub async fn receive(&mut self) -> Result<Frame, WsError> {
            self.read_state.async_receive(&mut self.stream).await
        }

        pub async fn send<'a, P: Into<Payload<'a>>>(
            &mut self,
            code: OpCode,
            payload: P,
        ) -> Result<usize, WsError> {
            self.write_state
                .async_send(payload.into(), code, &mut self.stream)
                .await
                .map_err(|e| WsError::IOError(Box::new(e)))
        }
    }
}

#[cfg(feature = "async")]
pub use non_block::AsyncWsFrameCodec;

pub fn default_handshake_handler(
    req: http::Request<()>,
) -> Result<(http::Request<()>, http::Response<String>), WsError> {
    match standard_handshake_req_check(&req) {
        Ok(_) => {
            let key = req.headers().get("sec-websocket-key").unwrap();
            let resp = http::Response::builder()
                .version(http::Version::HTTP_11)
                .status(http::StatusCode::SWITCHING_PROTOCOLS)
                .header("Upgrade", "WebSocket")
                .header("Connection", "Upgrade")
                .header("Sec-WebSocket-Accept", cal_accept_key(key.as_bytes()))
                .body(String::new())
                .unwrap();
            Ok((req, resp))
        }
        Err(e) => {
            let resp = http::Response::builder()
                .version(http::Version::HTTP_11)
                .status(http::StatusCode::BAD_REQUEST)
                .header("Content-Type", "text/html")
                .body(e.to_string())
                .unwrap();
            Ok((req, resp))
        }
    }
}
