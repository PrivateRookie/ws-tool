use std::io::{Read, Write};

use bytes::Buf;

use crate::{
    codec::{
        FrameConfig, FrameReadState, FrameWriteState, Split, WsFrameCodec, WsFrameRecv, WsFrameSend,
    },
    errors::{ProtocolError, WsError},
    frame::OpCode,
    protocol::standard_handshake_resp_check,
    Message,
};

macro_rules! impl_recv {
    () => {
        /// for close frame with body, first two bytes of string are close reason
        pub fn receive(&mut self) -> Result<Message<String>, WsError> {
            let frame = self.frame_codec.receive()?;
            let header = frame.header();
            let header_len = header.payload_idx().0;
            let op_code = header.opcode();
            let mut data = frame.0;
            data.advance(header_len);
            let close_code = if op_code == OpCode::Close {
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
                    self.frame_codec.send_mut(
                        msg.code,
                        vec![
                            &mut close_code.to_be_bytes()[..],
                            &mut msg.data.as_bytes().to_vec(),
                        ],
                        true,
                    )
                } else {
                    self.frame_codec
                        .send_mut(msg.code, &mut msg.data.as_bytes().to_vec()[..], true)
                }
            } else {
                self.frame_codec
                    .send_mut(msg.code, &mut msg.data.as_bytes().to_vec()[..], true)
            }
        }
    };
}

/// recv part of text message
pub struct WsStringRecv<S: Read> {
    frame_codec: WsFrameRecv<S>,
    validate_utf8: bool,
}

impl<S: Read> WsStringRecv<S> {
    /// construct method
    pub fn new(stream: S, state: FrameReadState, validate_utf8: bool) -> Self {
        Self {
            frame_codec: WsFrameRecv::new(stream, state),
            validate_utf8,
        }
    }

    impl_recv! {}
}

/// send part of text message
pub struct WsStringSend<S: Write> {
    frame_codec: WsFrameSend<S>,
}

impl<S: Write> WsStringSend<S> {
    /// construct method
    pub fn new(stream: S, state: FrameWriteState) -> Self {
        Self {
            frame_codec: WsFrameSend::new(stream, state),
        }
    }

    impl_send! {}
}

/// recv/send text message
pub struct WsStringCodec<S: Read + Write> {
    frame_codec: WsFrameCodec<S>,
    validate_utf8: bool,
}

impl<S: Read + Write> WsStringCodec<S> {
    /// construct method
    pub fn new(stream: S) -> Self {
        Self {
            frame_codec: WsFrameCodec::new(stream),
            validate_utf8: false,
        }
    }

    /// construct with config
    pub fn new_with(stream: S, config: FrameConfig, validate_utf8: bool) -> Self {
        Self {
            frame_codec: WsFrameCodec::new_with(stream, config),
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

impl<R, W, S> WsStringCodec<S>
where
    R: Read,
    W: Write,
    S: Read + Write + Split<R = R, W = W>,
{
    /// split codec to recv and send parts
    pub fn split(self) -> (WsStringRecv<R>, WsStringSend<W>) {
        let WsFrameCodec {
            stream,
            read_state,
            write_state,
        } = self.frame_codec;
        let (read, write) = stream.split();
        (
            WsStringRecv::new(read, read_state, self.validate_utf8),
            WsStringSend::new(write, write_state),
        )
    }
}
