use bytes::BytesMut;
use rand::random;
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt, BufReader, BufWriter};

use crate::{
    codec::{apply_mask, FrameConfig, Split},
    errors::{ProtocolError, WsError},
    frame::{ctor_header, OpCode, OwnedFrame},
    protocol::standard_handshake_resp_check,
};

use super::{DeflateReadState, DeflateWriteState, PMDConfig};

impl DeflateWriteState {
    /// send a read frame, **this method will not check validation of frame and do not fragment**
    pub async fn async_send_owned_frame<S: AsyncWrite + Unpin>(
        &mut self,
        stream: &mut S,
        mut frame: OwnedFrame,
    ) -> Result<(), WsError> {
        if !frame.header().opcode().is_data() {
            return self
                .write_state
                .async_send_owned_frame(stream, frame)
                .await
                .map_err(WsError::IOError);
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
            .async_send_owned_frame(stream, frame?)
            .await
            .map_err(WsError::IOError)
    }

    /// send payload
    ///
    /// will auto fragment **before compression** if auto_fragment_size > 0
    pub async fn async_send<S: AsyncWrite + Unpin>(
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
            return self.async_send_owned_frame(stream, frame).await;
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
                stream.write_all(header).await?;
                if let Some(mask) = mask {
                    apply_mask(&mut output, mask)
                };
                stream.write_all(&output).await?;
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
                stream.write_all(header).await?;
                if let Some(mask) = mask {
                    let mut data = BytesMut::from_iter(chunk);
                    apply_mask(&mut data, mask);
                    stream.write_all(&data).await?;
                } else {
                    stream.write_all(chunk).await?;
                }
            }
        }
        Ok(())
    }
}

impl DeflateReadState {
    async fn async_receive_one<S: AsyncRead + Unpin>(
        &mut self,
        stream: &mut S,
    ) -> Result<OwnedFrame, WsError> {
        let (header, data) = self.read_state.async_receive(stream).await?;
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
    pub async fn async_receive<S: AsyncRead + Unpin>(
        &mut self,
        stream: &mut S,
    ) -> Result<OwnedFrame, WsError> {
        loop {
            let unmasked_frame = self.async_receive_one(stream).await?;
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
pub struct AsyncDeflateCodec<S: AsyncRead + AsyncWrite> {
    read_state: DeflateReadState,
    write_state: DeflateWriteState,
    stream: S,
}

impl<S: AsyncRead + AsyncWrite + Unpin> AsyncDeflateCodec<S> {
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
        let mut pmd_configs: Vec<PMDConfig> = vec![];
        for (k, v) in req.headers() {
            if k.as_str().to_lowercase() == "sec-websocket-extensions" {
                if let Ok(s) = v.to_str() {
                    match PMDConfig::parse_str(s) {
                        Ok(mut conf) => {
                            pmd_configs.append(&mut conf);
                        }
                        Err(e) => return Err(WsError::HandShakeFailed(e)),
                    }
                }
            }
        }
        let mut pmd_config = pmd_configs.pop();
        if let Some(conf) = pmd_config.as_mut() {
            let min = conf.client_max_window_bits.min(conf.server_max_window_bits);
            conf.client_max_window_bits = min;
            conf.server_max_window_bits = min;
        }
        tracing::debug!("use deflate config {:?}", pmd_config);
        let frame_conf = FrameConfig {
            mask_send_frame: false,
            ..Default::default()
        };
        let codec = AsyncDeflateCodec::new(stream, frame_conf, pmd_config, true);
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
        let codec = AsyncDeflateCodec::new(stream, Default::default(), pmd_conf, false);
        Ok(codec)
    }

    /// get mutable underlying stream
    pub fn stream_mut(&mut self) -> &mut S {
        &mut self.stream
    }

    /// receive a message
    pub async fn receive(&mut self) -> Result<OwnedFrame, WsError> {
        self.read_state.async_receive(&mut self.stream).await
    }

    /// send a read frame, **this method will not check validation of frame and do not fragment**
    pub async fn send_owned_frame(&mut self, frame: OwnedFrame) -> Result<(), WsError> {
        self.write_state
            .async_send_owned_frame(&mut self.stream, frame)
            .await
    }

    /// send payload
    ///
    /// will auto fragment **before compression** if auto_fragment_size > 0
    pub async fn send(&mut self, code: OpCode, payload: &[u8]) -> Result<(), WsError> {
        self.write_state
            .async_send(&mut self.stream, code, payload)
            .await
    }

    /// flush stream to ensure all data are send
    pub async fn flush(&mut self) -> Result<(), WsError> {
        self.stream.flush().await.map_err(WsError::IOError)
    }
}

/// recv part of async deflate message
pub struct AsyncDeflateRecv<S: AsyncRead> {
    stream: S,
    read_state: DeflateReadState,
}

impl<S: AsyncRead + Unpin> AsyncDeflateRecv<S> {
    /// construct method
    pub fn new(stream: S, read_state: DeflateReadState) -> Self {
        Self { stream, read_state }
    }

    /// get mutable underlying stream
    pub fn stream_mut(&mut self) -> &mut S {
        &mut self.stream
    }

    /// receive a frame
    pub async fn receive(&mut self) -> Result<OwnedFrame, WsError> {
        self.read_state.async_receive(&mut self.stream).await
    }
}

/// send part of deflate message
pub struct AsyncDeflateSend<S: AsyncWrite> {
    stream: S,
    write_state: DeflateWriteState,
}

impl<S: AsyncWrite + Unpin> AsyncDeflateSend<S> {
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
    pub async fn send_owned_frame(&mut self, frame: OwnedFrame) -> Result<(), WsError> {
        self.write_state
            .async_send_owned_frame(&mut self.stream, frame)
            .await
    }

    /// send payload
    ///
    /// will auto fragment **before compression** if auto_fragment_size > 0
    pub async fn send(&mut self, code: OpCode, payload: &[u8]) -> Result<(), WsError> {
        self.write_state
            .async_send(&mut self.stream, code, payload)
            .await
    }

    /// flush stream to ensure all data are send
    pub async fn flush(&mut self) -> Result<(), WsError> {
        self.stream.flush().await.map_err(WsError::IOError)
    }
}

impl<R, W, S> AsyncDeflateCodec<S>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
    S: AsyncRead + AsyncWrite + Unpin + Split<R = R, W = W>,
{
    /// split codec to recv and send parts
    pub fn split(self) -> (AsyncDeflateRecv<R>, AsyncDeflateSend<W>) {
        let AsyncDeflateCodec {
            stream,
            read_state,
            write_state,
        } = self;
        let (read, write) = stream.split();
        (
            AsyncDeflateRecv::new(read, read_state),
            AsyncDeflateSend::new(write, write_state),
        )
    }
}

// TODO use constructed cap
impl<R, W, S> Split for BufReader<S>
where
    R: AsyncRead,
    W: AsyncWrite,
    S: AsyncRead + AsyncWrite + Split<R = R, W = W>,
{
    type R = BufReader<R>;

    type W = BufWriter<W>;

    fn split(self) -> (Self::R, Self::W) {
        let s = self.into_inner();
        let (r, w) = s.split();
        (BufReader::new(r), BufWriter::new(w))
    }
}

// TODO use constructed cap
impl<R, W, S> Split for BufWriter<S>
where
    R: AsyncRead,
    W: AsyncWrite,
    S: AsyncRead + AsyncWrite + Split<R = R, W = W>,
{
    type R = BufReader<R>;

    type W = BufWriter<W>;

    fn split(self) -> (Self::R, Self::W) {
        let s = self.into_inner();
        let (r, w) = s.split();
        (BufReader::new(r), BufWriter::new(w))
    }
}
