#[cfg(feature = "blocking")]
mod blocking {
    use std::io::{Read, Write};

    use crate::{
        codec::WsFrameCodec,
        errors::{ProtocolError, WsError},
        frame::OpCode,
    };

    pub struct WsStringCodec<S: Read + Write> {
        frame_codec: WsFrameCodec<S>,
        validate_utf8: bool,
    }

    impl<S: Read + Write> WsStringCodec<S> {
        pub fn new(stream: S, validate_utf8: bool) -> Self {
            Self {
                frame_codec: WsFrameCodec::new(stream),
                validate_utf8,
            }
        }

        pub fn receive(&mut self) -> Result<(OpCode, String), WsError> {
            let frame = self.frame_codec.receive()?;
            let data = frame.payload_data_unmask();
            let s = if self.validate_utf8 {
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
        codec::AsyncWsFrameCodec,
        errors::{ProtocolError, WsError},
        frame::OpCode,
    };

    pub struct AsyncWsStringCodec<S: AsyncRead + AsyncWrite> {
        frame_codec: AsyncWsFrameCodec<S>,
        validate_utf8: bool,
    }

    impl<S: AsyncRead + AsyncWrite + Unpin> AsyncWsStringCodec<S> {
        pub fn new(stream: S, validate_utf8: bool) -> Self {
            Self {
                frame_codec: AsyncWsFrameCodec::new(stream),
                validate_utf8,
            }
        }

        pub async fn receive(&mut self) -> Result<(OpCode, String), WsError> {
            let frame = self.frame_codec.receive().await?;
            let data = frame.payload_data_unmask();
            let s = if self.validate_utf8 {
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
