use std::io::{Read, Write};

use bytes::{Buf, BytesMut};

use crate::{
    codec::{FrameConfig, WsFrameCodec},
    errors::WsError,
    frame::OpCode,
    protocol::standard_handshake_resp_check,
    Message,
};

pub struct WsBytesCodec<S: Read + Write> {
    frame_codec: WsFrameCodec<S>,
}

impl<S: Read + Write> WsBytesCodec<S> {
    pub fn new(stream: S) -> Self {
        Self {
            frame_codec: WsFrameCodec::new(stream),
        }
    }

    pub fn new_with(stream: S, config: FrameConfig) -> Self {
        Self {
            frame_codec: WsFrameCodec::new_with(stream, config),
        }
    }

    pub fn factory(_req: http::Request<()>, stream: S) -> Result<Self, WsError> {
        let mut config = FrameConfig::default();
        // do not mask server side frame
        config.mask_send_frame = false;
        Ok(Self::new_with(stream, config))
    }

    pub fn check_fn(key: String, resp: http::Response<()>, stream: S) -> Result<Self, WsError> {
        standard_handshake_resp_check(key.as_bytes(), &resp)?;
        Ok(Self::new_with(stream, FrameConfig::default()))
    }

    pub fn stream_mut(&mut self) -> &mut S {
        self.frame_codec.stream_mut()
    }

    pub fn receive(&mut self) -> Result<Message<BytesMut>, WsError> {
        let frame = self.frame_codec.receive()?;
        let header = frame.header();
        let header_len = header.payload_idx().0;
        let op_code = header.opcode();
        let mut data = frame.0;
        data.advance(header_len);
        let close_code = if op_code == OpCode::Close {
            Some(data.get_u16())
        } else {
            None
        };
        Ok(Message {
            code: op_code,
            data,
            close_code,
        })
    }

    pub fn send<'a, T: Into<Message<&'a mut [u8]>>>(&mut self, msg: T) -> Result<(), WsError> {
        let msg: Message<&'a mut [u8]> = msg.into();
        if let Some(close_code) = msg.close_code {
            if msg.code == OpCode::Close {
                self.frame_codec.send_mut(
                    msg.code,
                    vec![&mut close_code.to_be_bytes()[..], msg.data],
                    true,
                )
            } else {
                self.frame_codec.send_mut(msg.code, msg.data, true)
            }
        } else {
            self.frame_codec.send_mut(msg.code, msg.data, true)
        }
    }
}
