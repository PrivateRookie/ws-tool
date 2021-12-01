use crate::errors::WsError;
use crate::frame::{Frame, OpCode};
use crate::protocol::standard_handshake_resp_check;
use crate::stream::WsStream;
use bytes::BytesMut;

use std::fmt::Debug;
use tokio::io::{ReadHalf, WriteHalf};
use tokio_util::codec::{Decoder, Encoder, Framed, FramedRead, FramedWrite};

use super::{SplitSocket, WebSocketFrameCodec, WebSocketFrameDecoder, WebSocketFrameEncoder};

const EXT_NAME: &str = "permessage-deflate";

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

pub fn init_header(config: DeflateConfig) -> String {
    todo!()
}

#[derive(Debug, Clone, Default)]
pub struct WebSocketDeflateEncoder {
    pub deflate_config: DeflateConfig,
    pub frame_encoder: WebSocketFrameEncoder,
}

#[derive(Debug, Clone, Default)]
pub struct WebSocketDeflateDecoder {
    pub deflate_config: DeflateConfig,
    pub frame_decoder: WebSocketFrameDecoder,
}

#[derive(Debug, Clone, Default)]
pub struct WebSocketDeflateCodec {
    pub deflate_config: DeflateConfig,
    pub frame_codec: WebSocketFrameCodec,
}
