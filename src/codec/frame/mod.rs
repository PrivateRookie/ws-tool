use crate::errors::{ProtocolError, WsError};
use crate::frame::{get_bit, Header, HeaderView, OpCode, OwnedFrame};
use crate::protocol::{cal_accept_key, standard_handshake_req_check};
use bytes::BytesMut;
use std::fmt::Debug;

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
    /// if avalible len < resize, resize read buf, default 1K
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
    // apply_mask_fast32(buf, mask)
    apply_mask_simd(buf, mask)
}

fn apply_mask_simd(buf: &mut [u8], mask: [u8; 4]) {
    use std::simd::*;
    match buf.len() {
        0..=3 => apply_mask_fallback(buf, mask),
        4..=63 => apply_mask_fast32(buf, mask),
        _ => {
            let mask: [u8; 64] = std::array::from_fn(|idx| mask[idx % 3]);
            let mut buf: Simd<u8, 64> = Simd::from_slice(buf);
            let mask = Simd::from(mask);
            buf ^= mask;
        }
    }
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

pub struct FrameNewReadState {
    fragmented: bool,
    config: FrameConfig,
    header_buf: [u8; 14],
    masked_buf: BytesMut,
    fragmented_data: OwnedFrame,
    fragmented_type: OpCode,
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

impl Default for FrameNewReadState {
    fn default() -> Self {
        Self {
            fragmented: false,
            config: Default::default(),
            header_buf: Default::default(),
            masked_buf: Default::default(),
            fragmented_data: OwnedFrame::new(OpCode::Binary, None, &[]),
            fragmented_type: Default::default(),
        }
    }
}

impl FrameNewReadState {
    pub fn with_config(config: FrameConfig) -> Self {
        Self {
            config,
            ..Default::default()
        }
    }
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
    pub fn get_leading_bits(&self) -> u8 {
        self.read_data[0] >> 4
    }

    /// try to parse frame header in buffer, return expected payload
    pub fn parse_frame_header(&mut self) -> Result<usize, WsError> {
        fn parse_payload_len(source: &[u8]) -> Result<(usize, usize), ProtocolError> {
            let len = source[1] & 0b01111111;
            match len {
                0..=125 => Ok((1, len as usize)),
                126 => {
                    if source.len() < 4 {
                        return Err(ProtocolError::InsufficientLen(source.len()));
                    }
                    let mut arr = [0u8; 2];
                    arr[0] = source[2];
                    arr[1] = source[3];
                    Ok((1 + 2, u16::from_be_bytes(arr) as usize))
                }
                127 => {
                    if source.len() < 10 {
                        return Err(ProtocolError::InsufficientLen(source.len()));
                    }
                    let mut arr = [0u8; 8];
                    arr[..8].copy_from_slice(&source[2..(8 + 2)]);
                    Ok((1 + 8, usize::from_be_bytes(arr)))
                }
                _ => Err(ProtocolError::InvalidLeadingLen(len)),
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
        let mut expected_len = 1 + len_occ_bytes + payload_len;
        let mask = get_bit(&self.read_data, 1, 0);
        if mask {
            expected_len += 4;
        };
        Ok(expected_len)
    }

    /// get a frame and reset state
    pub fn consume_frame(&mut self, len: usize) -> OwnedFrame {
        let mut data = self.read_data.split_to(len);
        let view = HeaderView(&data);
        let payload_len = view.payload_len();
        let header = data.split_to(data.len() - payload_len as usize);
        self.read_idx -= len;
        let mut frame = OwnedFrame::with_raw(Header::raw(header), data);
        if self.config.auto_unmask {
            frame.unmask();
        }
        frame
    }

    /// This method is technically private, but custom parsers are allowed to use it.
    #[doc(hidden)]
    pub fn merge_frame(
        &mut self,
        checked_frame: OwnedFrame,
    ) -> Result<Option<OwnedFrame>, WsError> {
        let header = checked_frame.header();
        let opcode = header.opcode();

        match opcode {
            OpCode::Continue => {
                let fin = header.fin();
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
                if !header.fin() {
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
    pub fn check_frame(&mut self, unmasked_frame: OwnedFrame) -> Result<OwnedFrame, WsError> {
        let header = unmasked_frame.header();
        let opcode = header.opcode();
        match opcode {
            OpCode::Continue => {
                if !self.fragmented {
                    return Err(WsError::ProtocolError {
                        close_code: 1002,
                        error: ProtocolError::MissInitialFragmentedFrame,
                    });
                }
                if header.fin() {
                    self.fragmented = false;
                }
                Ok(unmasked_frame)
            }
            OpCode::Text | OpCode::Binary => {
                if self.fragmented {
                    return Err(WsError::ProtocolError {
                        close_code: 1002,
                        error: ProtocolError::NotContinueFrameAfterFragmented,
                    });
                }
                if !header.fin() {
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
                if !header.fin() {
                    return Err(WsError::ProtocolError {
                        close_code: 1002,
                        error: ProtocolError::FragmentedControlFrame,
                    });
                }
                let payload_len = header.payload_len();
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
