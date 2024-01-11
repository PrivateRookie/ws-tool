use http;
use crate::{
    codec::{apply_mask, FrameConfig, Split},
    errors::{ProtocolError, WsError},
    frame::{ctor_header, OpCode, OwnedFrame, SimplifiedHeader},
    protocol::standard_handshake_resp_check,
};
use bytes::BytesMut;
use rand::random;
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};

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
        let frame: Result<OwnedFrame, WsError> = header
            .opcode()
            .is_data()
            .then(|| self.com.as_mut())
            .flatten()
            .map(|handler| {
                let mut compressed = Vec::with_capacity(frame.payload().len());
                handler
                    .com
                    .compress(&[frame.payload()], &mut compressed)
                    .map_err(|code| WsError::CompressFailed(code.to_string()))?;
                compressed.truncate(compressed.len() - 4);
                let mut new = OwnedFrame::new(header.opcode(), prev_mask, &compressed);
                let header = new.header_mut();
                header.set_rsv1(true);
                header.set_fin(header.fin());

                if (self.is_server && handler.config.server_no_context_takeover)
                    || (!self.is_server && handler.config.client_no_context_takeover)
                {
                    handler
                        .com
                        .reset()
                        .map_err(|code| WsError::CompressFailed(code.to_string()))?;
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
            match (self.com.as_mut(), code.is_data()) {
                (Some(handler), true) => {
                    let mut output = vec![];
                    handler
                        .com
                        .compress(&[chunk], &mut output)
                        .map_err(|code| WsError::CompressFailed(code.to_string()))?;
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
                        handler
                            .com
                            .reset()
                            .map_err(|code| WsError::CompressFailed(code.to_string()))?;
                        tracing::trace!("reset compressor");
                    }
                }
                _ => {
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
        }
        Ok(())
    }
}

impl DeflateReadState {
    async fn async_receive_one<S: AsyncRead + Unpin>(
        &mut self,
        stream: &mut S,
    ) -> Result<(SimplifiedHeader, Vec<u8>), WsError> {
        let (mut header, data) = self.read_state.async_receive(stream).await?;
        let data = data.to_vec();
        let compressed = header.rsv1;
        let is_data_frame = header.code.is_data();
        if compressed && !is_data_frame {
            return Err(WsError::ProtocolError {
                close_code: 1002,
                error: ProtocolError::CompressedControlFrame,
            });
        }
        if !is_data_frame || !compressed {
            return Ok((header, data.to_vec()));
        }
        let frame = match self.de.as_mut() {
            Some(handler) => {
                let mut de_data = vec![];
                handler
                    .de
                    .de_compress(&[&data, &[0, 0, 255, 255]], &mut de_data)
                    .map_err(|code| WsError::DeCompressFailed(code.to_string()))?;
                if (self.is_server && handler.config.server_no_context_takeover)
                    || (!self.is_server && handler.config.client_no_context_takeover)
                {
                    handler
                        .de
                        .reset()
                        .map_err(|code| WsError::DeCompressFailed(code.to_string()))?;
                    tracing::trace!("reset decompressor state");
                }
                de_data
            }
            None => {
                if header.rsv1 {
                    return Err(WsError::DeCompressFailed(
                        "extension not enabled but got compressed frame".into(),
                    ));
                } else {
                    data
                }
            }
        };
        header.rsv1 = false;
        Ok((header, frame))
    }

    /// receive a message
    pub async fn async_receive<S: AsyncRead + Unpin>(
        &mut self,
        stream: &mut S,
    ) -> Result<(SimplifiedHeader, &[u8]), WsError> {
        loop {
            let (mut header, mut data) = self.async_receive_one(stream).await?;
            if !self.config.merge_frame {
                self.fragmented_data.clear();
                self.fragmented_data.append(&mut data);
                break Ok((header, &self.fragmented_data));
            }
            match header.code {
                OpCode::Continue => {
                    if !self.fragmented {
                        return Err(WsError::ProtocolError {
                            close_code: 1002,
                            error: ProtocolError::MissInitialFragmentedFrame,
                        });
                    }
                    let fin = header.fin;
                    self.fragmented_data.extend_from_slice(&data);
                    if fin {
                        self.fragmented = false;
                        header.code = self.fragmented_type;
                        break Ok((header, &self.fragmented_data));
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
                    if !header.fin {
                        self.fragmented = true;
                        self.fragmented_type = header.code;
                        if header.code == OpCode::Text
                            && self.config.validate_utf8.is_fast_fail()
                            && simdutf8::basic::from_utf8(&data).is_err()
                        {
                            return Err(WsError::ProtocolError {
                                close_code: 1007,
                                error: ProtocolError::InvalidUtf8,
                            });
                        }
                        self.fragmented_data.clear();
                        self.fragmented_data.extend_from_slice(&data);
                        continue;
                    } else {
                        if header.code == OpCode::Text
                            && self.config.validate_utf8.should_check()
                            && simdutf8::basic::from_utf8(&data).is_err()
                        {
                            return Err(WsError::ProtocolError {
                                close_code: 1007,
                                error: ProtocolError::InvalidUtf8,
                            });
                        }
                        self.fragmented_data.clear();
                        self.fragmented_data.extend_from_slice(&data);
                        break Ok((header, &self.fragmented_data));
                    }
                }
                OpCode::Close | OpCode::Ping | OpCode::Pong => {
                    self.control_buf = data;
                    break Ok((header, &self.control_buf));
                }
                _ => break Err(WsError::UnsupportedFrame(header.code)),
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
    pub async fn receive(&mut self) -> Result<(SimplifiedHeader, &[u8]), WsError> {
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

    /// helper function to send text message
    pub async fn text(&mut self, text: &str) -> Result<(), WsError> {
        self.write_state
            .async_send(&mut self.stream, OpCode::Text, text.as_bytes())
            .await
    }

    /// helper function to send binary message
    pub async fn binary(&mut self, data: &[u8]) -> Result<(), WsError> {
        self.send(OpCode::Binary, data).await
    }

    /// helper function to send ping message
    pub async fn ping(&mut self, data: &[u8]) -> Result<(), WsError> {
        self.send(OpCode::Ping, data).await
    }

    /// helper function to send ping message
    pub async fn pong(&mut self, data: &[u8]) -> Result<(), WsError> {
        self.send(OpCode::Pong, data).await
    }

    /// helper method to send close message
    pub async fn close(&mut self, code: u16, msg: &[u8]) -> Result<(), WsError> {
        let mut data = code.to_be_bytes().to_vec();
        data.extend_from_slice(msg);
        self.send(OpCode::Close, &data).await
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
    pub async fn receive(&mut self) -> Result<(SimplifiedHeader, &[u8]), WsError> {
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

    /// helper function to send text message
    pub async fn text(&mut self, text: &str) -> Result<(), WsError> {
        self.write_state
            .async_send(&mut self.stream, OpCode::Text, text.as_bytes())
            .await
    }

    /// helper function to send binary message
    pub async fn binary(&mut self, data: &[u8]) -> Result<(), WsError> {
        self.send(OpCode::Binary, data).await
    }

    /// helper function to send ping message
    pub async fn ping(&mut self, data: &[u8]) -> Result<(), WsError> {
        self.send(OpCode::Ping, data).await
    }

    /// helper function to send ping message
    pub async fn pong(&mut self, data: &[u8]) -> Result<(), WsError> {
        self.send(OpCode::Pong, data).await
    }

    /// helper method to send close message
    pub async fn close(&mut self, code: u16, msg: &[u8]) -> Result<(), WsError> {
        let mut data = code.to_be_bytes().to_vec();
        data.extend_from_slice(msg);
        self.send(OpCode::Close, &data).await
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
