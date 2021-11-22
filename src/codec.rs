use crate::errors::{ProtocolError, WsError};
use crate::frame::{get_bit, parse_opcode, parse_payload_len, Frame, OpCode};
use bytes::{Buf, BytesMut};
use std::io::{Error as IOError, ErrorKind::InvalidData};
use std::{fmt::Debug, ops::Deref};
use tokio_util::codec::{Decoder, Encoder};

/// default websocket frame encoder
#[derive(Debug, Clone)]
pub struct FrameEncoder {}

impl Default for FrameEncoder {
    fn default() -> Self {
        Self {}
    }
}

impl Encoder<Frame> for FrameEncoder {
    type Error = WsError;

    fn encode(&mut self, item: Frame, dst: &mut BytesMut) -> Result<(), Self::Error> {
        dst.extend_from_slice(&item.0);
        Ok(())
    }
}

/// default websocket frame decoder
#[derive(Debug, Clone)]
pub struct FrameDecoder {
    pub check_rsv: bool,
    pub fragmented: bool,
    pub fragmented_data: BytesMut,
    pub fragmented_type: OpCode,
}

impl Default for FrameDecoder {
    fn default() -> Self {
        Self {
            check_rsv: true,
            fragmented: false,
            fragmented_data: Default::default(),
            fragmented_type: OpCode::Text,
        }
    }
}

impl FrameDecoder {
    fn decode_single(&mut self, src: &mut BytesMut) -> Result<Option<Frame>, IOError> {
        if src.len() < 2 {
            return Ok(None);
        }
        // TODO check nonzero value according to extension negotiation
        let leading_bits = src[0] >> 4;
        if self.check_rsv && !(leading_bits == 0b00001000 || leading_bits == 0b00000000) {
            return Err(IOError::new(
                InvalidData,
                ProtocolError::InvalidLeadingBits(leading_bits),
            ));
        }
        parse_opcode(src[0])
            .map_err(|e| IOError::new(InvalidData, ProtocolError::InvalidOpcode(e)))?;
        let (payload_len, len_occ_bytes) =
            parse_payload_len(src.deref()).map_err(|e| IOError::new(InvalidData, e))?;
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
}

impl Decoder for FrameDecoder {
    type Item = Frame;
    type Error = WsError;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        let maybe_frame = self.decode_single(src)?;
        if let Some(frame) = maybe_frame {
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
                        if String::from_utf8(self.fragmented_data.to_vec()).is_err() {
                            return Err(WsError::ProtocolError {
                                close_code: 1007,
                                error: ProtocolError::InvalidUtf8,
                            });
                        }
                        let completed_frame = Frame::new_with_payload(
                            self.fragmented_type.clone(),
                            &self.fragmented_data,
                        );
                        return Ok(Some(completed_frame));
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
                        if opcode == OpCode::Text
                            && String::from_utf8(frame.payload_data_unmask().to_vec()).is_err()
                        {
                            return Err(WsError::ProtocolError {
                                close_code: 1007,
                                error: ProtocolError::InvalidUtf8,
                            });
                        }
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
                            tracing::debug!("{:?}", payload);

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
}

#[derive(Debug, Clone)]
pub struct FrameCodec {
    pub encoder: FrameEncoder,
    pub decoder: FrameDecoder,
}

impl Default for FrameCodec {
    fn default() -> Self {
        Self {
            encoder: Default::default(),
            decoder: Default::default(),
        }
    }
}

impl Encoder<Frame> for FrameCodec {
    type Error = WsError;

    fn encode(&mut self, item: Frame, dst: &mut BytesMut) -> Result<(), Self::Error> {
        self.encoder.encode(item, dst)
    }
}

impl Decoder for FrameCodec {
    type Item = Frame;

    type Error = WsError;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        self.decoder.decode(src)
    }
}
