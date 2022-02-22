use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use bytes::{Buf, BytesMut};

use crate::{
    errors::{ProtocolError, WsError},
    frame::{get_bit, parse_opcode, parse_payload_len, Frame, OpCode},
};

use super::{FrameReadState, FrameConfig};

type IOResult<T> = std::io::Result<T>;
pub struct AWebSocketFrameCodec<S: AsyncRead + AsyncWrite> {
    stream: S,
    state: FrameReadState,
}

impl<S: AsyncRead + AsyncWrite + Unpin> AWebSocketFrameCodec<S> {
    pub fn new(stream: S) -> Self {
        Self {
            stream,
            state: FrameReadState::new(),
        }
    }

    pub async fn poll(&mut self) -> IOResult<usize> {
        let state = &mut self.state;
        let count = self.stream.read(&mut state.read_buf).await?;
        Ok(count)
    }

    async fn read_single_frame(&mut self) -> Result<Frame, WsError> {
        while !self.state.leading_bits_ok() {
            self.poll().await?;
        }
        // TODO check nonzero value according to extension negotiation
        let leading_bits = self.state.get_leading_bits();
        if self.state.config.check_rsv
            && !(leading_bits == 0b00001000 || leading_bits == 0b00000000)
        {
            return Err(WsError::ProtocolError {
                close_code: 1008,
                error: ProtocolError::InvalidLeadingBits(leading_bits),
            });
        }
        parse_opcode(self.state.read_buf[0]).map_err(|e| WsError::ProtocolError {
            error: ProtocolError::InvalidOpcode(e),
            close_code: 1008,
        })?;
        let (payload_len, len_occ_bytes) = parse_payload_len(self.state.read_buf.as_ref())
            .map_err(|e| WsError::ProtocolError {
                close_code: 1008,
                error: e,
            })?;
        let max_payload_size = self.state.config.max_frame_payload_size;
        if max_payload_size > 0 && payload_len > max_payload_size {
            return Err(WsError::ProtocolError {
                close_code: 1008,
                error: ProtocolError::PayloadTooLarge(max_payload_size),
            });
        }
        let mut expected_len = 1 + len_occ_bytes + payload_len;
        let mask = get_bit(&self.state.read_buf, 1, 0);
        if mask {
            expected_len += 4;
        }
        while !self.state.body_ok(expected_len) {
            self.poll().await?;
        }
        let mut data = BytesMut::with_capacity(expected_len);
        data.extend_from_slice(&self.state.read_buf[..expected_len]);
        self.state.read_buf.advance(expected_len);
        Ok(Frame(data))
    }

    pub async fn receive(&mut self) -> Result<Frame, WsError> {
        loop {
            let frame = self.read_single_frame().await?;
            let opcode = frame.opcode();
            match opcode {
                OpCode::Continue => {
                    if !self.state.fragmented {
                        return Err(WsError::ProtocolError {
                            close_code: 1002,
                            error: ProtocolError::MissInitialFragmentedFrame,
                        });
                    }
                    self.state
                        .fragmented_data
                        .extend_from_slice(&frame.payload_data_unmask());
                    if frame.fin() {
                        // if String::from_utf8(fragmented_data.to_vec()).is_err() {
                        //     return Err(WsError::ProtocolError {
                        //         close_code: 1007,
                        //         error: ProtocolError::InvalidUtf8,
                        //     });
                        // }
                        let completed_frame = Frame::new_with_payload(
                            self.state.fragmented_type.clone(),
                            &self.state.fragmented_data,
                        );
                        break Ok(completed_frame);
                    } else {
                        continue;
                    }
                }
                OpCode::Text | OpCode::Binary => {
                    if self.state.fragmented {
                        return Err(WsError::ProtocolError {
                            close_code: 1002,
                            error: ProtocolError::NotContinueFrameAfterFragmented,
                        });
                    }
                    if !frame.fin() {
                        self.state.fragmented = true;
                        self.state.fragmented_type = opcode.clone();
                        let payload = frame.payload_data_unmask();
                        self.state.fragmented_data.extend_from_slice(&payload);
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
                    if opcode == OpCode::Close || !self.state.fragmented {
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
