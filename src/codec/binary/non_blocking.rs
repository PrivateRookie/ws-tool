use bytes::{Buf, BytesMut};
use tokio::io::{AsyncRead, AsyncWrite};

use crate::{
    codec::{
        AsyncWsFrameCodec, AsyncWsFrameRecv, AsyncWsFrameSend, FrameConfig, FrameReadState,
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
            let header = frame.header();
            let header_len = header.payload_idx().0;
            let code = header.opcode();
            let mut data = frame.0;
            data.advance(header_len);
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
        pub async fn send<'a, T: Into<Message<&'a mut [u8]>>>(
            &mut self,
            msg: T,
        ) -> Result<(), WsError> {
            let msg: Message<&'a mut [u8]> = msg.into();
            if let Some(close_code) = msg.close_code {
                if msg.code == OpCode::Close {
                    self.frame_codec
                        .send_mut(
                            msg.code,
                            vec![&mut close_code.to_be_bytes()[..], msg.data],
                            true,
                        )
                        .await
                } else {
                    self.frame_codec.send_mut(msg.code, msg.data, true).await
                }
            } else {
                self.frame_codec.send_mut(msg.code, msg.data, true).await
            }
        }

        /// flush underlaying stream
        pub async fn flush(&mut self) -> Result<(), WsError> {
            self.frame_codec.flush().await
        }
    };
}

/// recv part of bytes message
pub struct AsyncWsBytesRecv<S: AsyncRead> {
    frame_codec: AsyncWsFrameRecv<S>,
}

impl<S: AsyncRead + Unpin> AsyncWsBytesRecv<S> {
    /// construct method
    pub fn new(stream: S, state: FrameReadState) -> Self {
        Self {
            frame_codec: AsyncWsFrameRecv::new(stream, state),
        }
    }

    impl_recv! {}
}

/// send part of bytes message
pub struct AsyncWsBytesSend<S: AsyncWrite> {
    frame_codec: AsyncWsFrameSend<S>,
}

impl<S: AsyncWrite + Unpin> AsyncWsBytesSend<S> {
    /// construct method
    pub fn new(stream: S, state: FrameWriteState) -> Self {
        Self {
            frame_codec: AsyncWsFrameSend::new(stream, state),
        }
    }

    impl_send! {}
}

/// recv/send bytes message
pub struct AsyncWsBytesCodec<S: AsyncRead + AsyncWrite> {
    frame_codec: AsyncWsFrameCodec<S>,
}

impl<S: AsyncRead + AsyncWrite + Unpin> AsyncWsBytesCodec<S> {
    /// construct method
    pub fn new(stream: S) -> Self {
        Self {
            frame_codec: AsyncWsFrameCodec::new(stream),
        }
    }

    /// construct with stream & config
    pub fn new_with(stream: S, config: FrameConfig, read_bytes: BytesMut) -> Self {
        Self {
            frame_codec: AsyncWsFrameCodec::new_with(stream, config, read_bytes),
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

impl<R, W, S> AsyncWsBytesCodec<S>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
    S: AsyncRead + AsyncWrite + Unpin + Split<R = R, W = W>,
{
    /// split codec to recv and send parts
    pub fn split(self) -> (AsyncWsBytesRecv<R>, AsyncWsBytesSend<W>) {
        let AsyncWsFrameCodec {
            stream,
            read_state,
            write_state,
        } = self.frame_codec;
        let (read, write) = stream.split();
        (
            AsyncWsBytesRecv::new(read, read_state),
            AsyncWsBytesSend::new(write, write_state),
        )
    }
}
