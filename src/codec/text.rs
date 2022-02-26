#[cfg(feature = "blocking")]
mod blocking {
    use std::io::{Read, Write};

    use crate::{
        codec::{FrameConfig, WsFrameCodec},
        errors::{ProtocolError, WsError},
        frame::OpCode,
        protocol::standard_handshake_resp_check,
    };

    pub struct WsStringCodec<S: Read + Write> {
        frame_codec: WsFrameCodec<S>,
        validate_utf8: bool,
    }

    impl<S: Read + Write> WsStringCodec<S> {
        pub fn new(stream: S) -> Self {
            Self {
                frame_codec: WsFrameCodec::new(stream),
                validate_utf8: false,
            }
        }

        pub fn new_with(stream: S, config: FrameConfig, validate_utf8: bool) -> Self {
            Self {
                frame_codec: WsFrameCodec::new_with(stream, config),
                validate_utf8,
            }
        }

        pub fn stream_mut(&mut self) -> &mut S {
            self.frame_codec.stream_mut()
        }

        pub fn check_fn(key: String, resp: http::Response<()>, stream: S) -> Result<Self, WsError> {
            standard_handshake_resp_check(key.as_bytes(), &resp)?;
            Ok(Self::new_with(stream, FrameConfig::default(), true))
        }

        pub fn factory(_req: http::Request<()>, stream: S) -> Result<Self, WsError> {
            let mut config = FrameConfig::default();
            config.mask = false;
            Ok(Self::new_with(stream, config, true))
        }

        pub fn receive(&mut self) -> Result<(OpCode, String), WsError> {
            let frame = self.frame_codec.receive()?;
            let data = frame.payload_data_unmask();
            let s = if self.validate_utf8 && frame.opcode() == OpCode::Text {
                String::from_utf8(data.to_vec()).map_err(|_| WsError::ProtocolError {
                    close_code: 1001,
                    error: ProtocolError::InvalidUtf8,
                })?
            } else {
                String::from_utf8_lossy(&data).to_string()
            };
            Ok((frame.opcode(), s))
        }

        pub fn send<T: Into<Option<OpCode>>>(
            &mut self,
            (code, content): (T, String),
        ) -> Result<usize, WsError> {
            self.frame_codec
                .send(code.into().unwrap_or(OpCode::Text), content.as_bytes())
        }
    }
}

#[cfg(feature = "blocking")]
pub use blocking::WsStringCodec;

#[cfg(feature = "async")]
mod non_blocking {
    use tokio::io::{AsyncRead, AsyncWrite};

    use crate::{
        codec::{AsyncWsFrameCodec, FrameConfig},
        errors::{ProtocolError, WsError},
        frame::OpCode,
        protocol::standard_handshake_resp_check,
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

        pub fn new_with(stream: S, config: FrameConfig, validate_utf8: bool) -> Self {
            Self {
                frame_codec: AsyncWsFrameCodec::new_with(stream, config),
                validate_utf8,
            }
        }

        pub fn stream_mut(&mut self) -> &mut S {
            self.frame_codec.stream_mut()
        }

        pub fn check_fn(key: String, resp: http::Response<()>, stream: S) -> Result<Self, WsError> {
            standard_handshake_resp_check(key.as_bytes(), &resp)?;
            Ok(Self::new_with(stream, FrameConfig::default(), true))
        }

        pub fn factory(_req: http::Request<()>, stream: S) -> Result<Self, WsError> {
            let mut config = FrameConfig::default();
            config.mask = false;
            Ok(Self::new_with(stream, config, true))
        }

        pub async fn receive(&mut self) -> Result<(OpCode, String), WsError> {
            let frame = self.frame_codec.receive().await?;
            let data = frame.payload_data_unmask();
            let s = if self.validate_utf8 && frame.opcode() == OpCode::Text {
                String::from_utf8(data.to_vec()).map_err(|_| WsError::ProtocolError {
                    close_code: 1001,
                    error: ProtocolError::InvalidUtf8,
                })?
            } else {
                String::from_utf8_lossy(&data).to_string()
            };
            Ok((frame.opcode(), s))
        }

        pub async fn send<T: Into<Option<OpCode>>>(
            &mut self,
            (code, content): (T, String),
        ) -> Result<usize, WsError> {
            self.frame_codec
                .send(code.into().unwrap_or(OpCode::Text), content.as_bytes())
                .await
        }
    }
}

#[cfg(feature = "async")]
pub use non_blocking::AsyncWsStringCodec;
