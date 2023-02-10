use bytes::{Buf, BytesMut};
use tokio::io::{AsyncRead, AsyncWrite};

use crate::{
    codec::{
        AsyncFrameCodec, AsyncFrameRecv, AsyncFrameSend, FrameConfig, FrameReadState,
        FrameWriteState, Split,
    },
    errors::WsError,
    frame::OpCode,
    protocol::standard_handshake_resp_check,
    Message,
};

macro_rules! impl_recv {
    () => {
        /// receive a message
        pub async fn receive(&mut self) -> Result<Message<BytesMut>, WsError> {
            let frame = self.frame_codec.receive().await?;
            let (header, mut data) = frame.parts();
            let code = header.opcode();
            let close_code = if code == OpCode::Close {
                Some(data.get_u16())
            } else {
                None
            };
            Ok(Message {
                code,
                data,
                close_code,
            })
        }
    };
}

macro_rules! impl_send {
    () => {
        /// send a message
        pub async fn send<'a, T: Into<Message<&'a [u8]>>>(
            &mut self,
            msg: T,
        ) -> Result<(), WsError> {
            let msg: Message<&'a [u8]> = msg.into();
            if let Some(close_code) = msg.close_code {
                if msg.code == OpCode::Close {
                    let mut data = close_code.to_be_bytes().to_vec();
                    data.extend_from_slice(msg.data);
                    self.frame_codec.send(msg.code, &data).await
                } else {
                    self.frame_codec.send(msg.code, msg.data).await
                }
            } else {
                self.frame_codec.send(msg.code, msg.data).await
            }
        }

        /// flush underlaying stream
        pub async fn flush(&mut self) -> Result<(), WsError> {
            self.frame_codec.flush().await
        }
    };
}

/// recv part of bytes message
pub struct AsyncBytesRecv<S: AsyncRead> {
    frame_codec: AsyncFrameRecv<S>,
}

impl<S: AsyncRead + Unpin> AsyncBytesRecv<S> {
    /// construct method
    pub fn new(stream: S, state: FrameReadState) -> Self {
        Self {
            frame_codec: AsyncFrameRecv::new(stream, state),
        }
    }

    impl_recv! {}
}

/// send part of bytes message
pub struct AsyncBytesSend<S: AsyncWrite> {
    frame_codec: AsyncFrameSend<S>,
}

impl<S: AsyncWrite + Unpin> AsyncBytesSend<S> {
    /// construct method
    pub fn new(stream: S, state: FrameWriteState) -> Self {
        Self {
            frame_codec: AsyncFrameSend::new(stream, state),
        }
    }

    impl_send! {}
}

/// recv/send bytes message
pub struct AsyncBytesCodec<S: AsyncRead + AsyncWrite> {
    frame_codec: AsyncFrameCodec<S>,
}

impl<S: AsyncRead + AsyncWrite + Unpin> AsyncBytesCodec<S> {
    /// construct method
    pub fn new(stream: S) -> Self {
        Self {
            frame_codec: AsyncFrameCodec::new(stream),
        }
    }

    /// construct with stream & config
    pub fn new_with(stream: S, config: FrameConfig, read_bytes: BytesMut) -> Self {
        Self {
            frame_codec: AsyncFrameCodec::new_with(stream, config, read_bytes),
        }
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

    /// get mutable underlying stream
    pub fn stream_mut(&mut self) -> &mut S {
        self.frame_codec.stream_mut()
    }

    impl_recv! {}

    impl_send! {}
}

impl<R, W, S> AsyncBytesCodec<S>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
    S: AsyncRead + AsyncWrite + Unpin + Split<R = R, W = W>,
{
    /// split codec to recv and send parts
    pub fn split(self) -> (AsyncBytesRecv<R>, AsyncBytesSend<W>) {
        let AsyncFrameCodec {
            stream,
            read_state,
            write_state,
        } = self.frame_codec;
        let (read, write) = stream.split();
        (
            AsyncBytesRecv::new(read, read_state),
            AsyncBytesSend::new(write, write_state),
        )
    }
}
