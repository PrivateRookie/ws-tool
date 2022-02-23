use crate::errors::{ProtocolError, WsError};
use crate::frame::{get_bit, parse_opcode, parse_payload_len, Frame, OpCode};
use crate::protocol::{
    cal_accept_key, standard_handshake_req_check, standard_handshake_resp_check,
};
use crate::stream::WsStream;
use bytes::{Buf, BytesMut};
use futures::{Sink, SinkExt};
use std::{fmt::Debug, ops::Deref};
use tokio::io::{ReadHalf, WriteHalf};
use tokio_util::codec::{Decoder, Encoder, Framed, FramedRead, FramedWrite};

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

#[cfg(features = "async")]
pub use non_block::AWebSocketFrameCodec;

#[cfg(features = "blocking")]
pub use blocking::WebSocketFrameCodec;

struct FrameReadState {
    fragmented: bool,
    config: FrameConfig,
    read_buf: BytesMut,
    fragmented_data: BytesMut,
    fragmented_type: OpCode,
}

impl FrameReadState {
    pub fn new() -> Self {
        Self {
            config: FrameConfig::default(),
            fragmented: false,
            read_buf: BytesMut::with_capacity(1024 * 4),
            fragmented_data: BytesMut::new(),
            fragmented_type: OpCode::default(),
        }
    }

    pub fn with_config(config: FrameConfig) -> Self {
        Self {
            config,
            ..Self::new()
        }
    }

    pub fn leading_bits_ok(&self) -> bool {
        self.read_buf.len() >= 2
    }

    pub fn get_leading_bits(&self) -> u8 {
        self.read_buf[0] >> 4
    }

    pub fn body_ok(&self, expect_len: usize) -> bool {
        self.read_buf.len() >= expect_len
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
        parse_opcode(self.read_buf[0]).map_err(|e| WsError::ProtocolError {
            error: ProtocolError::InvalidOpcode(e),
            close_code: 1008,
        })?;
        let (payload_len, len_occ_bytes) =
            parse_payload_len(self.read_buf.as_ref()).map_err(|e| WsError::ProtocolError {
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
        let mask = get_bit(&self.read_buf, 1, 0);
        if mask {
            expected_len += 4;
        };
        Ok(expected_len)
    }

    fn consume_frame(&mut self, len: usize) -> Frame {
        let mut data = BytesMut::with_capacity(len);
        data.extend_from_slice(&self.read_buf[..len]);
        self.read_buf.advance(len);
        Frame(data)
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
        frame::{Frame, OpCode},
    };
    use std::io::{Read, Write};

    impl FrameReadState {
        fn poll<S: Read>(&mut self, stream: &mut S) -> IOResult<usize> {
            stream.read(&mut self.read_buf)
        }

        fn read_one_frame<S: Read>(&mut self, stream: &mut S) -> Result<Frame, WsError> {
            while !self.leading_bits_ok() {
                self.poll(stream)?;
            }
            let len = self.parse_frame_header()?;
            while !self.body_ok(len) {
                self.poll(stream)?;
            }
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
        fn send_one<S: Write>(
            &mut self,
            payload: &[u8],
            fin: bool,
            mask: bool,
            code: OpCode,
            stream: &mut S,
        ) -> IOResult<usize> {
            let mut frame = Frame::new_with_opcode(code);
            frame.set_fin(fin);
            frame.set_mask(mask);
            frame.set_payload(payload);
            stream.write(&frame.0)
        }

        pub fn send<S: Write>(
            &mut self,
            payload: &[u8],
            code: OpCode,
            stream: &mut S,
        ) -> IOResult<usize> {
            let split_size = self.config.auto_fragment_size;
            let data = payload;
            if split_size > 0 {
                let mut idx = 0;
                let mut fin = idx + split_size >= data.len();
                let mut total = 0;
                while (idx + split_size) <= data.len() {
                    let end = data.len().min(idx + split_size);
                    let send = self.send_one(
                        &data[idx..end],
                        fin,
                        self.config.mask,
                        code.clone(),
                        stream,
                    )?;
                    total += send;
                    idx = end;
                    fin = idx + split_size >= data.len();
                }
                Ok(total)
            } else {
                self.send_one(&data, true, self.config.mask, code, stream)
            }
        }
    }

    pub struct WebSocketFrameCodec<S: Read + Write> {
        stream: S,
        read_state: FrameReadState,
        write_state: FrameWriteState,
    }

    impl<S: Read + Write> WebSocketFrameCodec<S> {
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

        pub fn receive(&mut self) -> Result<Frame, WsError> {
            self.read_state.receive(&mut self.stream)
        }

        pub fn send(&mut self, payload: &[u8], code: OpCode) -> Result<usize, WsError> {
            self.write_state
                .send(payload, code, &mut self.stream)
                .map_err(|e| WsError::IOError(Box::new(e)))
        }
    }
}

#[cfg(feature = "async")]
mod non_block {
    use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

    use super::{FrameConfig, FrameReadState, FrameWriteState, IOResult};
    use crate::{
        errors::WsError,
        frame::{Frame, OpCode},
    };

    impl FrameReadState {
        async fn async_poll<S: AsyncRead + Unpin>(&mut self, stream: &mut S) -> IOResult<usize> {
            stream.read(&mut self.read_buf).await
        }

        async fn async_read_one_frame<S: AsyncRead + Unpin>(
            &mut self,
            stream: &mut S,
        ) -> Result<Frame, WsError> {
            while !self.leading_bits_ok() {
                self.async_poll(stream).await?;
            }
            let len = self.parse_frame_header()?;
            while !self.body_ok(len) {
                self.async_poll(stream).await?;
            }
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
        async fn async_send_one<S: AsyncWrite + Unpin>(
            &mut self,
            payload: &[u8],
            fin: bool,
            mask: bool,
            code: OpCode,
            stream: &mut S,
        ) -> IOResult<usize> {
            let mut frame = Frame::new_with_opcode(code);
            frame.set_fin(fin);
            frame.set_mask(mask);
            frame.set_payload(payload);
            stream.write(&frame.0).await
        }

        pub async fn async_send<S: AsyncWrite + Unpin>(
            &mut self,
            payload: &[u8],
            code: OpCode,
            stream: &mut S,
        ) -> IOResult<usize> {
            let split_size = self.config.auto_fragment_size;
            let data = payload;
            if split_size > 0 {
                let mut idx = 0;
                let mut fin = idx + split_size >= data.len();
                let mut total = 0;
                while (idx + split_size) <= data.len() {
                    let end = data.len().min(idx + split_size);
                    let send = self
                        .async_send_one(
                            &data[idx..end],
                            fin,
                            self.config.mask,
                            code.clone(),
                            stream,
                        )
                        .await?;
                    total += send;
                    idx = end;
                    fin = idx + split_size >= data.len();
                }
                Ok(total)
            } else {
                self.async_send_one(&data, true, self.config.mask, code, stream)
                    .await
            }
        }
    }

    pub struct AWebSocketFrameCodec<S: AsyncRead + AsyncWrite> {
        stream: S,
        read_state: FrameReadState,
        write_state: FrameWriteState,
    }

    impl<S: AsyncRead + AsyncWrite + Unpin> AWebSocketFrameCodec<S> {
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

        pub async fn receive(&mut self) -> Result<Frame, WsError> {
            self.read_state.async_receive(&mut self.stream).await
        }

        pub async fn send(&mut self, payload: &[u8], code: OpCode) -> Result<usize, WsError> {
            self.write_state
                .async_send(payload, code, &mut self.stream)
                .await
                .map_err(|e| WsError::IOError(Box::new(e)))
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct WebSocketFrameEncoder {
    pub config: FrameConfig,
}

#[derive(Debug, Clone, Default)]
pub struct WebSocketFrameDecoder {
    pub config: FrameConfig,
    pub fragmented: bool,
    pub fragmented_data: BytesMut,
    pub fragmented_type: OpCode,
}

#[derive(Debug, Clone, Default)]
pub struct WebSocketFrameCodec {
    pub config: FrameConfig,
    pub fragmented: bool,
    pub fragmented_data: BytesMut,
    pub fragmented_type: OpCode,
}

pub async fn send_close<S: Sink<I, Error = E> + Unpin, E: From<WsError>, I: From<Frame>>(
    framed: &mut S,
    code: u16,
    reason: String,
) -> Result<(), E> {
    let mut payload = BytesMut::with_capacity(2 + reason.as_bytes().len());
    payload.extend_from_slice(&code.to_be_bytes());
    payload.extend_from_slice(reason.as_bytes());
    let close = Frame::new_with_payload(OpCode::Close, &payload);
    framed.send(close.into()).await
}

pub fn default_frame_check_fn(
    key: String,
    resp: http::Response<()>,
    stream: WsStream,
) -> Result<Framed<WsStream, WebSocketFrameCodec>, WsError> {
    standard_handshake_resp_check(key.as_bytes(), &resp)?;
    Ok(Framed::new(stream, WebSocketFrameCodec::default()))
}

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

pub fn default_frame_codec_factory(
    _req: http::Request<()>,
    stream: WsStream,
) -> Result<Framed<WsStream, WebSocketFrameCodec>, WsError> {
    let mut codec = WebSocketFrameCodec::default();
    // do not mask server side frame
    codec.config.mask = false;
    Ok(Framed::new(stream, codec))
}

pub fn write_single_frame(payload: &[u8], fin: bool, mask: bool, code: OpCode, dst: &mut BytesMut) {
    let mut frame = Frame::new_with_opcode(code);
    frame.set_fin(fin);
    frame.set_mask(mask);
    frame.set_payload(payload);
    dst.extend_from_slice(&frame.0);
}

pub fn write_frame(config: &FrameConfig, payload: &[u8], code: OpCode, dst: &mut BytesMut) {
    let split_size = config.auto_fragment_size;
    let data = payload;
    if split_size > 0 {
        let mut idx = 0;
        let mut fin = idx + split_size >= data.len();
        while (idx + split_size) <= data.len() {
            let end = data.len().min(idx + split_size);
            write_single_frame(&data[idx..end], fin, config.mask, code.clone(), dst);
            idx = end;
            fin = idx + split_size >= data.len();
        }
    } else {
        write_single_frame(&data, true, config.mask, code, dst)
    }
}

impl Encoder<Frame> for WebSocketFrameEncoder {
    type Error = WsError;

    fn encode(&mut self, item: Frame, dst: &mut BytesMut) -> Result<(), Self::Error> {
        write_frame(
            &self.config,
            &item.payload_data_unmask(),
            item.opcode(),
            dst,
        );
        Ok(())
    }
}

impl Encoder<Frame> for WebSocketFrameCodec {
    type Error = WsError;

    fn encode(&mut self, item: Frame, dst: &mut BytesMut) -> Result<(), Self::Error> {
        write_frame(
            &self.config,
            &item.payload_data_unmask(),
            item.opcode(),
            dst,
        );
        Ok(())
    }
}

pub fn decode_single_frame(
    config: &FrameConfig,
    src: &mut BytesMut,
) -> Result<Option<Frame>, WsError> {
    if src.len() < 2 {
        return Ok(None);
    }
    // TODO check nonzero value according to extension negotiation
    let leading_bits = src[0] >> 4;
    if config.check_rsv && !(leading_bits == 0b00001000 || leading_bits == 0b00000000) {
        return Err(WsError::ProtocolError {
            close_code: 1008,
            error: ProtocolError::InvalidLeadingBits(leading_bits),
        });
    }
    parse_opcode(src[0]).map_err(|e| WsError::ProtocolError {
        error: ProtocolError::InvalidOpcode(e),
        close_code: 1008,
    })?;
    let (payload_len, len_occ_bytes) =
        parse_payload_len(src.deref()).map_err(|e| WsError::ProtocolError {
            close_code: 1008,
            error: e,
        })?;
    let max_payload_size = config.max_frame_payload_size;
    if max_payload_size > 0 && payload_len > max_payload_size {
        return Err(WsError::ProtocolError {
            close_code: 1008,
            error: ProtocolError::PayloadTooLarge(max_payload_size),
        });
    }
    let mut expected_len = 1 + len_occ_bytes + payload_len;
    let mask = get_bit(&src, 1, 0);
    if mask {
        expected_len += 4;
    }
    if expected_len > src.len() {
        src.reserve(expected_len - src.len() + 1);
        Ok(None)
    } else {
        let mut data = BytesMut::with_capacity(expected_len);
        data.extend_from_slice(&src[..expected_len]);
        src.advance(expected_len);
        Ok(Some(Frame(data)))
    }
}

pub fn decode_frame(
    config: &FrameConfig,
    fragmented: &mut bool,
    fragmented_data: &mut BytesMut,
    fragmented_type: &mut OpCode,
    src: &mut BytesMut,
) -> Result<Option<Frame>, WsError> {
    let maybe_frame = decode_single_frame(config, src)?;
    if let Some(frame) = maybe_frame {
        let opcode = frame.opcode();
        match opcode {
            OpCode::Continue => {
                if !*fragmented {
                    return Err(WsError::ProtocolError {
                        close_code: 1002,
                        error: ProtocolError::MissInitialFragmentedFrame,
                    });
                }
                fragmented_data.extend_from_slice(&frame.payload_data_unmask());
                if frame.fin() {
                    // if String::from_utf8(fragmented_data.to_vec()).is_err() {
                    //     return Err(WsError::ProtocolError {
                    //         close_code: 1007,
                    //         error: ProtocolError::InvalidUtf8,
                    //     });
                    // }
                    let completed_frame =
                        Frame::new_with_payload(fragmented_type.clone(), fragmented_data);
                    return Ok(Some(completed_frame));
                } else {
                    Ok(None)
                }
            }
            OpCode::Text | OpCode::Binary => {
                if *fragmented {
                    return Err(WsError::ProtocolError {
                        close_code: 1002,
                        error: ProtocolError::NotContinueFrameAfterFragmented,
                    });
                }
                if !frame.fin() {
                    *fragmented = true;
                    *fragmented_type = opcode.clone();
                    let payload = frame.payload_data_unmask();
                    fragmented_data.extend_from_slice(&payload);
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
                    return Ok(Some(frame));
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
                if opcode == OpCode::Close || !*fragmented {
                    return Ok(Some(frame));
                } else {
                    tracing::debug!("{:?} frame between self.fragmented data", opcode);
                    return Ok(Some(frame));
                }
            }
            OpCode::ReservedNonControl | OpCode::ReservedControl => {
                return Err(WsError::UnsupportedFrame(opcode));
            }
        }
    } else {
        Ok(None)
    }
}

impl Decoder for WebSocketFrameDecoder {
    type Item = Frame;

    type Error = WsError;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        decode_frame(
            &self.config,
            &mut self.fragmented,
            &mut self.fragmented_data,
            &mut self.fragmented_type,
            src,
        )
    }
}

impl Decoder for WebSocketFrameCodec {
    type Item = Frame;

    type Error = WsError;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        decode_frame(
            &self.config,
            &mut self.fragmented,
            &mut self.fragmented_data,
            &mut self.fragmented_type,
            src,
        )
    }
}

// impl SplitSocket<Frame, Frame, WebSocketFrameEncoder, WebSocketFrameDecoder>
//     for Framed<WsStream, WebSocketFrameCodec>
// {
//     fn split(
//         self,
//     ) -> (
//         FramedRead<ReadHalf<WsStream>, WebSocketFrameDecoder>,
//         FramedWrite<WriteHalf<WsStream>, WebSocketFrameEncoder>,
//     ) {
//         let parts = self.into_parts();
//         let (read_io, write_io) = tokio::io::split(parts.io);
//         let codec = parts.codec;
//         let mut frame_read = FramedRead::new(
//             read_io,
//             WebSocketFrameDecoder {
//                 config: codec.config.clone(),
//                 fragmented: codec.fragmented,
//                 fragmented_data: codec.fragmented_data,
//                 fragmented_type: codec.fragmented_type,
//             },
//         );
//         *frame_read.read_buffer_mut() = parts.read_buf;
//         let mut frame_write = FramedWrite::new(
//             write_io,
//             WebSocketFrameEncoder {
//                 config: codec.config,
//             },
//         );
//         *frame_write.write_buffer_mut() = parts.write_buf;
//         (frame_read, frame_write)
//     }
// }
