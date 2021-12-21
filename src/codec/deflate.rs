use crate::codec::FrameConfig;
use crate::errors::{ProtocolError, WsError};
use crate::frame::{Frame, OpCode};
use crate::protocol::standard_handshake_resp_check;
use crate::stream::WsStream;
use bytes::{Buf, Bytes, BytesMut};
use flate2::read::DeflateDecoder;
use flate2::write::DeflateEncoder;
use flate2::Compression;
use tracing::{debug, trace};

use std::fmt::Debug;
use std::io::{Read, Write};
use tokio::io::{ReadHalf, WriteHalf};
use tokio_util::codec::{Decoder, Encoder, Framed, FramedRead, FramedWrite};

use super::{SplitSocket, WebSocketFrameCodec, WebSocketFrameDecoder, WebSocketFrameEncoder};

const EXT_ID: &str = "permessage-deflate";

#[repr(u8)]
#[derive(Debug, Clone)]
pub enum WindowBit {
    Eight = 8,
    Night,
    Ten,
    Eleven,
    Twelve,
    Thirteen,
    Fourteen,
    FifTeen,
}

impl Default for WindowBit {
    fn default() -> Self {
        Self::Eight
    }
}

#[derive(Debug, Clone, Default)]
pub struct DeflateConfig {
    pub server_no_context_takeover: bool,
    pub client_no_context_takeover: bool,
    pub server_max_window_bits: Option<WindowBit>,
    pub client_mas_window_bits: Option<WindowBit>,
}
impl DeflateConfig {
    pub fn build_header(&self) -> String {
        let mut ext_header = vec![EXT_ID.to_string()];
        if self.server_no_context_takeover {
            ext_header.push("server_no_context_takeover".to_string());
        }
        if self.client_no_context_takeover {
            ext_header.push("client_no_context_takeover".to_string());
        }

        if let Some(bit) = self.server_max_window_bits.clone() {
            ext_header.push(format!("server_max_window_bits = {}", bit as u8))
        }
        if let Some(bit) = self.client_mas_window_bits.clone() {
            ext_header.push(format!("client_max_window_bits = {}", bit as u8))
        }
        ext_header.join(" ;")
    }
}

#[derive(Debug, Clone, Default)]
pub struct WebSocketDeflateEncoder {
    pub enable: bool,
    pub deflate_config: DeflateConfig,
    pub frame_encoder: WebSocketFrameEncoder,
}

#[derive(Debug, Clone, Default)]
pub struct WebSocketDeflateDecoder {
    pub enable: bool,
    pub deflate_config: DeflateConfig,
    pub frame_decoder: WebSocketFrameDecoder,
}

#[derive(Debug, Clone, Default)]
pub struct WebSocketDeflateCodec {
    pub enable: bool,
    pub deflate_config: DeflateConfig,
    pub codec: WebSocketFrameCodec,
}

fn encode_frame(enable: bool, item: (OpCode, BytesMut)) -> Frame {
    match &item.0 {
        OpCode::Text | OpCode::Binary if enable => {
            let mut deflate_encoder =
                DeflateEncoder::new(Vec::with_capacity(item.1.len()), Compression::fast());
            deflate_encoder.write(item.1.as_ref()).unwrap();
            let compressed = deflate_encoder.finish().unwrap();
            println!("{:x?}", &compressed);
            let mut frame = Frame::new_with_payload(item.0, &compressed);
            frame.set_rsv1(true);
            frame

        }
        _ => Frame::new_with_payload(item.0, &item.1),
    }
}

impl Encoder<(OpCode, BytesMut)> for WebSocketDeflateEncoder {
    type Error = WsError;

    fn encode(&mut self, item: (OpCode, BytesMut), dst: &mut BytesMut) -> Result<(), Self::Error> {
        tracing::info!("{:?}", item.1);
        self.frame_encoder
            .encode(encode_frame(self.enable, item), dst)
    }
}

impl Encoder<(OpCode, BytesMut)> for WebSocketDeflateCodec {
    type Error = WsError;

    fn encode(&mut self, item: (OpCode, BytesMut), dst: &mut BytesMut) -> Result<(), Self::Error> {
        tracing::info!("{:?}", item.1);
        self.codec.encode(encode_frame(self.enable, item), dst)
    }
}

fn decode_deflate_frame(enable: bool, frame: Frame) -> Result<Option<(OpCode, BytesMut)>, WsError> {
    let op_code = frame.opcode();
    let compressed = frame.rsv1();

    if !(op_code == OpCode::Text || op_code == OpCode::Binary) && compressed {
        if !enable {
            return Err(WsError::ProtocolError {
                close_code: 1002,
                error: ProtocolError::NotDeflateDataWhileEnabled,
            });
        }
        return Err(WsError::ProtocolError {
            close_code: 1002,
            error: ProtocolError::InvalidOpcode(op_code as u8),
        });
    }
    if compressed {
        let mut data = vec![];
        let mut input = frame.payload_data_unmask().to_vec();
        tracing::debug!("{:?}, {:x?}", frame, input);
        input.extend([0x00, 0x00, 0xff, 0xff]);
        let mut deflate_decoder = DeflateDecoder::new(&input[..]);
        deflate_decoder.read_to_end(&mut data).unwrap();
        Ok(Some((op_code, BytesMut::from(&data[..]))))
    } else {
        let mut data = BytesMut::new();
        data.extend_from_slice(&frame.payload_data_unmask());
        Ok(Some((op_code, data)))
    }
}

impl Decoder for WebSocketDeflateDecoder {
    type Item = (OpCode, BytesMut);

    type Error = WsError;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        if let Some(frame) = self.frame_decoder.decode(src)? {
            decode_deflate_frame(self.enable, frame)
        } else {
            Ok(None)
        }
    }
}

impl Decoder for WebSocketDeflateCodec {
    type Item = (OpCode, BytesMut);

    type Error = WsError;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        if let Some(frame) = self.codec.decode(src)? {
            decode_deflate_frame(self.enable, frame)
        } else {
            Ok(None)
        }
    }
}

pub fn default_deflate_check_fn(
    key: String,
    resp: http::Response<()>,
    stream: WsStream,
) -> Result<Framed<WsStream, WebSocketDeflateCodec>, WsError> {
    standard_handshake_resp_check(key.as_bytes(), &resp)?;
    let enable = if let Some(ext) = resp.headers().get("Sec-WebSocket-Extensions") {
        let ext = ext.to_str().unwrap_or_default().to_lowercase();
        if ext.contains(EXT_ID) {
            // TODO handle more params
            // let deflate_ext: Vec<Vec<&str>> = ext
            //     .split(",")
            //     .filter(|seg| seg.contains(EXT_ID))
            //     .map(|seg| seg.split(";").map(|i| i.trim()).collect())
            //     .collect();
            true
        } else {
            tracing::debug!("server not support per message deflate");
            false
        }
    } else {
        false
    };
    let mut codec = WebSocketDeflateCodec {
        enable,
        ..Default::default()
    };
    codec.codec.config.check_rsv = false;
    debug!("{:#?}", codec);
    Ok(Framed::new(stream, codec))
}

pub fn default_bytes_codec_factory(
    req: http::Request<()>,
    stream: WsStream,
) -> Result<Framed<WsStream, WebSocketDeflateCodec>, WsError> {
    let enable = if let Some(ext) = req.headers().get("Sec-WebSocket-Extensions") {
        ext.to_str()
            .unwrap_or_default()
            .to_lowercase()
            .contains(EXT_ID)
    } else {
        false
    };
    let mut codec = WebSocketDeflateCodec {
        enable,
        ..Default::default()
    };
    codec.codec.config.mask = false;
    codec.codec.config.check_rsv = false;
    Ok(Framed::new(stream, codec))
}

impl
    SplitSocket<
        (OpCode, BytesMut),
        (OpCode, BytesMut),
        WebSocketDeflateEncoder,
        WebSocketDeflateDecoder,
    > for Framed<WsStream, WebSocketDeflateCodec>
{
    fn split(
        self,
    ) -> (
        FramedRead<ReadHalf<WsStream>, WebSocketDeflateDecoder>,
        FramedWrite<WriteHalf<WsStream>, WebSocketDeflateEncoder>,
    ) {
        let parts = self.into_parts();
        let (read_io, write_io) = tokio::io::split(parts.io);
        let codec = parts.codec.codec;
        let mut frame_read = FramedRead::new(
            read_io,
            WebSocketDeflateDecoder {
                frame_decoder: WebSocketFrameDecoder {
                    config: codec.config.clone(),
                    fragmented: codec.fragmented,
                    fragmented_data: codec.fragmented_data,
                    fragmented_type: codec.fragmented_type,
                },
                enable: parts.codec.enable,
                deflate_config: parts.codec.deflate_config.clone(),
            },
        );
        *frame_read.read_buffer_mut() = parts.read_buf;
        let mut frame_write = FramedWrite::new(
            write_io,
            WebSocketDeflateEncoder {
                frame_encoder: WebSocketFrameEncoder {
                    config: codec.config,
                },
                enable: parts.codec.enable,
                deflate_config: parts.codec.deflate_config,
            },
        );
        *frame_write.write_buffer_mut() = parts.write_buf;
        (frame_read, frame_write)
    }
}
