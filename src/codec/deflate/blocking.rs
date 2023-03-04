use std::io::{Read, Write};

use bytes::BytesMut;
use rand::random;

use crate::{
    codec::{apply_mask_fast32, FrameConfig, Split},
    errors::{ProtocolError, WsError},
    frame::{ctor_header, OpCode, OwnedFrame},
    protocol::standard_handshake_resp_check,
};

use super::{DeflateReadState, DeflateWriteState, PMDConfig};

impl DeflateWriteState {
    /// send a read frame, **this method will not check validation of frame and do not fragment**
    pub fn send_owned_frame<S: Write>(
        &mut self,
        stream: &mut S,
        mut frame: OwnedFrame,
    ) -> Result<(), WsError> {
        if !frame.header().opcode().is_data() {
            return self
                .write_state
                .send_owned_frame(stream, frame)
                .map_err(|e| WsError::IOError(Box::new(e)));
        }
        let prev_mask = frame.unmask();
        let header = frame.header();
        let frame: Result<OwnedFrame, WsError> = self
            .com
            .as_mut()
            .map(|handler| {
                let mut compressed = Vec::with_capacity(frame.payload().len());
                handler
                    .com
                    .compress(frame.payload(), &mut compressed)
                    .map_err(WsError::CompressFailed)?;
                compressed.truncate(compressed.len() - 4);
                let mut new = OwnedFrame::new(header.opcode(), prev_mask, &compressed);
                let header = new.header_mut();
                header.set_rsv1(true);
                header.set_fin(header.fin());

                if (self.is_server && handler.config.server_no_context_takeover)
                    || (!self.is_server && handler.config.client_no_context_takeover)
                {
                    handler.com.reset().map_err(WsError::CompressFailed)?;
                    tracing::trace!("reset compressor");
                }
                Ok(new)
            })
            .unwrap_or_else(|| {
                if let Some(mask) = prev_mask {
                    frame.mask(mask);
                }
                Ok(frame)
            });
        self.write_state
            .send_owned_frame(stream, frame?)
            .map_err(|e| WsError::IOError(Box::new(e)))
    }

    /// send payload
    ///
    /// will auto fragment **before compression** if auto_fragment_size > 0
    pub fn send<S: Write>(
        &mut self,
        stream: &mut S,
        code: OpCode,
        payload: &[u8],
    ) -> Result<(), WsError> {
        let mask_send = self.config.mask_send_frame;
        let mask_fn = || {
            if mask_send {
                Some(random())
            } else {
                None
            }
        };
        if payload.is_empty() {
            let mask = mask_fn();
            let frame = OwnedFrame::new(code, mask, &[]);
            return self.send_owned_frame(stream, frame);
        }

        let chunk_size = if self.config.auto_fragment_size > 0 {
            self.config.auto_fragment_size
        } else {
            payload.len()
        };
        let parts: Vec<&[u8]> = payload.chunks(chunk_size).collect();
        let total = parts.len();
        for (idx, chunk) in parts.into_iter().enumerate() {
            let fin = idx + 1 == total;
            let mask = mask_fn();

            if let Some(handler) = self.com.as_mut() {
                let mut output = Vec::with_capacity(chunk.len());
                handler
                    .com
                    .compress(chunk, &mut output)
                    .map_err(WsError::CompressFailed)?;
                output.truncate(output.len() - 4);
                let header = ctor_header(
                    &mut self.header_buf,
                    fin,
                    true,
                    false,
                    false,
                    mask,
                    code,
                    output.len() as u64,
                );
                stream.write_all(header)?;
                if let Some(mask) = mask {
                    apply_mask_fast32(&mut output, mask)
                };
                stream.write_all(&output)?;
                if (self.is_server && handler.config.server_no_context_takeover)
                    || (!self.is_server && handler.config.client_no_context_takeover)
                {
                    handler.com.reset().map_err(WsError::CompressFailed)?;
                    tracing::trace!("reset compressor");
                }
            } else {
                let header = ctor_header(
                    &mut self.header_buf,
                    fin,
                    false,
                    false,
                    false,
                    mask,
                    code,
                    chunk.len() as u64,
                );
                stream.write_all(header)?;
                if let Some(mask) = mask {
                    let mut data = BytesMut::from_iter(chunk);
                    apply_mask_fast32(&mut data, mask);
                    stream.write_all(&data)?;
                } else {
                    stream.write_all(chunk)?;
                }
            }
        }
        Ok(())
    }
}

impl DeflateReadState {
    fn receive_one<S: Read>(&mut self, stream: &mut S) -> Result<OwnedFrame, WsError> {
        let frame = self.read_state.receive(stream)?;
        // let frame = self.frame_codec.receive()?;
        let compressed = frame.header().rsv1();
        let is_data_frame = frame.header().opcode().is_data();
        if compressed && !is_data_frame {
            return Err(WsError::ProtocolError {
                close_code: 1002,
                error: ProtocolError::CompressedControlFrame,
            });
        }
        if !is_data_frame {
            return Ok(frame);
        }
        let frame: OwnedFrame = match self.de.as_mut() {
            Some(handler) => {
                let mut decompressed = Vec::with_capacity(frame.payload().len() * 2);
                let (header, mut payload) = frame.parts();
                payload.extend_from_slice(&[0, 0, 255, 255]);
                handler
                    .de
                    .decompress(&payload, &mut decompressed)
                    .map_err(WsError::DeCompressFailed)?;
                if (self.is_server && handler.config.server_no_context_takeover)
                    || (!self.is_server && handler.config.client_no_context_takeover)
                {
                    handler.de.reset().map_err(WsError::DeCompressFailed)?;
                    tracing::trace!("reset decompressor state");
                }
                OwnedFrame::new(header.opcode(), None, &decompressed[..])
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

    /// receive a message
    pub fn receive<S: Read>(&mut self, stream: &mut S) -> Result<OwnedFrame, WsError> {
        loop {
            let unmasked_frame = self.receive_one(stream)?;
            if !self.config.merge_frame {
                break Ok(unmasked_frame);
            }
            let header = unmasked_frame.header();
            let opcode = header.opcode();
            match opcode {
                OpCode::Continue => {
                    if !self.fragmented {
                        return Err(WsError::ProtocolError {
                            close_code: 1002,
                            error: ProtocolError::MissInitialFragmentedFrame,
                        });
                    }
                    let fin = header.fin();
                    self.fragmented_data
                        .extend_from_slice(unmasked_frame.payload());
                    if fin {
                        let completed_frame = std::mem::replace(
                            &mut self.fragmented_data,
                            OwnedFrame::new(OpCode::Binary, None, &[]),
                        );
                        self.fragmented = false;
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
                    if !header.fin() {
                        self.fragmented = true;
                        self.fragmented_type = opcode;
                        if opcode == OpCode::Text
                            && self.config.validate_utf8.is_fast_fail()
                            && simdutf8::basic::from_utf8(unmasked_frame.payload()).is_err()
                        {
                            return Err(WsError::ProtocolError {
                                close_code: 1007,
                                error: ProtocolError::InvalidUtf8,
                            });
                        }
                        self.fragmented_data.header_mut().set_opcode(opcode);
                        self.fragmented_data
                            .extend_from_slice(unmasked_frame.payload());
                        continue;
                    } else {
                        if opcode == OpCode::Text
                            && self.config.validate_utf8.should_check()
                            && simdutf8::basic::from_utf8(unmasked_frame.payload()).is_err()
                        {
                            return Err(WsError::ProtocolError {
                                close_code: 1007,
                                error: ProtocolError::InvalidUtf8,
                            });
                        }
                        break Ok(unmasked_frame);
                    }
                }
                OpCode::Close | OpCode::Ping | OpCode::Pong => break Ok(unmasked_frame),
                _ => break Err(WsError::UnsupportedFrame(opcode)),
            }
        }
    }
}

/// recv/send deflate message
pub struct DeflateCodec<S: Read + Write> {
    read_state: DeflateReadState,
    write_state: DeflateWriteState,
    stream: S,
}

impl<S: Read + Write> DeflateCodec<S> {
    /// construct method
    pub fn new(
        stream: S,
        frame_config: FrameConfig,
        pmd_config: Option<PMDConfig>,
        is_server: bool,
    ) -> Self {
        let read_state =
            DeflateReadState::with_config(frame_config.clone(), pmd_config.clone(), is_server);
        let write_state = DeflateWriteState::with_config(frame_config, pmd_config, is_server);
        Self {
            read_state,
            write_state,
            stream,
        }
    }

    /// used for server side to construct a new server
    pub fn factory(req: http::Request<()>, stream: S) -> Result<Self, WsError> {
        let mut pmd_confs: Vec<PMDConfig> = vec![];
        for (k, v) in req.headers() {
            if k.as_str().to_lowercase() == "sec-websocket-extensions" {
                if let Ok(s) = v.to_str() {
                    match PMDConfig::parse_str(s) {
                        Ok(mut conf) => {
                            pmd_confs.append(&mut conf);
                        }
                        Err(e) => return Err(WsError::HandShakeFailed(e)),
                    }
                }
            }
        }
        let mut pmd_conf = pmd_confs.pop();
        if let Some(conf) = pmd_conf.as_mut() {
            let min = conf.client_max_window_bits.min(conf.server_max_window_bits);
            conf.client_max_window_bits = min;
            conf.server_max_window_bits = min;
        }
        tracing::debug!("use deflate config {:?}", pmd_conf);

        let frame_conf = FrameConfig {
            mask_send_frame: false,
            ..Default::default()
        };
        let codec = DeflateCodec::new(stream, frame_conf, pmd_conf, true);
        Ok(codec)
    }

    /// used for client side to construct a new client
    pub fn check_fn(key: String, resp: http::Response<()>, stream: S) -> Result<Self, WsError> {
        standard_handshake_resp_check(key.as_bytes(), &resp)?;
        let mut pmd_confs: Vec<PMDConfig> = vec![];
        for (k, v) in resp.headers() {
            if k.as_str().to_lowercase() == "sec-websocket-extensions" {
                if let Ok(s) = v.to_str() {
                    match PMDConfig::parse_str(s) {
                        Ok(mut conf) => {
                            pmd_confs.append(&mut conf);
                        }
                        Err(e) => return Err(WsError::HandShakeFailed(e)),
                    }
                }
            }
        }
        let mut pmd_conf = pmd_confs.pop();
        if let Some(conf) = pmd_conf.as_mut() {
            let min = conf.client_max_window_bits.min(conf.server_max_window_bits);
            conf.client_max_window_bits = min;
            conf.server_max_window_bits = min;
        }
        tracing::debug!("use deflate config: {:?}", pmd_conf);
        let codec = DeflateCodec::new(stream, Default::default(), pmd_conf, false);
        Ok(codec)
    }

    /// get mutable underlying stream
    pub fn stream_mut(&mut self) -> &mut S {
        &mut self.stream
    }

    /// receive a message
    pub fn receive(&mut self) -> Result<OwnedFrame, WsError> {
        self.read_state.receive(&mut self.stream)
    }

    /// send a read frame, **this method will not check validation of frame and do not fragment**
    pub fn send_owned_frame(&mut self, frame: OwnedFrame) -> Result<(), WsError> {
        self.write_state.send_owned_frame(&mut self.stream, frame)
    }

    /// send payload
    ///
    /// will auto fragment **before compression** if auto_fragment_size > 0
    pub fn send(&mut self, code: OpCode, payload: &[u8]) -> Result<(), WsError> {
        self.write_state.send(&mut self.stream, code, payload)
    }

    /// flush stream to ensure all data are send
    pub fn flush(&mut self) -> Result<(), WsError> {
        self.stream
            .flush()
            .map_err(|e| WsError::IOError(Box::new(e)))
    }
}

/// recv part of deflate message
pub struct DeflateRecv<S: Read> {
    stream: S,
    read_state: DeflateReadState,
}

impl<S: Read> DeflateRecv<S> {
    /// construct method
    pub fn new(stream: S, read_state: DeflateReadState) -> Self {
        Self { stream, read_state }
    }

    /// get mutable underlying stream
    pub fn stream_mut(&mut self) -> &mut S {
        &mut self.stream
    }

    /// receive a frame
    pub fn receive(&mut self) -> Result<OwnedFrame, WsError> {
        self.read_state.receive(&mut self.stream)
    }
}

/// send part of deflate message
pub struct DeflateSend<S: Write> {
    stream: S,
    write_state: DeflateWriteState,
}

impl<S: Write> DeflateSend<S> {
    /// construct method
    pub fn new(stream: S, write_state: DeflateWriteState) -> Self {
        Self {
            stream,
            write_state,
        }
    }

    /// get mutable underlying stream
    pub fn stream_mut(&mut self) -> &mut S {
        &mut self.stream
    }

    /// send a read frame, **this method will not check validation of frame and do not fragment**
    pub fn send_owned_frame(&mut self, frame: OwnedFrame) -> Result<(), WsError> {
        self.write_state.send_owned_frame(&mut self.stream, frame)
    }

    /// send payload
    ///
    /// will auto fragment **before compression** if auto_fragment_size > 0
    pub fn send(&mut self, code: OpCode, payload: &[u8]) -> Result<(), WsError> {
        self.write_state.send(&mut self.stream, code, payload)
    }

    /// flush stream to ensure all data are send
    pub fn flush(&mut self) -> Result<(), WsError> {
        self.stream
            .flush()
            .map_err(|e| WsError::IOError(Box::new(e)))
    }
}

impl<R, W, S> DeflateCodec<S>
where
    R: Read,
    W: Write,
    S: Read + Write + Split<R = R, W = W>,
{
    /// split codec to recv and send parts
    pub fn split(self) -> (DeflateRecv<R>, DeflateSend<W>) {
        let DeflateCodec {
            stream,
            read_state,
            write_state,
        } = self;
        let (read, write) = stream.split();
        (
            DeflateRecv::new(read, read_state),
            DeflateSend::new(write, write_state),
        )
    }
}
