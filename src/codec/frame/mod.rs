use crate::errors::{ProtocolError, WsError};
use crate::frame::{get_bit, Header, HeaderView, OpCode, OwnedFrame};
use crate::protocol::{cal_accept_key, standard_handshake_req_check};
use bytes::BytesMut;
use std::fmt::Debug;
use std::io::{Read, Write};

#[cfg(feature = "sync")]
mod blocking;

#[cfg(feature = "sync")]
pub use blocking::*;

#[cfg(feature = "async")]
mod non_blocking;

#[cfg(feature = "async")]
pub use non_blocking::*;

/// text frame utf-8 checking policy
#[derive(Debug, Clone)]
pub enum ValidateUtf8Policy {
    /// no not validate utf
    Off,
    /// fail if fragment frame payload is not valid utf8
    FastFail,
    /// check utf8 after merged
    On,
}

#[allow(missing_docs)]
impl ValidateUtf8Policy {
    pub fn should_check(&self) -> bool {
        !matches!(self, Self::Off)
    }

    pub fn is_fast_fail(&self) -> bool {
        matches!(self, Self::FastFail)
    }
}

/// frame send/recv config
#[derive(Debug, Clone)]
pub struct FrameConfig {
    /// check rsv1 bits
    pub check_rsv: bool,
    /// auto mask send frame payload, for client, it must be true
    pub mask_send_frame: bool,
    /// allocate new buf for every frame
    pub renew_buf_on_write: bool,
    /// auto unmask a masked frame payload
    pub auto_unmask: bool,
    /// limit max payload size
    pub max_frame_payload_size: usize,
    /// auto split size, if set 0, do not split frame
    pub auto_fragment_size: usize,
    /// auto merge fragmented frames into one frame
    pub merge_frame: bool,
    /// utf8 check policy
    pub validate_utf8: ValidateUtf8Policy,
    /// resize size of read buf, default 4K
    pub resize_size: usize,
    /// if available len < resize, resize read buf, default 1K
    pub resize_thresh: usize,
}

impl Default for FrameConfig {
    fn default() -> Self {
        Self {
            check_rsv: true,
            mask_send_frame: true,
            renew_buf_on_write: false,
            auto_unmask: true,
            max_frame_payload_size: 0,
            auto_fragment_size: 0,
            merge_frame: true,
            validate_utf8: ValidateUtf8Policy::FastFail,
            resize_size: 4096,
            resize_thresh: 1024,
        }
    }
}

/// apply websocket mask to buf by given key
#[inline]
pub fn apply_mask(buf: &mut [u8], mask: [u8; 4]) {
    #[cfg(feature = "simd")]
    {
        apply_mask_simd(buf, mask)
    }
    #[cfg(not(feature = "simd"))]
    {
        apply_mask_fast32(buf, mask)
    }
}

#[cfg(feature = "simd")]
fn apply_mask_simd(buf: &mut [u8], mask: [u8; 4]) {
    use std::simd::*;
    let total_len = buf.len();
    let (prefix, middle, suffix) = buf.as_simd_mut::<32>();
    apply_mask_fast32(prefix, mask);
    let middle_mask: [u8; 32] = std::array::from_fn(|idx| mask[(idx + prefix.len()) % 4]);
    let middle_mask = Simd::from_array(middle_mask);
    middle.iter_mut().for_each(|m| *m ^= middle_mask);
    let suffix_mask: [u8; 4] =
        std::array::from_fn(|idx| mask[(idx + total_len - suffix.len()) % 4]);
    apply_mask_fast32(suffix, suffix_mask);
}

#[inline]
fn apply_mask_fallback(buf: &mut [u8], mask: [u8; 4]) {
    for (i, byte) in buf.iter_mut().enumerate() {
        *byte ^= mask[i & 3];
    }
}

/// copy from tungstite
#[inline]
fn apply_mask_fast32(buf: &mut [u8], mask: [u8; 4]) {
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

pub struct ReactorRead {
    fragmented: bool,
    config: FrameConfig,
    fragmented_data: Vec<u8>,
    fragmented_type: OpCode,
    buf: FrameBuffer,
}

impl Default for ReactorRead {
    fn default() -> Self {
        Self {
            fragmented: false,
            config: Default::default(),
            fragmented_data: vec![],
            fragmented_type: OpCode::default(),
            buf: FrameBuffer::new(),
        }
    }
}

impl ReactorRead {
    /// construct with config
    pub fn with_config(config: FrameConfig) -> Self {
        Self {
            config,
            ..Self::default()
        }
    }

    /// check if data in buffer is enough to parse frame header
    pub fn is_header_ok(&self) -> bool {
        let ava_data = self.buf.ava_data();
        if ava_data.len() < 2 {
            false
        } else {
            let len = ava_data[1] & 0b01111111;
            let mask = get_bit(&ava_data, 1, 0);
            let mut min_len = match len {
                0..=125 => 2,
                126 => 4,
                127 => 10,
                _ => unreachable!(),
            };
            if mask {
                min_len += 4;
            }
            ava_data.len() >= min_len
        }
    }

    /// return current frame header bits of buffer
    #[inline]
    pub fn get_leading_bits(&self) -> u8 {
        self.buf.ava_data()[0] >> 4
    }

    /// try to parse frame header in buffer, return (header_len, payload_len, header_len + payload_len)
    #[inline]
    pub fn parse_frame_header(&mut self) -> Result<(usize, usize, usize), WsError> {
        fn parse_payload_len(source: &[u8]) -> Result<(usize, usize), ProtocolError> {
            match source[1] {
                len @ (0..=125 | 128..=253) => Ok((1, (len & 127) as usize)),
                126 | 254 => {
                    if source.len() < 4 {
                        return Err(ProtocolError::InsufficientLen(source.len()));
                    }
                    Ok((
                        1 + 2,
                        u16::from_be_bytes((&source[2..4]).try_into().unwrap()) as usize,
                    ))
                }
                127 | 255 => {
                    if source.len() < 10 {
                        return Err(ProtocolError::InsufficientLen(source.len()));
                    }
                    Ok((
                        1 + 8,
                        usize::from_be_bytes((&source[2..(8 + 2)]).try_into().unwrap()),
                    ))
                }
            }
        }

        let leading_bits = self.get_leading_bits();
        if self.config.check_rsv && !(leading_bits == 0b00001000 || leading_bits == 0b00000000) {
            return Err(WsError::ProtocolError {
                close_code: 1008,
                error: ProtocolError::InvalidLeadingBits(leading_bits),
            });
        }
        let ava_data = self.buf.ava_data();
        let (len_occ_bytes, payload_len) =
            parse_payload_len(ava_data).map_err(|e| WsError::ProtocolError {
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
        let mask = get_bit(ava_data, 1, 0);
        let header_len = 1 + len_occ_bytes + if mask { 4 } else { 0 };
        Ok((header_len, payload_len, header_len + payload_len))
    }

    /// get a frame and reset state
    #[inline]
    pub fn consume_frame(
        &mut self,
        header_len: usize,
        payload_len: usize,
        total_len: usize,
    ) -> (bool, OpCode, &[u8]) {
        let ava_data = self.buf.ava_mut_data();
        let (header_data, remain) = ava_data.split_at_mut(header_len);
        let header = HeaderView(header_data);
        let payload = remain.split_at_mut(payload_len).0;
        let fin = header.fin();
        let code = header.opcode();
        if self.config.auto_unmask {
            if let Some(mask) = header.masking_key() {
                apply_mask(payload, mask)
            }
        }
        let s_idx = self.buf.consume_idx + header_len;
        let e_idx = s_idx + payload_len;
        self.buf.consume(total_len);
        (fin, code, &self.buf.buf[s_idx..e_idx])
    }
}

impl ReactorRead {}

struct FrameBuffer {
    buf: Vec<u8>,
    produce_idx: usize,
    consume_idx: usize,
}

impl FrameBuffer {
    fn new() -> Self {
        Self {
            buf: vec![0; 4096],
            produce_idx: 0,
            consume_idx: 0,
        }
    }

    fn prepare(&mut self, payload_size: usize) -> &mut [u8] {
        let remain = self.buf.len() - self.produce_idx;
        if remain >= payload_size {
            &mut self.buf[self.produce_idx..(self.produce_idx + payload_size)]
        } else {
            if (self.produce_idx - self.consume_idx) * 2 > self.buf.len() {
                self.buf.resize(payload_size - remain, 0);
                &mut self.buf[self.produce_idx..(self.produce_idx + payload_size)]
            } else {
                let not_consumed_data = self.buf[self.consume_idx..self.produce_idx].to_vec();
                if payload_size + not_consumed_data.len() > self.buf.len() {
                    self.buf.resize(payload_size + not_consumed_data.len(), 0);
                }
                self.buf[..(not_consumed_data.len())].copy_from_slice(&not_consumed_data);
                self.consume_idx = 0;
                self.produce_idx = not_consumed_data.len();
                &mut self.buf[self.produce_idx..(self.produce_idx + payload_size)]
            }
        }
    }

    fn ava_data(&self) -> &[u8] {
        &self.buf[self.consume_idx..self.produce_idx]
    }

    fn ava_mut_data(&mut self) -> &mut [u8] {
        &mut self.buf[self.consume_idx..self.produce_idx]
    }

    fn produce(&mut self, num: usize) {
        self.produce_idx += num;
    }

    fn consume(&mut self, num: usize) {
        self.consume_idx += num;
    }
}

impl ReactorRead {
    /// **NOTE** masked frame has already been unmasked
    pub fn receive<S: Read>(&mut self, stream: &mut S) -> Result<(bool, OpCode, &[u8]), WsError> {
        let buf = self.read_one_frame(stream)?;
        Ok(buf)
    }

    #[inline]
    fn read_one_frame<S: Read>(
        &mut self,
        stream: &mut S,
    ) -> Result<(bool, OpCode, &[u8]), WsError> {
        while !self.is_header_ok() {
            self.poll(stream)?;
        }
        let (header_len, payload_len, total_len) = self.parse_frame_header()?;
        self.poll_one_frame(stream, total_len)?;
        Ok(self.consume_frame(header_len, payload_len, total_len))
    }

    #[inline]
    fn poll<S: Read>(&mut self, stream: &mut S) -> std::io::Result<usize> {
        let buf = self.buf.prepare(self.config.resize_size);
        let count = stream.read(buf)?;
        self.buf.produce(count);
        if count == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::ConnectionAborted,
                "read eof",
            ));
        }
        Ok(count)
    }

    #[inline]
    fn poll_one_frame<S: Read>(&mut self, stream: &mut S, size: usize) -> std::io::Result<()> {
        let read_len = self.buf.ava_data().len();
        if read_len < size {
            let buf = self.buf.prepare(size - read_len);
            stream.read_exact(buf)?;
            self.buf.produce(size - read_len);
        }
        Ok(())
    }
}

/// recv/send websocket frame
pub struct ReactorFrameCodec<S: Read + Write> {
    /// underlying transport stream
    pub stream: S,
    /// read state
    pub read_state: ReactorRead,
    /// write state
    pub write_state: FrameWriteState,
}

impl<S: Read + Write> ReactorFrameCodec<S> {
    /// construct method
    pub fn new(stream: S) -> Self {
        Self {
            stream,
            read_state: ReactorRead::default(),
            write_state: FrameWriteState::default(),
        }
    }

    /// construct with stream and config
    pub fn new_with(stream: S, config: FrameConfig) -> Self {
        Self {
            stream,
            read_state: ReactorRead::with_config(config.clone()),
            write_state: FrameWriteState::with_config(config),
        }
    }

    /// get mutable underlying stream
    pub fn stream_mut(&mut self) -> &mut S {
        &mut self.stream
    }

    /// used for server side to construct a new server
    pub fn factory(_req: http::Request<()>, stream: S) -> Result<Self, WsError> {
        let config = FrameConfig {
            mask_send_frame: false,
            ..Default::default()
        };
        Ok(Self::new_with(stream, config))
    }

    /// used to client side to construct a new client
    pub fn check_fn(key: String, resp: http::Response<()>, stream: S) -> Result<Self, WsError> {
        crate::protocol::standard_handshake_resp_check(key.as_bytes(), &resp)?;
        Ok(Self::new_with(stream, Default::default()))
    }

    /// receive a frame
    pub fn receive(&mut self) -> Result<(bool, OpCode, &[u8]), WsError> {
        self.read_state.receive(&mut self.stream)
    }

    /// send data, **will copy data if need mask**
    pub fn send(&mut self, code: OpCode, payload: &[u8]) -> Result<(), WsError> {
        self.write_state
            .send(&mut self.stream, code, payload)
            .map_err(WsError::IOError)
    }

    /// send a read frame, **this method will not check validation of frame and do not fragment**
    pub fn send_owned_frame(&mut self, frame: OwnedFrame) -> Result<(), WsError> {
        self.write_state
            .send_owned_frame(&mut self.stream, frame)
            .map_err(WsError::IOError)
    }

    /// flush stream to ensure all data are send
    pub fn flush(&mut self) -> Result<(), WsError> {
        self.stream.flush().map_err(WsError::IOError)
    }
}

/// recv part of websocket stream
pub struct ReactorFrameRecv<S: Read> {
    stream: S,
    read_state: ReactorRead,
}

impl<S: Read> ReactorFrameRecv<S> {
    /// construct method
    pub fn new(stream: S, read_state: ReactorRead) -> Self {
        Self { stream, read_state }
    }

    /// receive a frame
    pub fn receive(&mut self) -> Result<(bool, OpCode, &[u8]), WsError> {
        self.read_state.receive(&mut self.stream)
    }
}

/// websocket read state
#[derive(Debug, Clone)]
pub struct FrameReadState {
    fragmented: bool,
    config: FrameConfig,
    read_idx: usize,
    read_data: BytesMut,
    fragmented_data: OwnedFrame,
    fragmented_type: OpCode,
}

impl Default for FrameReadState {
    fn default() -> Self {
        Self {
            config: FrameConfig::default(),
            fragmented: false,
            read_idx: 0,
            read_data: BytesMut::new(),
            fragmented_data: OwnedFrame::new(OpCode::Binary, None, &[]),
            fragmented_type: OpCode::default(),
        }
    }
}

impl FrameReadState {
    /// construct with config
    pub fn with_config(config: FrameConfig) -> Self {
        Self {
            config,
            ..Self::default()
        }
    }

    /// check if data in buffer is enough to parse frame header
    #[inline]
    pub fn is_header_ok(&self) -> bool {
        let buf_len = self.read_idx;
        if buf_len < 2 {
            false
        } else {
            let len = self.read_data[1] & 0b01111111;
            let mask = get_bit(&self.read_data, 1, 0);

            let mut min_len = match len {
                0..=125 => 2,
                126 => 4,
                127 => 10,
                _ => unreachable!(),
            };
            if mask {
                min_len += 4;
            }
            buf_len >= min_len
        }
    }

    /// return current frame header bits of buffer
    #[inline]
    pub fn get_leading_bits(&self) -> u8 {
        self.read_data[0] >> 4
    }

    /// try to parse frame header in buffer, return (header_len, payload_len, header_len + payload_len)
    #[inline]
    pub fn parse_frame_header(&mut self) -> Result<(usize, usize, usize), WsError> {
        fn parse_payload_len(source: &[u8]) -> Result<(usize, usize), ProtocolError> {
            match source[1] {
                len @ (0..=125 | 128..=253) => Ok((1, (len & 127) as usize)),
                126 | 254 => {
                    if source.len() < 4 {
                        return Err(ProtocolError::InsufficientLen(source.len()));
                    }
                    Ok((
                        1 + 2,
                        u16::from_be_bytes((&source[2..4]).try_into().unwrap()) as usize,
                    ))
                }
                127 | 255 => {
                    if source.len() < 10 {
                        return Err(ProtocolError::InsufficientLen(source.len()));
                    }
                    Ok((
                        1 + 8,
                        usize::from_be_bytes((&source[2..(8 + 2)]).try_into().unwrap()),
                    ))
                }
            }
        }

        let leading_bits = self.get_leading_bits();
        if self.config.check_rsv && !(leading_bits == 0b00001000 || leading_bits == 0b00000000) {
            return Err(WsError::ProtocolError {
                close_code: 1008,
                error: ProtocolError::InvalidLeadingBits(leading_bits),
            });
        }
        let (len_occ_bytes, payload_len) =
            parse_payload_len(&self.read_data).map_err(|e| WsError::ProtocolError {
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
        let mask = get_bit(&self.read_data, 1, 0);
        let header_len = 1 + len_occ_bytes + if mask { 4 } else { 0 };
        Ok((header_len, payload_len, header_len + payload_len))
    }

    /// get a frame and reset state
    #[inline]
    pub fn consume_frame(
        &mut self,
        header_len: usize,
        payload_len: usize,
        total_len: usize,
    ) -> (bool, OpCode, u64, OwnedFrame) {
        let header = Header(self.read_data.split_to(header_len));
        let payload = self.read_data.split_to(payload_len);
        let fin = header.fin();
        let code = header.opcode();
        self.read_idx -= total_len;
        let mut frame = OwnedFrame { header, payload };
        if self.config.auto_unmask {
            frame.unmask();
        }
        (fin, code, payload_len as u64, frame)
    }

    /// This method is technically private, but custom parsers are allowed to use it.
    #[doc(hidden)]
    #[inline]
    pub fn merge_frame(
        &mut self,
        fin: bool,
        opcode: OpCode,
        checked_frame: OwnedFrame,
    ) -> Result<Option<OwnedFrame>, WsError> {
        match opcode {
            OpCode::Continue => {
                self.fragmented_data
                    .extend_from_slice(checked_frame.payload());
                if fin {
                    let completed_frame = std::mem::replace(
                        &mut self.fragmented_data,
                        OwnedFrame::new(OpCode::Binary, None, &[]),
                    );
                    self.fragmented = false;
                    Ok(Some(completed_frame))
                } else {
                    Ok(None)
                }
            }
            OpCode::Text | OpCode::Binary => {
                if !fin {
                    self.fragmented = true;
                    self.fragmented_type = opcode;
                    self.fragmented_data.header_mut().set_opcode(opcode);
                    self.fragmented_data
                        .extend_from_slice(checked_frame.payload());
                    Ok(None)
                } else {
                    Ok(Some(checked_frame))
                }
            }
            OpCode::Close | OpCode::Ping | OpCode::Pong => Ok(Some(checked_frame)),
            _ => unreachable!(),
        }
    }

    /// perform protocol checking after receiving a frame
    ///
    /// This method is technically private, but custom parsers are allowed to use it.
    #[doc(hidden)]
    #[inline]
    pub fn check_frame(
        &mut self,
        fin: bool,
        opcode: OpCode,
        payload_len: u64,
        unmasked_frame: OwnedFrame,
    ) -> Result<OwnedFrame, WsError> {
        match opcode {
            OpCode::Continue => {
                if !self.fragmented {
                    return Err(WsError::ProtocolError {
                        close_code: 1002,
                        error: ProtocolError::MissInitialFragmentedFrame,
                    });
                }
                if fin {
                    self.fragmented = false;
                }
                Ok(unmasked_frame)
            }
            OpCode::Binary => {
                if self.fragmented {
                    return Err(WsError::ProtocolError {
                        close_code: 1002,
                        error: ProtocolError::NotContinueFrameAfterFragmented,
                    });
                }
                self.fragmented = !fin;
                Ok(unmasked_frame)
            }
            OpCode::Text => {
                if self.fragmented {
                    return Err(WsError::ProtocolError {
                        close_code: 1002,
                        error: ProtocolError::NotContinueFrameAfterFragmented,
                    });
                }
                if !fin {
                    self.fragmented = true;
                    if opcode == OpCode::Text
                        && self.config.validate_utf8.is_fast_fail()
                        && simdutf8::basic::from_utf8(unmasked_frame.payload()).is_err()
                    {
                        return Err(WsError::ProtocolError {
                            close_code: 1007,
                            error: ProtocolError::InvalidUtf8,
                        });
                    }

                    Ok(unmasked_frame)
                } else {
                    if opcode == OpCode::Text
                        && self.config.validate_utf8.should_check()
                        && simdutf8::basic::from_utf8(unmasked_frame.payload()).is_err()
                    {
                        return Err(WsError::ProtocolError {
                            close_code: 1007,
                            error: ProtocolError::InvalidUtf8,
                        });
                    }
                    Ok(unmasked_frame)
                }
            }
            OpCode::Close | OpCode::Ping | OpCode::Pong => {
                if !fin {
                    return Err(WsError::ProtocolError {
                        close_code: 1002,
                        error: ProtocolError::FragmentedControlFrame,
                    });
                }
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
                        let payload = unmasked_frame.payload();

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
                Ok(unmasked_frame)
            }
            _ => Err(WsError::UnsupportedFrame(opcode)),
        }
    }
}

/// websocket writing state
#[allow(dead_code)]
#[derive(Debug, Clone, Default)]
pub struct FrameWriteState {
    config: FrameConfig,
    header_buf: [u8; 14],
    buf: BytesMut,
}

impl FrameWriteState {
    /// construct with config
    pub fn with_config(config: FrameConfig) -> Self {
        Self {
            config,
            header_buf: [0; 14],
            buf: BytesMut::new(),
        }
    }
}

/// do standard handshake check and return response
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
