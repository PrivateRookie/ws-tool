use std::io::{Read, Write};

use crate::{
    codec::{FrameConfig, WsFrameCodec},
    errors::{ProtocolError, WsError},
    frame::{OpCode, ReadFrame},
    protocol::standard_handshake_resp_check,
};

use super::{Compressor, DeCompressor, PMGConfig, EXT_ID};

/// helper struct to handle com/de stream
#[allow(missing_docs)]
pub struct StreamHandler {
    pub config: PMGConfig,
    pub com: Compressor,
    pub de: DeCompressor,
}

/// recv/send deflate message
pub struct WsDeflateCodec<S: Read + Write> {
    frame_codec: WsFrameCodec<S>,
    stream_handler: Option<StreamHandler>,
}

impl<S: Read + Write> WsDeflateCodec<S> {
    /// construct method
    pub fn new(frame_codec: WsFrameCodec<S>, config: Option<PMGConfig>, is_server: bool) -> Self {
        let stream_handler = if let Some(config) = config {
            let com_size = if is_server {
                config.client_max_window_bits
            } else {
                config.server_max_window_bits
            };
            let com = Compressor::new(com_size);
            let de_size = if is_server {
                config.client_max_window_bits
            } else {
                config.server_max_window_bits
            };
            let de = DeCompressor::new(de_size);
            Some(StreamHandler { config, com, de })
        } else {
            None
        };
        Self {
            frame_codec,
            stream_handler,
        }
    }

    /// used for server side to construct a new server
    pub fn factory(req: http::Request<()>, stream: S) -> Result<Self, WsError> {
        let frame_config = FrameConfig {
            mask_send_frame: false,
            ..Default::default()
        };
        let mut configs: Vec<PMGConfig> = vec![];
        for (k, v) in req.headers() {
            if k.as_str().to_lowercase() == EXT_ID {
                if let Ok(s) = v.to_str() {
                    match PMGConfig::parse_str(s) {
                        Ok(mut conf) => {
                            configs.append(&mut conf);
                        }
                        Err(e) => return Err(WsError::HandShakeFailed(e)),
                    }
                }
            }
        }
        let config = configs.pop();
        let frame_codec = WsFrameCodec::new_with(stream, frame_config);
        let codec = WsDeflateCodec::new(frame_codec, config, true);
        Ok(codec)
    }

    /// used for client side to construct a new client
    pub fn check_fn(key: String, resp: http::Response<()>, stream: S) -> Result<Self, WsError> {
        standard_handshake_resp_check(key.as_bytes(), &resp)?;
        let mut configs: Vec<PMGConfig> = vec![];
        for (k, v) in resp.headers() {
            if k.as_str().to_lowercase() == EXT_ID {
                if let Ok(s) = v.to_str() {
                    match PMGConfig::parse_str(s) {
                        Ok(mut conf) => {
                            configs.append(&mut conf);
                        }
                        Err(e) => return Err(WsError::HandShakeFailed(e)),
                    }
                }
            }
        }
        let config = configs.pop();
        let frame_codec = WsFrameCodec::new_with(stream, Default::default());
        let codec = WsDeflateCodec::new(frame_codec, config, true);
        Ok(codec)
    }

    /// get mutable underlying stream
    pub fn stream_mut(&mut self) -> &mut S {
        self.frame_codec.stream_mut()
    }

    pub fn receive(&mut self) -> Result<ReadFrame, WsError> {
        let frame = self.frame_codec.receive()?;
        let compressed = frame.header().rsv1();
        let is_data_frame = matches!(frame.header().opcode(), OpCode::Text | OpCode::Binary);
        if compressed && !is_data_frame {
            return Err(WsError::ProtocolError {
                close_code: 1000,
                error: ProtocolError::CompressedControlFrame,
            });
        }
        todo!()
    }
}
