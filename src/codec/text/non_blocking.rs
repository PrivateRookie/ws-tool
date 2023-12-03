use crate::http;
use crate::{
    codec::{
        AsyncFrameCodec, AsyncFrameRecv, AsyncFrameSend, FrameConfig, FrameReadState,
        FrameWriteState, Split,
    },
    errors::{ProtocolError, WsError},
    frame::OpCode,
    protocol::standard_handshake_resp_check,
    Message,
};
use bytes::Buf;
use std::borrow::Cow;
use tokio::io::{AsyncRead, AsyncWrite};

macro_rules! impl_recv {
    () => {
        /// in case of ping/pong/close contain non utf-8 string, use this api to receive raw message
        ///
        /// for close frame with body, first two bytes of string are close reason
        pub async fn receive_raw(&mut self) -> Result<Message<Cow<[u8]>>, WsError> {
            let (header, mut data) = self.frame_codec.receive().await?;
            let close_code = if header.code == OpCode::Close && data.len() >= 2 {
                let code = if data.len() >= 2 {
                    data.get_u16()
                } else {
                    1000
                };
                Some(code)
            } else {
                None
            };
            Ok(Message {
                data: Cow::Borrowed(data),
                close_code,
                code: header.code,
            })
        }

        /// for close frame with body, first two bytes of string are close reason
        pub async fn receive(&mut self) -> Result<Message<Cow<str>>, WsError> {
            let (header, mut data) = self.frame_codec.receive().await?;
            // TODO check protocol error
            let close_code = if header.code == OpCode::Close && data.len() >= 2 {
                let code = if data.len() >= 2 {
                    data.get_u16()
                } else {
                    1000
                };
                Some(code)
            } else {
                None
            };
            let data = if self.validate_utf8 && header.code == OpCode::Text {
                std::str::from_utf8(data).map_err(|_| WsError::ProtocolError {
                    close_code: 1001,
                    error: ProtocolError::InvalidUtf8,
                })?
            } else {
                unsafe { std::str::from_utf8_unchecked(data) }
            };
            Ok(Message {
                data: Cow::Borrowed(data),
                close_code,
                code: header.code,
            })
        }
    };
}

macro_rules! impl_send {
    () => {
        /// helper method to send ping message
        pub async fn ping<'a>(&mut self, msg: &'a str) -> Result<(), WsError> {
            self.send((OpCode::Ping, msg)).await
        }

        /// helper method to send pong message
        pub async fn pong<'a>(&mut self, msg: &'a str) -> Result<(), WsError> {
            self.send((OpCode::Pong, msg)).await
        }

        /// helper method to send close message
        pub async fn close<'a>(&mut self, code: u16, msg: &'a str) -> Result<(), WsError> {
            self.send((code, msg)).await
        }

        /// send text message
        pub async fn send<'a, T: Into<Message<Cow<'a, str>>>>(
            &mut self,
            msg: T,
        ) -> Result<(), WsError> {
            let msg: Message<Cow<'a, str>> = msg.into();
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
pub struct AsyncStringRecv<S: AsyncRead> {
    frame_codec: AsyncFrameRecv<S>,
    validate_utf8: bool,
}

impl<S: AsyncRead + Unpin> AsyncStringRecv<S> {
    /// construct method
    pub fn new(stream: S, state: FrameReadState, validate_utf8: bool) -> Self {
        Self {
            frame_codec: AsyncFrameRecv::new(stream, state),
            validate_utf8,
        }
    }

    impl_recv! {}
}

/// recv/send text message
pub struct AsyncStringSend<S: AsyncWrite> {
    frame_codec: AsyncFrameSend<S>,
}

impl<S: AsyncWrite + Unpin> AsyncStringSend<S> {
    /// construct method
    pub fn new(stream: S, state: FrameWriteState) -> Self {
        Self {
            frame_codec: AsyncFrameSend::new(stream, state),
        }
    }

    impl_send! {}
}

/// recv/send text message
pub struct AsyncStringCodec<S: AsyncRead + AsyncWrite> {
    frame_codec: AsyncFrameCodec<S>,
    validate_utf8: bool,
}

impl<S: AsyncRead + AsyncWrite + Unpin> AsyncStringCodec<S> {
    /// construct method
    pub fn new(stream: S) -> Self {
        Self {
            frame_codec: AsyncFrameCodec::new(stream),
            validate_utf8: false,
        }
    }

    /// construct with config
    pub fn new_with(stream: S, config: FrameConfig, validate_utf8: bool) -> Self {
        Self {
            frame_codec: AsyncFrameCodec::new_with(stream, config),
            validate_utf8,
        }
    }

    /// get mutable underlying stream
    pub fn stream_mut(&mut self) -> &mut S {
        self.frame_codec.stream_mut()
    }

    /// used for server side to construct a new server
    pub fn factory(_req: http::Request<()>, stream: S) -> Result<Self, WsError> {
        let config = FrameConfig {
            mask_send_frame: false,
            ..Default::default()
        };
        Ok(Self::new_with(stream, config, true))
    }

    /// used to client side to construct a new client
    pub fn check_fn(key: String, resp: http::Response<()>, stream: S) -> Result<Self, WsError> {
        standard_handshake_resp_check(key.as_bytes(), &resp)?;
        Ok(Self::new_with(stream, FrameConfig::default(), true))
    }

    impl_recv! {}
    impl_send! {}
}

impl<R, W, S> AsyncStringCodec<S>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
    S: AsyncRead + AsyncWrite + Unpin + Split<R = R, W = W>,
{
    /// split codec to recv and send parts
    pub fn split(self) -> (AsyncStringRecv<R>, AsyncStringSend<W>) {
        let AsyncFrameCodec {
            stream,
            read_state,
            write_state,
        } = self.frame_codec;
        let (read, write) = stream.split();
        (
            AsyncStringRecv::new(read, read_state, self.validate_utf8),
            AsyncStringSend::new(write, write_state),
        )
    }
}
