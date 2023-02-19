use bytes::BytesMut;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use super::{apply_mask_fast32, FrameConfig, FrameReadState, FrameWriteState};
use crate::{
    codec::Split,
    errors::WsError,
    frame::{Header, OpCode, OwnedFrame},
    protocol::standard_handshake_resp_check,
};

type IOResult<T> = std::io::Result<T>;

impl FrameReadState {
    async fn async_poll<S: AsyncRead + Unpin>(&mut self, stream: &mut S) -> IOResult<usize> {
        self.read_data.resize(self.read_idx + 1024, 0);
        let count = stream.read(&mut self.read_data[self.read_idx..]).await?;
        self.read_idx += count;
        self.read_data.resize(self.read_idx, 0);
        if count == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::ConnectionAborted,
                "read eof",
            ));
        }
        Ok(count)
    }

    async fn async_poll_one_frame<S: AsyncRead + Unpin>(
        &mut self,
        stream: &mut S,
        size: usize,
    ) -> IOResult<usize> {
        let buf_len = self.read_data.len();
        if buf_len < size {
            self.read_data.resize(size, 0);
            stream
                .read_exact(&mut self.read_data[buf_len..size])
                .await?;
            Ok(size - buf_len)
        } else {
            Ok(0)
        }
    }

    async fn async_read_one_frame<S: AsyncRead + Unpin>(
        &mut self,
        stream: &mut S,
    ) -> Result<OwnedFrame, WsError> {
        while !self.is_header_ok() {
            self.async_poll(stream).await?;
        }
        let len = self.parse_frame_header()?;
        self.async_poll_one_frame(stream, len).await?;
        Ok(self.consume_frame(len))
    }

    /// **NOTE** masked frame has already been unmasked
    pub async fn async_receive<S: AsyncRead + Unpin>(
        &mut self,
        stream: &mut S,
    ) -> Result<OwnedFrame, WsError> {
        loop {
            let frame = self.async_read_one_frame(stream).await?;
            if self.config.merge_frame {
                if let Some(frame) = self
                    .check_frame(frame)
                    .and_then(|frame| self.merge_frame(frame))?
                {
                    break Ok(frame);
                }
            } else {
                break self.check_frame(frame);
            }
        }
    }
}

impl FrameWriteState {
    /// send immutable payload
    ///
    /// if need to mask, copy data to inner buffer and then apply mask
    ///
    /// will auto fragment if auto_fragment_size > 0
    pub async fn async_send<S: AsyncWrite + Unpin>(
        &mut self,
        stream: &mut S,
        opcode: OpCode,
        payload: &[u8],
    ) -> IOResult<()> {
        let mask_send = self.config.mask_send_frame;
        let mask_fn = || {
            if mask_send {
                Some(rand::random())
            } else {
                None
            }
        };

        if payload.is_empty() {
            let mask = mask_fn();
            let header = Header::new(true, false, false, false, mask, opcode, 0);
            stream.write_all(&header.0).await?;
            return Ok(());
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
            let header = Header::new(
                fin,
                false,
                false,
                false,
                mask,
                opcode.clone(),
                chunk.len() as u64,
            );
            stream.write_all(&header.0).await?;
            if let Some(mask) = mask {
                let mut data = BytesMut::from_iter(chunk);
                apply_mask_fast32(&mut data, mask);
                stream.write_all(&data).await?;
            } else {
                stream.write_all(chunk).await?;
            }
        }
        Ok(())
    }

    pub(crate) async fn async_send_owned_frame<S: AsyncWrite + Unpin>(
        &mut self,
        stream: &mut S,
        frame: OwnedFrame,
    ) -> IOResult<()> {
        stream.write_all(&frame.header().0).await?;
        stream.write_all(frame.payload()).await
    }
}

/// recv part of websocket stream
pub struct AsyncFrameRecv<S: AsyncRead> {
    stream: S,
    read_state: FrameReadState,
}

impl<S: AsyncRead + Unpin> AsyncFrameRecv<S> {
    /// construct method
    pub fn new(stream: S, read_state: FrameReadState) -> Self {
        Self { stream, read_state }
    }

    /// receive a frame
    pub async fn receive(&mut self) -> Result<OwnedFrame, WsError> {
        self.read_state.async_receive(&mut self.stream).await
    }
}

/// send part of websocket frame
pub struct AsyncFrameSend<S: AsyncWrite> {
    stream: S,
    write_state: FrameWriteState,
}

impl<S: AsyncWrite + Unpin> AsyncFrameSend<S> {
    /// construct method
    pub fn new(stream: S, write_state: FrameWriteState) -> Self {
        Self {
            stream,
            write_state,
        }
    }

    /// send immutable payload
    ///
    /// will auto fragment if auto_fragment_size > 0
    pub async fn send(&mut self, opcode: OpCode, payload: &[u8]) -> Result<(), WsError> {
        self.write_state
            .async_send(&mut self.stream, opcode, payload)
            .await
            .map_err(|e| WsError::IOError(Box::new(e)))
    }

    /// send a read frame, **this method will not check validation of frame and do not fragment**
    pub async fn send_owned_frame(&mut self, frame: OwnedFrame) -> Result<(), WsError> {
        self.write_state
            .async_send_owned_frame(&mut self.stream, frame)
            .await
            .map_err(|e| WsError::IOError(Box::new(e)))
    }

    /// flush to ensure all data are send
    pub async fn flush(&mut self) -> Result<(), WsError> {
        self.stream
            .flush()
            .await
            .map_err(|e| WsError::IOError(Box::new(e)))
    }
}

/// recv/send websocket frame
pub struct AsyncFrameCodec<S: AsyncRead + AsyncWrite> {
    /// underlying transport stream
    pub stream: S,
    /// read state
    pub read_state: FrameReadState,
    /// write state
    pub write_state: FrameWriteState,
}

impl<S: AsyncRead + AsyncWrite + Unpin> AsyncFrameCodec<S> {
    /// construct method
    pub fn new(stream: S) -> Self {
        Self {
            stream,
            read_state: FrameReadState::default(),
            write_state: FrameWriteState::default(),
        }
    }

    /// construct with stream and config
    pub fn new_with(stream: S, config: FrameConfig) -> Self {
        Self {
            stream,
            read_state: FrameReadState::with_config(config.clone()),
            write_state: FrameWriteState::with_config(config),
        }
    }

    /// get mutable underlying stream
    pub fn stream_mut(&mut self) -> &mut S {
        &mut self.stream
    }

    /// used for server side to construct a new server
    pub fn factory(_req: http::Request<()>, stream: S) -> Result<Self, WsError> {
        let config = FrameConfig {
            mask_send_frame: false,
            ..Default::default()
        };
        Ok(Self::new_with(stream, config))
    }

    /// used to client side to construct a new client
    pub fn check_fn(key: String, resp: http::Response<()>, stream: S) -> Result<Self, WsError> {
        standard_handshake_resp_check(key.as_bytes(), &resp)?;
        Ok(Self::new_with(stream, FrameConfig::default()))
    }

    /// receive a frame
    pub async fn receive(&mut self) -> Result<OwnedFrame, WsError> {
        self.read_state.async_receive(&mut self.stream).await
    }

    /// send payload
    ///
    /// will auto fragment if auto_fragment_size > 0
    pub async fn send(&mut self, opcode: OpCode, payload: &[u8]) -> Result<(), WsError> {
        self.write_state
            .async_send(&mut self.stream, opcode, payload)
            .await
            .map_err(|e| WsError::IOError(Box::new(e)))
    }

    /// send a read frame, **this method will not check validation of frame and do not fragment**
    pub async fn send_owned_frame(&mut self, frame: OwnedFrame) -> Result<(), WsError> {
        self.write_state
            .async_send_owned_frame(&mut self.stream, frame)
            .await
            .map_err(|e| WsError::IOError(Box::new(e)))
    }

    /// flush to ensure all data are send
    pub async fn flush(&mut self) -> Result<(), WsError> {
        self.stream
            .flush()
            .await
            .map_err(|e| WsError::IOError(Box::new(e)))
    }
}

impl<R, W, S> AsyncFrameCodec<S>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
    S: AsyncRead + AsyncWrite + Unpin + Split<R = R, W = W>,
{
    /// split codec to recv and send parts
    pub fn split(self) -> (AsyncFrameRecv<R>, AsyncFrameSend<W>) {
        let AsyncFrameCodec {
            stream,
            read_state,
            write_state,
        } = self;
        let (read, write) = stream.split();
        (
            AsyncFrameRecv::new(read, read_state),
            AsyncFrameSend::new(write, write_state),
        )
    }
}
