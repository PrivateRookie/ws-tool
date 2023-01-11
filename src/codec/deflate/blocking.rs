use std::io::{Read, Write};

use crate::{
    codec::{FrameConfig, WsFrameCodec},
    errors::{ProtocolError, WsError},
    frame::ReadFrame,
    protocol::standard_handshake_resp_check,
};

use super::{Compressor, DeCompressor, PMDConfig, EXT_ID};

/// helper struct to handle com/de stream
#[allow(missing_docs)]
pub struct StreamHandler {
    pub config: PMDConfig,
    pub com: Compressor,
    pub de: DeCompressor,
}

/// recv/send deflate message
pub struct WsDeflateCodec<S: Read + Write> {
    frame_codec: WsFrameCodec<S>,
    stream_handler: Option<StreamHandler>,
    is_server: bool,
}

impl<S: Read + Write> WsDeflateCodec<S> {
    /// construct method
    pub fn new(frame_codec: WsFrameCodec<S>, config: Option<PMDConfig>, is_server: bool) -> Self {
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
            is_server,
        }
    }

    /// used for server side to construct a new server
    pub fn factory(req: http::Request<()>, stream: S) -> Result<Self, WsError> {
        let frame_config = FrameConfig {
            mask_send_frame: false,
            check_rsv: false,
            ..Default::default()
        };
        let mut configs: Vec<PMDConfig> = vec![];
        for (k, v) in req.headers() {
            if k.as_str().to_lowercase() == "sec-websocket-extensions" {
                if let Ok(s) = v.to_str() {
                    match PMDConfig::parse_str(s) {
                        Ok(mut conf) => {
                            configs.append(&mut conf);
                        }
                        Err(e) => return Err(WsError::HandShakeFailed(e)),
                    }
                }
            }
        }
        let config = configs.pop();
        tracing::debug!("use deflate config {:?}", config);
        let frame_codec = WsFrameCodec::new_with(stream, frame_config);
        let codec = WsDeflateCodec::new(frame_codec, config, true);
        Ok(codec)
    }

    /// used for client side to construct a new client
    pub fn check_fn(key: String, resp: http::Response<()>, stream: S) -> Result<Self, WsError> {
        standard_handshake_resp_check(key.as_bytes(), &resp)?;
        let mut configs: Vec<PMDConfig> = vec![];
        for (k, v) in resp.headers() {
            if k.as_str().to_lowercase() == EXT_ID {
                if let Ok(s) = v.to_str() {
                    match PMDConfig::parse_str(s) {
                        Ok(mut conf) => {
                            configs.append(&mut conf);
                        }
                        Err(e) => return Err(WsError::HandShakeFailed(e)),
                    }
                }
            }
        }
        let config = configs.pop();
        let frame_codec = WsFrameCodec::new_with(
            stream,
            FrameConfig {
                check_rsv: false,
                mask_send_frame: false,
                ..Default::default()
            },
        );
        tracing::debug!("use deflate config: {:?}", config);
        let codec = WsDeflateCodec::new(frame_codec, config, false);
        Ok(codec)
    }

    /// get mutable underlying stream
    pub fn stream_mut(&mut self) -> &mut S {
        self.frame_codec.stream_mut()
    }

    /// receive a message
    pub fn receive(&mut self) -> Result<ReadFrame, WsError> {
        let frame = self.frame_codec.receive()?;
        let compressed = frame.header().rsv1();
        let is_data_frame = frame.header().opcode().is_data();
        if compressed && !is_data_frame {
            return Err(WsError::ProtocolError {
                close_code: 1000,
                error: ProtocolError::CompressedControlFrame,
            });
        }
        // TODO handle continue frame
        let frame: ReadFrame = match self.stream_handler.as_mut() {
            Some(handler) => {
                let mut decompressed = Vec::with_capacity(frame.payload().len() * 2);
                let (header, mut payload) = frame.split();
                payload.extend_from_slice(&[0, 0, 255, 255]);
                handler
                    .de
                    .decompress(&payload, &mut decompressed)
                    .map_err(WsError::DeCompressFailed)?;

                if (self.is_server && handler.config.server_no_context_takeover)
                    || (!self.is_server && handler.config.client_no_context_takeover)
                {
                    handler.de.reset().map_err(WsError::DeCompressFailed)?;
                    tracing::debug!("reset decompressor state");
                }
                let new = ReadFrame::new(
                    true,
                    false,
                    false,
                    false,
                    header.opcode(),
                    header.mask(),
                    &mut decompressed[..],
                );
                new
            }
            None => {
                if frame.header().rsv1() {
                    return Err(WsError::DeCompressFailed(
                        "extension not enabled but got compressed frame".into(),
                    ));
                } else {
                    frame
                }
            }
        };
        Ok(frame)
    }

    /// send a read frame, **this method will not check validation of frame and do not fragment**
    pub fn send_read_frame(&mut self, frame: ReadFrame) -> Result<(), WsError> {
        let frame: Result<ReadFrame, WsError> = self
            .stream_handler
            .as_mut()
            .map(|handler| {
                let header = frame.header();
                let mut compressed = Vec::with_capacity(frame.payload().len());
                handler
                    .com
                    .compress(frame.payload(), &mut compressed)
                    .map_err(WsError::CompressFailed)?;
                compressed.truncate(compressed.len() - 4);
                let new = ReadFrame::new(
                    header.fin(),
                    true,
                    false,
                    false,
                    header.opcode(),
                    header.mask(),
                    &mut compressed[..],
                );
                if (self.is_server && handler.config.server_no_context_takeover)
                    || (!self.is_server && handler.config.client_no_context_takeover)
                {
                    handler.com.reset().map_err(WsError::CompressFailed)?;
                    tracing::debug!("reset compressor");
                }
                Ok(new)
            })
            .unwrap_or(Ok(frame));

        self.frame_codec.send_read_frame(frame?)
    }
}
