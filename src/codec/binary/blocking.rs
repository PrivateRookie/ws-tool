use crate::http;
use crate::{
    codec::{
        FrameCodec, FrameConfig, FrameReadState, FrameRecv, FrameSend, FrameWriteState, Split,
    },
    errors::WsError,
    frame::OpCode,
    protocol::standard_handshake_resp_check,
    Message,
};
use bytes::Buf;
use std::borrow::Cow;
use std::io::{Read, Write};

macro_rules! impl_recv {
    () => {
        /// receive a message
        pub fn receive(&mut self) -> Result<Message<Cow<[u8]>>, WsError> {
            let (header, mut data) = self.frame_codec.receive()?;
            let close_code = if header.code == OpCode::Close {
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
                code: header.code,
                data: Cow::Borrowed(data),
                close_code,
            })
        }
    };
}

macro_rules! impl_send {
    () => {
        /// helper method to send ping message
        pub fn ping<'a>(&mut self, msg: &'a [u8]) -> Result<(), WsError> {
            self.send((OpCode::Ping, msg))
        }

        /// helper method to send pong message
        pub fn pong<'a>(&mut self, msg: &'a [u8]) -> Result<(), WsError> {
            self.send((OpCode::Pong, msg))
        }

        /// helper method to send close message
        pub fn close<'a>(&mut self, code: u16, msg: &'a [u8]) -> Result<(), WsError> {
            self.send((code, msg))
        }

        /// send a message
        pub fn send<'a, T: Into<Message<Cow<'a, [u8]>>>>(&mut self, msg: T) -> Result<(), WsError> {
            let msg: Message<Cow<'a, [u8]>> = msg.into();
            if let Some(close_code) = msg.close_code {
                if msg.code == OpCode::Close {
                    let mut data = close_code.to_be_bytes().to_vec();
                    data.extend_from_slice(msg.data.as_ref());
                    self.frame_codec.send(msg.code, &data)
                } else {
                    self.frame_codec.send(msg.code, msg.data.as_ref())
                }
            } else {
                self.frame_codec.send(msg.code, msg.data.as_ref())
            }
        }

        /// flush underlying stream
        pub fn flush(&mut self) -> Result<(), WsError> {
            self.frame_codec.flush()
        }
    };
}

/// recv part of bytes message
pub struct BytesRecv<S: Read> {
    frame_codec: FrameRecv<S>,
}

impl<S: Read> BytesRecv<S> {
    /// construct method
    pub fn new(stream: S, state: FrameReadState) -> Self {
        Self {
            frame_codec: FrameRecv::new(stream, state),
        }
    }

    impl_recv! {}
}

/// send part of bytes message
pub struct BytesSend<S: Write> {
    frame_codec: FrameSend<S>,
}

impl<S: Write> BytesSend<S> {
    /// construct method
    pub fn new(stream: S, state: FrameWriteState) -> Self {
        Self {
            frame_codec: FrameSend::new(stream, state),
        }
    }

    impl_send! {}
}

/// recv/send bytes message
pub struct BytesCodec<S: Read + Write> {
    frame_codec: FrameCodec<S>,
}

impl<S: Read + Write> BytesCodec<S> {
    /// construct method
    pub fn new(stream: S) -> Self {
        Self {
            frame_codec: FrameCodec::new(stream),
        }
    }

    /// construct with stream & config
    pub fn new_with(stream: S, config: FrameConfig) -> Self {
        Self {
            frame_codec: FrameCodec::new_with(stream, config),
        }
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

    /// get mutable underlying stream
    pub fn stream_mut(&mut self) -> &mut S {
        self.frame_codec.stream_mut()
    }

    impl_recv! {}

    impl_send! {}
}

impl<R, W, S> BytesCodec<S>
where
    R: Read,
    W: Write,
    S: Read + Write + Split<R = R, W = W>,
{
    /// split codec to recv and send parts
    pub fn split(self) -> (BytesRecv<R>, BytesSend<W>) {
        let FrameCodec {
            stream,
            read_state,
            write_state,
        } = self.frame_codec;
        let (read, write) = stream.split();
        (
            BytesRecv::new(read, read_state),
            BytesSend::new(write, write_state),
        )
    }
}
