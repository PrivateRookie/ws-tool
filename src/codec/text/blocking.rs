use crate::{
    codec::{
        FrameConfig, FrameReadState, FrameWriteState, Split, FrameCodec, FrameRecv, FrameSend,
    },
    errors::{ProtocolError, WsError},
    frame::OpCode,
    protocol::standard_handshake_resp_check,
    Message,
};
use bytes::Buf;
use std::io::{Read, Write};

macro_rules! impl_recv {
    () => {
        /// for close frame with body, first two bytes of string are close reason
        pub fn receive(&mut self) -> Result<Message<String>, WsError> {
            let frame = self.frame_codec.receive()?;
            let (header, mut data) = frame.parts();
            let op_code = header.opcode();
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
        pub fn send<T: Into<Message<String>>>(&mut self, msg: T) -> Result<(), WsError> {
            let msg: Message<String> = msg.into();
            if let Some(close_code) = msg.close_code {
                if msg.code == OpCode::Close {
                    let mut data = close_code.to_be_bytes().to_vec();
                    data.extend_from_slice(msg.data.as_bytes());
                    self.frame_codec.send(msg.code, &data)
                } else {
                    self.frame_codec.send(msg.code, msg.data.as_bytes())
                }
            } else {
                self.frame_codec.send(msg.code, msg.data.as_bytes())
            }
        }

        /// flush underlying stream
        pub fn flush(&mut self) -> Result<(), WsError> {
            self.frame_codec.flush()
        }
    };
}

/// recv part of text message
pub struct StringRecv<S: Read> {
    frame_codec: FrameRecv<S>,
    validate_utf8: bool,
}

impl<S: Read> StringRecv<S> {
    /// construct method
    pub fn new(stream: S, state: FrameReadState, validate_utf8: bool) -> Self {
        Self {
            frame_codec: FrameRecv::new(stream, state),
            validate_utf8,
        }
    }

    impl_recv! {}
}

/// send part of text message
pub struct StringSend<S: Write> {
    frame_codec: FrameSend<S>,
}

impl<S: Write> StringSend<S> {
    /// construct method
    pub fn new(stream: S, state: FrameWriteState) -> Self {
        Self {
            frame_codec: FrameSend::new(stream, state),
        }
    }

    impl_send! {}
}

/// recv/send text message
pub struct StringCodec<S: Read + Write> {
    frame_codec: FrameCodec<S>,
    validate_utf8: bool,
}

impl<S: Read + Write> StringCodec<S> {
    /// construct method
    pub fn new(stream: S) -> Self {
        Self {
            frame_codec: FrameCodec::new(stream),
            validate_utf8: false,
        }
    }

    /// construct with config
    pub fn new_with(stream: S, config: FrameConfig, validate_utf8: bool) -> Self {
        Self {
            frame_codec: FrameCodec::new_with(stream, config),
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

impl<R, W, S> StringCodec<S>
where
    R: Read,
    W: Write,
    S: Read + Write + Split<R = R, W = W>,
{
    /// split codec to recv and send parts
    pub fn split(self) -> (StringRecv<R>, StringSend<W>) {
        let FrameCodec {
            stream,
            read_state,
            write_state,
        } = self.frame_codec;
        let (read, write) = stream.split();
        (
            StringRecv::new(read, read_state, self.validate_utf8),
            StringSend::new(write, write_state),
        )
    }
}
