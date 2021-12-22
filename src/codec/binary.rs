use crate::errors::WsError;
use crate::frame::{Frame, OpCode};
use crate::protocol::standard_handshake_resp_check;
use crate::stream::WsStream;
use bytes::BytesMut;

use std::fmt::Debug;
use tokio::io::{ReadHalf, WriteHalf};
use tokio_util::codec::{Decoder, Encoder, Framed, FramedRead, FramedWrite};

use super::{SplitSocket, WebSocketFrameCodec, WebSocketFrameDecoder, WebSocketFrameEncoder};

#[derive(Debug, Clone, Default)]
pub struct WebSocketBytesEncoder {
    pub frame_encoder: WebSocketFrameEncoder,
}

#[derive(Debug, Clone, Default)]
pub struct WebSocketBytesDecoder {
    pub frame_decoder: WebSocketFrameDecoder,
}

#[derive(Debug, Clone, Default)]
pub struct WebSocketBytesCodec {
    pub frame_codec: WebSocketFrameCodec,
}

pub fn default_bytes_check_fn(
    key: String,
    resp: http::Response<()>,
    stream: WsStream,
) -> Result<Framed<WsStream, WebSocketBytesCodec>, WsError> {
    standard_handshake_resp_check(key.as_bytes(), &resp)?;
    Ok(Framed::new(stream, WebSocketBytesCodec::default()))
}

#[allow(dead_code)]
pub fn default_bytes_codec_factory(
    _req: http::Request<()>,
    stream: WsStream,
) -> Result<Framed<WsStream, WebSocketBytesCodec>, WsError> {
    let mut frame_codec = WebSocketFrameCodec::default();
    // do not mask server side frame
    frame_codec.config.mask = false;
    Ok(Framed::new(stream, WebSocketBytesCodec { frame_codec }))
}

impl Encoder<(OpCode, BytesMut)> for WebSocketBytesEncoder {
    type Error = WsError;

    fn encode(&mut self, item: (OpCode, BytesMut), dst: &mut BytesMut) -> Result<(), Self::Error> {
        let frame = Frame::new_with_payload(item.0, &item.1);
        self.frame_encoder.encode(frame, dst)
    }
}

impl Encoder<(OpCode, BytesMut)> for WebSocketBytesCodec {
    type Error = WsError;

    fn encode(&mut self, item: (OpCode, BytesMut), dst: &mut BytesMut) -> Result<(), Self::Error> {
        let frame = Frame::new_with_payload(item.0, &item.1);
        self.frame_codec.encode(frame, dst)
    }
}

impl Encoder<BytesMut> for WebSocketBytesEncoder {
    type Error = WsError;

    fn encode(&mut self, item: BytesMut, dst: &mut BytesMut) -> Result<(), Self::Error> {
        let frame = Frame::new_with_payload(OpCode::Binary, &item);
        self.frame_encoder.encode(frame, dst)
    }
}

impl Encoder<BytesMut> for WebSocketBytesCodec {
    type Error = WsError;

    fn encode(&mut self, item: BytesMut, dst: &mut BytesMut) -> Result<(), Self::Error> {
        let frame = Frame::new_with_payload(OpCode::Binary, &item);
        self.frame_codec.encode(frame, dst)
    }
}

impl Decoder for WebSocketBytesDecoder {
    type Item = (OpCode, BytesMut);

    type Error = WsError;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        self.frame_decoder.decode(src).map(|maybe_frame| {
            maybe_frame.map(|frame| {
                let mut data = BytesMut::new();
                data.extend_from_slice(&frame.payload_data_unmask());
                (frame.opcode(), data)
            })
        })
    }
}

impl Decoder for WebSocketBytesCodec {
    type Item = (OpCode, BytesMut);

    type Error = WsError;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        self.frame_codec.decode(src).map(|maybe_frame| {
            maybe_frame.map(|frame| {
                let mut data = BytesMut::new();
                data.extend_from_slice(&frame.payload_data_unmask());
                (frame.opcode(), data)
            })
        })
    }
}

impl SplitSocket<BytesMut, (OpCode, BytesMut), WebSocketBytesEncoder, WebSocketBytesDecoder>
    for Framed<WsStream, WebSocketBytesCodec>
{
    fn split(
        self,
    ) -> (
        FramedRead<ReadHalf<WsStream>, WebSocketBytesDecoder>,
        FramedWrite<WriteHalf<WsStream>, WebSocketBytesEncoder>,
    ) {
        let parts = self.into_parts();
        let (read_io, write_io) = tokio::io::split(parts.io);
        let codec = parts.codec.frame_codec;
        let mut frame_read = FramedRead::new(
            read_io,
            WebSocketBytesDecoder {
                frame_decoder: WebSocketFrameDecoder {
                    config: codec.config.clone(),
                    fragmented: codec.fragmented,
                    fragmented_data: codec.fragmented_data,
                    fragmented_type: codec.fragmented_type,
                },
            },
        );
        *frame_read.read_buffer_mut() = parts.read_buf;
        let mut frame_write = FramedWrite::new(
            write_io,
            WebSocketBytesEncoder {
                frame_encoder: WebSocketFrameEncoder {
                    config: codec.config,
                },
            },
        );
        *frame_write.write_buffer_mut() = parts.write_buf;
        (frame_read, frame_write)
    }
}
