use std::io::Read;

use crate::errors::ProtocolError;
use crate::frame::{get_bit, parse_opcode, parse_payload_len, Frame};
use crate::stream::WsStream;
use crate::{errors::WsError, frame::OpCode};
use bytes::{Buf, BytesMut};
use tokio::io::{AsyncRead, AsyncReadExt, ReadHalf, WriteHalf};
use tokio_util::codec::{Decoder, Encoder, FramedRead, FramedWrite};

mod binary;
#[cfg(feature = "deflate")]
mod deflate;
mod frame;
mod text;

mod non_block;
mod sync;

pub use binary::*;
#[cfg(feature = "deflate")]
pub use deflate::*;
pub use frame::*;
pub use text::*;
pub trait SplitSocket<EI, DI, E, D>
where
    E: Encoder<EI, Error = WsError>,
    D: Decoder<Item = DI, Error = WsError>,
{
    fn split(
        self,
    ) -> (
        FramedRead<ReadHalf<WsStream>, D>,
        FramedWrite<WriteHalf<WsStream>, E>,
    );
}

struct FrameReadState {
    fragmented: bool,
    config: FrameConfig,
    read_buf: BytesMut,
    fragmented_data: BytesMut,
    fragmented_type: OpCode,
}

type IOResult<T> = std::io::Result<T>;

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
