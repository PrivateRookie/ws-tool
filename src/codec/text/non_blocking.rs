use bytes::{Buf, BytesMut};
use tokio::io::{AsyncRead, AsyncWrite};

use crate::{
    codec::{
        AsyncWsFrameCodec, AsyncWsFrameRecv, AsyncWsFrameSend, FrameConfig, FrameReadState,
        FrameWriteState, Split,
    },
    errors::{ProtocolError, WsError},
    frame::OpCode,
    protocol::standard_handshake_resp_check,
    Message,
};

macro_rules! impl_recv {
    () => {
        /// for close frame with body, first two bytes of string are close reason
        pub async fn receive(&mut self) -> Result<Message<String>, WsError> {
            let frame = self.frame_codec.receive().await?;
            let (header, mut data) = frame.parts();
            let op_code = header.opcode();
            // TODO check protocol error
            let close_code = if op_code == OpCode::Close && data.len() >= 2 {
                let close_code = data.get_u16();
                Some(close_code)
            } else {
                None
            };
            let data = if self.validate_utf8 && op_code == OpCode::Text {
                String::from_utf8(data.to_vec()).map_err(|_| WsError::ProtocolError {
                    close_code: 1001,
                    error: ProtocolError::InvalidUtf8,
                })?
            } else {
                String::from_utf8_lossy(&data).to_string()
            };
            Ok(Message {
                data,
                close_code,
                code: op_code,
            })
        }
    };
}

macro_rules! impl_send {
    () => {
        /// send text message
        pub async fn send<T: Into<Message<String>>>(&mut self, msg: T) -> Result<(), WsError> {
            let msg: Message<String> = msg.into();
            if let Some(close_code) = msg.close_code {
                if msg.code == OpCode::Close {
                    let mut data = close_code.to_be_bytes().to_vec();
                    data.extend_from_slice(msg.data.as_bytes());
                    self.frame_codec.send(msg.code, &data).await
                } else {
                    self.frame_codec.send(msg.code, msg.data.as_bytes()).await
                }
            } else {
                self.frame_codec.send(msg.code, msg.data.as_bytes()).await
            }
        }

        /// flush underlying stream
        pub async fn flush(&mut self) -> Result<(), WsError> {
            self.frame_codec.flush().await
        }
    };
}

/// send part of text message
pub struct AsyncWsStringRecv<S: AsyncRead> {
    frame_codec: AsyncWsFrameRecv<S>,
    validate_utf8: bool,
}

impl<S: AsyncRead + Unpin> AsyncWsStringRecv<S> {
    /// construct method
    pub fn new(stream: S, state: FrameReadState, validate_utf8: bool) -> Self {
        Self {
            frame_codec: AsyncWsFrameRecv::new(stream, state),
            validate_utf8,
        }
    }

    impl_recv! {}
}

/// recv/send text message
pub struct AsyncWsStringSend<S: AsyncWrite> {
    frame_codec: AsyncWsFrameSend<S>,
}

impl<S: AsyncWrite + Unpin> AsyncWsStringSend<S> {
    /// construct method
    pub fn new(stream: S, state: FrameWriteState) -> Self {
        Self {
            frame_codec: AsyncWsFrameSend::new(stream, state),
        }
    }

    impl_send! {}
}

/// recv/send text message
pub struct AsyncWsStringCodec<S: AsyncRead + AsyncWrite> {
    frame_codec: AsyncWsFrameCodec<S>,
    validate_utf8: bool,
}

impl<S: AsyncRead + AsyncWrite + Unpin> AsyncWsStringCodec<S> {
    /// construct method
    pub fn new(stream: S) -> Self {
        Self {
            frame_codec: AsyncWsFrameCodec::new(stream),
            validate_utf8: false,
        }
    }

    /// construct with config
    pub fn new_with(stream: S, config: FrameConfig, remain: BytesMut, validate_utf8: bool) -> Self {
        Self {
            frame_codec: AsyncWsFrameCodec::new_with(stream, config, remain),
            validate_utf8,
        }
    }

    /// get mutable underlying stream
    pub fn stream_mut(&mut self) -> &mut S {
        self.frame_codec.stream_mut()
    }

    /// used for server side to construct a new server
    pub fn factory(_req: http::Request<()>, remain: BytesMut, stream: S) -> Result<Self, WsError> {
        let config = FrameConfig {
            mask_send_frame: false,
            ..Default::default()
        };
        Ok(Self::new_with(stream, config, remain, true))
    }

    /// used to client side to construct a new client
    pub fn check_fn(
        key: String,
        resp: http::Response<()>,
        remain: BytesMut,
        stream: S,
    ) -> Result<Self, WsError> {
        standard_handshake_resp_check(key.as_bytes(), &resp)?;
        Ok(Self::new_with(stream, FrameConfig::default(), remain, true))
    }

    impl_recv! {}
    impl_send! {}
}

impl<R, W, S> AsyncWsStringCodec<S>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
    S: AsyncRead + AsyncWrite + Unpin + Split<R = R, W = W>,
{
    /// split codec to recv and send parts
    pub fn split(self) -> (AsyncWsStringRecv<R>, AsyncWsStringSend<W>) {
        let AsyncWsFrameCodec {
            stream,
            read_state,
            write_state,
        } = self.frame_codec;
        let (read, write) = stream.split();
        (
            AsyncWsStringRecv::new(read, read_state, self.validate_utf8),
            AsyncWsStringSend::new(write, write_state),
        )
    }
}
