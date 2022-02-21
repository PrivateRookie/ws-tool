use std::io::{Read, Write};

use bytes::{Buf, BytesMut};

use crate::{
    errors::{ProtocolError, WsError},
    frame::{get_bit, parse_opcode, parse_payload_len, Frame, OpCode},
};

use super::FrameConfig;

type IOResult<T> = std::io::Result<T>;

pub struct WebSocketFrameCodec<S: Read + Write> {
    stream: S,
    fragmented: bool,
    config: FrameConfig,
    read_buf: BytesMut,
    fragmented_data: BytesMut,
    fragmented_type: OpCode,
}

impl<S: Read + Write> WebSocketFrameCodec<S> {
    pub fn new(stream: S) -> Self {
        Self {
            stream,
            config: FrameConfig::default(),
            fragmented: false,
            read_buf: BytesMut::with_capacity(1024 * 4),
            fragmented_data: BytesMut::new(),
            fragmented_type: OpCode::default(),
        }
    }

    fn poll(&mut self) -> IOResult<usize> {
        self.stream.read(&mut self.read_buf)
    }

    fn read_single_frame(&mut self) -> Result<Frame, WsError> {
        while self.read_buf.len() < 2 {
            self.poll()?;
        }

        // TODO check nonzero value according to extension negotiation
        let leading_bits = self.read_buf[0] >> 4;
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
        }
        while expected_len > self.read_buf.len() {
            self.poll()?;
        }
        let mut data = BytesMut::with_capacity(expected_len);
        data.extend_from_slice(&self.read_buf[..expected_len]);
        self.read_buf.advance(expected_len);
        Ok(Frame(data))
    }

    pub fn receive(&mut self) -> Result<Frame, WsError> {
        loop {
            let frame = self.read_single_frame()?;
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
                        break Ok(completed_frame);
                    } else {
                        continue;
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
                        continue;
                    } else {
                        // if opcode == OpCode::Text
                        //     && String::from_utf8(frame.payload_data_unmask().to_vec()).is_err()
                        // {
                        //     return Err(WsError::ProtocolError {
                        //         close_code: 1007,
                        //         error: ProtocolError::InvalidUtf8,
                        //     });
                        // }
                        break Ok(frame);
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
                        break Ok(frame);
                    } else {
                        tracing::debug!("{:?} frame between self.fragmented data", opcode);
                        break Ok(frame);
                    }
                }
                OpCode::ReservedNonControl | OpCode::ReservedControl => {
                    return Err(WsError::UnsupportedFrame(opcode));
                }
            }
        }
    }
}
