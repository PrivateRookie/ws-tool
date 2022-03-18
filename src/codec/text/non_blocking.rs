use bytes::{Buf, BytesMut};
use tokio::io::{AsyncRead, AsyncWrite};

use crate::{
    codec::{AsyncWsFrameCodec, FrameConfig},
    errors::{ProtocolError, WsError},
    frame::OpCode,
    protocol::standard_handshake_resp_check,
    Message,
};

pub struct AsyncWsStringCodec<S: AsyncRead + AsyncWrite> {
    frame_codec: AsyncWsFrameCodec<S>,
    validate_utf8: bool,
}

impl<S: AsyncRead + AsyncWrite + Unpin> AsyncWsStringCodec<S> {
    pub fn new(stream: S) -> Self {
        Self {
            frame_codec: AsyncWsFrameCodec::new(stream),
            validate_utf8: false,
        }
    }

    pub fn new_with(stream: S, config: FrameConfig, remain: BytesMut, validate_utf8: bool) -> Self {
        Self {
            frame_codec: AsyncWsFrameCodec::new_with(stream, config, remain),
            validate_utf8,
        }
    }

    pub fn stream_mut(&mut self) -> &mut S {
        self.frame_codec.stream_mut()
    }

    pub fn check_fn(
        key: String,
        resp: http::Response<()>,
        remain: BytesMut,
        stream: S,
    ) -> Result<Self, WsError> {
        standard_handshake_resp_check(key.as_bytes(), &resp)?;
        Ok(Self::new_with(stream, FrameConfig::default(), remain, true))
    }

    pub fn factory(_req: http::Request<()>, remain: BytesMut, stream: S) -> Result<Self, WsError> {
        let mut config = FrameConfig::default();
        config.mask_send_frame = false;
        Ok(Self::new_with(stream, config, remain, true))
    }

    pub async fn receive(&mut self) -> Result<Message<String>, WsError> {
        let frame = self.frame_codec.receive().await?;
        let header = frame.header();
        let header_len = header.payload_idx().0;
        let op_code = header.opcode();
        let mut data = frame.0;
        data.advance(header_len);
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

    pub async fn send<T: Into<Message<String>>>(&mut self, msg: T) -> Result<(), WsError> {
        let msg: Message<String> = msg.into();
        if let Some(close_code) = msg.close_code {
            if msg.code == OpCode::Close {
                self.frame_codec
                    .send_mut(
                        msg.code,
                        vec![
                            &mut close_code.to_be_bytes()[..],
                            &mut msg.data.as_bytes().to_vec(),
                        ],
                        true,
                    )
                    .await
            } else {
                self.frame_codec
                    .send_mut(msg.code, &mut msg.data.as_bytes().to_vec()[..], true)
                    .await
            }
        } else {
            self.frame_codec
                .send_mut(msg.code, &mut msg.data.as_bytes().to_vec()[..], true)
                .await
        }
    }
}
