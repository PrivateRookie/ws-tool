use crate::errors::{ProtocolError, WsError};
use crate::frame::{Frame, OpCode};
use crate::protocol::standard_handshake_resp_check;
use crate::stream::WsStream;
use bytes::BytesMut;

use std::fmt::Debug;
use tokio_util::codec::{Decoder, Encoder, Framed};

use super::{WebSocketFrameCodec, WebSocketFrameDecoder, WebSocketFrameEncoder};

#[derive(Debug, Clone, Default)]
pub struct WebSocketStringEncoder {
    pub frame_encoder: WebSocketFrameEncoder,
}

#[derive(Debug, Clone)]
pub struct WebSocketStringDecoder {
    pub frame_decoder: WebSocketFrameDecoder,
    pub validate_utf8: bool,
}

impl Default for WebSocketStringDecoder {
    fn default() -> Self {
        Self {
            frame_decoder: Default::default(),
            validate_utf8: true,
        }
    }
}

#[derive(Debug, Clone)]
pub struct WebSocketStringCodec {
    pub frame_codec: WebSocketFrameCodec,
    pub validate_utf8: bool,
}

impl Default for WebSocketStringCodec {
    fn default() -> Self {
        Self {
            frame_codec: Default::default(),
            validate_utf8: true,
        }
    }
}

pub fn default_string_check_fn(
    key: String,
    resp: http::Response<()>,
    stream: WsStream,
) -> Result<Framed<WsStream, WebSocketStringCodec>, WsError> {
    standard_handshake_resp_check(key.as_bytes(), &resp)?;
    Ok(Framed::new(stream, WebSocketStringCodec::default()))
}

pub fn default_string_codec_factory(
    _req: http::Request<()>,
    stream: WsStream,
) -> Result<Framed<WsStream, WebSocketStringCodec>, WsError> {
    let mut frame_codec = WebSocketFrameCodec::default();
    // do not mask server side frame
    frame_codec.config.mask = false;
    Ok(Framed::new(
        stream,
        WebSocketStringCodec {
            frame_codec,
            validate_utf8: true,
        },
    ))
}

impl Encoder<String> for WebSocketStringEncoder {
    type Error = WsError;

    fn encode(&mut self, item: String, dst: &mut BytesMut) -> Result<(), Self::Error> {
        let frame = Frame::new_with_payload(OpCode::Text, item.as_bytes());
        self.frame_encoder.encode(frame, dst)
    }
}

impl Encoder<String> for WebSocketStringCodec {
    type Error = WsError;

    fn encode(&mut self, item: String, dst: &mut BytesMut) -> Result<(), Self::Error> {
        let frame = Frame::new_with_payload(OpCode::Text, item.as_bytes());
        self.frame_codec.encode(frame, dst)
    }
}

impl Decoder for WebSocketStringDecoder {
    type Item = (OpCode, String);

    type Error = WsError;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        let maybe_frame = self.frame_decoder.decode(src)?;
        if let Some(frame) = maybe_frame {
            let data = frame.payload_data_unmask();
            let s = if self.validate_utf8 {
                String::from_utf8(data.to_vec()).map_err(|_| WsError::ProtocolError {
                    close_code: 1001,
                    error: ProtocolError::InvalidUtf8,
                })?
            } else {
                String::from_utf8_lossy(&data).to_string()
            };
            Ok(Some((frame.opcode(), s)))
        } else {
            Ok(None)
        }
    }
}

impl Decoder for WebSocketStringCodec {
    type Item = (OpCode, String);

    type Error = WsError;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        let maybe_frame = self.frame_codec.decode(src)?;
        if let Some(frame) = maybe_frame {
            let data = frame.payload_data_unmask();
            let s = if self.validate_utf8 {
                String::from_utf8(data.to_vec()).map_err(|_| WsError::ProtocolError {
                    close_code: 1001,
                    error: ProtocolError::InvalidUtf8,
                })?
            } else {
                String::from_utf8_lossy(&data).to_string()
            };
            Ok(Some((frame.opcode(), s)))
        } else {
            Ok(None)
        }
    }
}

// impl SplitSocket<String, (OpCode, String), WebSocketStringEncoder, WebSocketStringDecoder>
//     for Framed<WsStream, WebSocketStringCodec>
// {
//     fn split(
//         self,
//     ) -> (
//         FramedRead<ReadHalf<WsStream>, WebSocketStringDecoder>,
//         FramedWrite<WriteHalf<WsStream>, WebSocketStringEncoder>,
//     ) {
//         let parts = self.into_parts();
//         let (read_io, write_io) = tokio::io::split(parts.io);
//         let codec = parts.codec.frame_codec;
//         let mut frame_read = FramedRead::new(
//             read_io,
//             WebSocketStringDecoder {
//                 frame_decoder: WebSocketFrameDecoder {
//                     config: codec.config.clone(),
//                     fragmented: codec.fragmented,
//                     fragmented_data: codec.fragmented_data,
//                     fragmented_type: codec.fragmented_type,
//                 },
//                 validate_utf8: parts.codec.validate_utf8,
//             },
//         );
//         *frame_read.read_buffer_mut() = parts.read_buf;
//         let mut frame_write = FramedWrite::new(
//             write_io,
//             WebSocketStringEncoder {
//                 frame_encoder: WebSocketFrameEncoder {
//                     config: codec.config,
//                 },
//             },
//         );
//         *frame_write.write_buffer_mut() = parts.write_buf;
//         (frame_read, frame_write)
//     }
// }
