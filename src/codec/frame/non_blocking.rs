use bytes::BytesMut;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use super::{FrameConfig, FrameReadState, FrameWriteState};
use crate::{
    codec::Split,
    errors::WsError,
    frame::{Header, OpCode, OwnedFrame, Payload, PayloadMut},
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
            if let Some(frame) = self.check_frame(frame)? {
                break Ok(frame);
            }
        }
    }
}

impl FrameWriteState {
    #[allow(clippy::too_many_arguments)]
    async fn async_send_one_mut<'a, S: AsyncWrite + Unpin, M: Into<Option<[u8; 4]>>>(
        &mut self,
        stream: &mut S,
        fin: bool,
        rsv1: bool,
        rsv2: bool,
        rsv3: bool,
        mask_key: M,
        opcode: OpCode,
        mut payload: PayloadMut<'a>,
        mask_payload: bool,
    ) -> IOResult<()> {
        let header = Header::new(
            fin,
            rsv1,
            rsv2,
            rsv3,
            mask_key,
            opcode,
            payload.len() as u64,
        );
        if mask_payload {
            if let Some(key) = header.masking_key() {
                payload.apply_mask(key)
            }
        }
        stream.write_all(&header.0).await?;
        for part in payload.0 {
            stream.write_all(part).await?;
        }
        Ok(())
    }

    /// send mutable payload
    ///
    /// if mask_payload is set, mask payload first
    ///
    /// will auto fragment if auto_fragment_size > 0
    pub async fn async_send_mut<'a, S: AsyncWrite + Unpin>(
        &mut self,
        stream: &mut S,
        opcode: OpCode,
        payload: PayloadMut<'a>,
        mask_payload: bool,
    ) -> IOResult<()> {
        let split_size = self.config.auto_fragment_size;
        let mask_send = self.config.mask_send_frame;
        let mask_fn = || {
            if mask_send {
                Some(rand::random())
            } else {
                None
            }
        };
        if split_size > 0 {
            let parts = payload.split_with(split_size);
            let total = parts.len();
            for (idx, part) in parts.into_iter().enumerate() {
                let fin = idx + 1 == total;
                let key = mask_fn();
                self.async_send_one_mut(
                    stream,
                    fin,
                    false,
                    false,
                    false,
                    key,
                    opcode.clone(),
                    part,
                    mask_payload,
                )
                .await?;
            }
        } else {
            let key = mask_fn();
            self.async_send_one_mut(
                stream,
                true,
                false,
                false,
                false,
                key,
                opcode,
                payload,
                mask_payload,
            )
            .await?;
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    async fn async_send_one<'a, S: AsyncWrite + Unpin, M: Into<Option<[u8; 4]>>>(
        &mut self,
        stream: &mut S,
        fin: bool,
        rsv1: bool,
        rsv2: bool,
        rsv3: bool,
        mask_key: M,
        opcode: OpCode,
        payload: Payload<'a>,
    ) -> IOResult<()> {
        let header = Header::new(
            fin,
            rsv1,
            rsv2,
            rsv3,
            mask_key,
            opcode,
            payload.len() as u64,
        );
        stream.write_all(&header.0).await?;
        for part in payload.0 {
            stream.write_all(part).await?;
        }
        Ok(())
    }

    /// send immutable payload
    ///
    /// will auto fragment if auto_fragment_size > 0
    pub async fn async_send<'a, S: AsyncWrite + Unpin>(
        &mut self,
        stream: &mut S,
        opcode: OpCode,
        payload: Payload<'a>,
    ) -> IOResult<()> {
        let split_size = self.config.auto_fragment_size;
        let mask_send = self.config.mask_send_frame;
        let mask_fn = || {
            if mask_send {
                Some(rand::random())
            } else {
                None
            }
        };
        if split_size > 0 {
            let parts = payload.split_with(split_size);
            let total = parts.len();
            for (idx, part) in parts.into_iter().enumerate() {
                let fin = idx + 1 == total;
                self.async_send_one(
                    stream,
                    fin,
                    false,
                    false,
                    false,
                    mask_fn(),
                    opcode.clone(),
                    part,
                )
                .await?;
            }
        } else {
            self.async_send_one(
                stream,
                true,
                false,
                false,
                false,
                mask_fn(),
                opcode,
                payload,
            )
            .await?;
        }
        Ok(())
    }

    async fn async_send_read_frame<S: AsyncWrite + Unpin>(
        &mut self,
        stream: &mut S,
        frame: OwnedFrame,
    ) -> IOResult<()> {
        stream.write_all(frame.payload()).await
    }
}

macro_rules! impl_recv {
    () => {
        /// receive a frame
        pub async fn receive(&mut self) -> Result<OwnedFrame, WsError> {
            self.read_state.async_receive(&mut self.stream).await
        }
    };
}

macro_rules! impl_send {
    () => {
        /// send mutable payload
        ///
        /// if mask_payload is set, mask payload first
        ///
        /// will auto fragment if auto_fragment_size > 0
        pub async fn send_mut<'a, P: Into<PayloadMut<'a>>>(
            &mut self,
            opcode: OpCode,
            payload: P,
            mask_payload: bool,
        ) -> Result<(), WsError> {
            self.write_state
                .async_send_mut(&mut self.stream, opcode, payload.into(), mask_payload)
                .await
                .map_err(|e| WsError::IOError(Box::new(e)))
        }

        /// send immutable payload
        ///
        /// will auto fragment if auto_fragment_size > 0
        pub async fn send<'a, P: Into<Payload<'a>>>(
            &mut self,
            opcode: OpCode,
            payload: P,
        ) -> Result<(), WsError> {
            self.write_state
                .async_send(&mut self.stream, opcode, payload.into())
                .await
                .map_err(|e| WsError::IOError(Box::new(e)))
        }

        /// send a read frame, **this method will not check validation of frame and do not fragment**
        pub async fn send_read_frame(&mut self, frame: OwnedFrame) -> Result<(), WsError> {
            self.write_state
                .async_send_read_frame(&mut self.stream, frame)
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
    };
}

/// recv part of websocket stream
pub struct AsyncWsFrameRecv<S: AsyncRead> {
    stream: S,
    read_state: FrameReadState,
}

impl<S: AsyncRead + Unpin> AsyncWsFrameRecv<S> {
    /// construct method
    pub fn new(stream: S, read_state: FrameReadState) -> Self {
        Self { stream, read_state }
    }

    impl_recv! {}
}

/// send part of websocket frame
pub struct AsyncWsFrameSend<S: AsyncWrite> {
    stream: S,
    write_state: FrameWriteState,
}

impl<S: AsyncWrite + Unpin> AsyncWsFrameSend<S> {
    /// construct method
    pub fn new(stream: S, write_state: FrameWriteState) -> Self {
        Self {
            stream,
            write_state,
        }
    }

    impl_send! {}
}

/// recv/send websocket frame
pub struct AsyncWsFrameCodec<S: AsyncRead + AsyncWrite> {
    /// underlying transport stream
    pub stream: S,
    /// read state
    pub read_state: FrameReadState,
    /// write state
    pub write_state: FrameWriteState,
}

impl<S: AsyncRead + AsyncWrite + Unpin> AsyncWsFrameCodec<S> {
    /// construct method
    pub fn new(stream: S) -> Self {
        Self {
            stream,
            read_state: FrameReadState::default(),
            write_state: FrameWriteState::default(),
        }
    }

    /// construct with stream and config
    pub fn new_with(stream: S, config: FrameConfig, read_bytes: BytesMut) -> Self {
        Self {
            stream,
            read_state: FrameReadState::with_remain(config.clone(), read_bytes),
            write_state: FrameWriteState::with_config(config),
        }
    }

    /// get mutable underlying stream
    pub fn stream_mut(&mut self) -> &mut S {
        &mut self.stream
    }

    /// used for server side to construct a new server
    pub fn factory(_req: http::Request<()>, remain: BytesMut, stream: S) -> Result<Self, WsError> {
        let config = FrameConfig {
            mask_send_frame: false,
            ..Default::default()
        };
        Ok(Self::new_with(stream, config, remain))
    }

    /// used to client side to construct a new client
    pub fn check_fn(
        key: String,
        resp: http::Response<()>,
        remain: BytesMut,
        stream: S,
    ) -> Result<Self, WsError> {
        standard_handshake_resp_check(key.as_bytes(), &resp)?;
        Ok(Self::new_with(stream, FrameConfig::default(), remain))
    }

    impl_recv! {}

    impl_send! {}
}

impl<R, W, S> AsyncWsFrameCodec<S>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
    S: AsyncRead + AsyncWrite + Unpin + Split<R = R, W = W>,
{
    /// split codec to recv and send parts
    pub fn split(self) -> (AsyncWsFrameRecv<R>, AsyncWsFrameSend<W>) {
        let AsyncWsFrameCodec {
            stream,
            read_state,
            write_state,
        } = self;
        let (read, write) = stream.split();
        (
            AsyncWsFrameRecv::new(read, read_state),
            AsyncWsFrameSend::new(write, write_state),
        )
    }
}
