use std::collections::HashMap;
use std::fmt::Debug;

use bytes::{Bytes, BytesMut};
use frame::{Frame, OpCode};

/// websocket error definitions
pub mod errors;
/// websocket transport unit
pub mod frame;
/// build connection & read/write frame utils
pub mod protocol;

#[cfg(feature = "proxy")]
/// connection proxy support
pub mod proxy;

/// stream definition
pub mod stream;

/// frame codec impl
pub mod codec;

/// websocket connection state
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionState {
    /// init state
    Created,
    /// tcp & tls connection creating state
    HandShaking,
    /// websocket connection has been successfully established
    Running,
    /// client or peer has send "close frame"
    Closing,
    /// client or peer have send "close" response frame
    Closed,
}

pub struct ClientBuilder {
    uri: String,
    #[cfg(feature = "proxy")]
    proxy_uri: Option<String>,
    protocols: Vec<String>,
    extensions: Vec<String>,
    #[cfg(feature = "async_tls_rustls")]
    certs: std::collections::HashSet<std::path::PathBuf>,
    version: u8,
    headers: HashMap<String, String>,
}

impl ClientBuilder {
    pub fn new<S: ToString>(uri: S) -> Self {
        Self {
            uri: uri.to_string(),
            #[cfg(feature = "proxy")]
            proxy_uri: None,
            protocols: vec![],
            extensions: vec![],
            headers: HashMap::new(),
            #[cfg(feature = "async_tls_rustls")]
            certs: std::collections::HashSet::new(),
            version: 13,
        }
    }

    #[cfg(feature = "proxy")]
    pub fn proxy<S: ToString>(self, uri: S) -> Self {
        Self {
            proxy_uri: Some(uri.to_string()),
            ..self
        }
    }

    /// add protocols
    pub fn protocol(mut self, protocol: String) -> Self {
        self.protocols.push(protocol);
        self
    }

    /// set extension in handshake http header
    ///
    /// **NOTE** it will clear protocols set by `protocol` method
    pub fn protocols(self, protocols: Vec<String>) -> Self {
        Self { protocols, ..self }
    }

    /// add protocols
    pub fn extension(mut self, extension: String) -> Self {
        self.extensions.push(extension);
        self
    }

    /// set extension in handshake http header
    ///
    /// **NOTE** it will clear protocols set by `protocol` method
    pub fn extensions(self, extensions: Vec<String>) -> Self {
        Self { extensions, ..self }
    }

    #[cfg(feature = "async_tls_rustls")]
    pub fn cert(mut self, cert: std::path::PathBuf) -> Self {
        self.certs.insert(cert);
        self
    }

    #[cfg(feature = "async_tls_rustls")]
    // set ssl certs in wss connection
    ///
    /// **NOTE** it will clear certs set by `cert` method
    pub fn certs(self, certs: std::collections::HashSet<std::path::PathBuf>) -> Self {
        Self { certs, ..self }
    }

    /// set websocket version
    pub fn version(self, version: u8) -> Self {
        Self { version, ..self }
    }

    pub fn header<K: ToString, V: ToString>(mut self, name: K, value: V) -> Self {
        self.headers.insert(name.to_string(), value.to_string());
        self
    }

    pub fn headers(self, headers: HashMap<String, String>) -> Self {
        Self { headers, ..self }
    }
}

#[cfg(feature = "blocking")]
mod blocking {
    use std::{
        io::{Read, Write},
        net::TcpStream,
    };

    use crate::{
        errors::WsError,
        protocol::{handle_handshake, req_handshake, Mode},
        stream::WsStream,
        ClientBuilder, ServerBuilder,
    };

    impl ClientBuilder {
        fn _connect(&self) -> Result<(String, http::Response<()>, WsStream<TcpStream>), WsError> {
            let Self {
                uri,
                #[cfg(feature = "proxy")]
                proxy_uri,
                protocols,
                extensions,
                #[cfg(feature = "async_tls_rustls")]
                certs,
                version,
                headers,
            } = self;
            let uri = uri
                .parse::<http::Uri>()
                .map_err(|e| WsError::InvalidUri(format!("{} {}", uri, e.to_string())))?;
            let mode = if let Some(schema) = uri.scheme_str() {
                match schema.to_ascii_lowercase().as_str() {
                    "ws" => Ok(Mode::WS),
                    "wss" => Ok(Mode::WSS),
                    _ => Err(WsError::InvalidUri(format!("invalid schema {}", schema))),
                }
            } else {
                Err(WsError::InvalidUri("missing ws or wss schema".to_string()))
            }?;
            #[cfg(feature = "async_tls_rustls")]
            if mode == Mode::WS && !certs.is_empty() {
                tracing::warn!("setting tls cert has no effect on insecure ws")
            }
            let host = uri
                .host()
                .ok_or_else(|| WsError::InvalidUri(format!("can not find host {}", self.uri)))?;
            let port = match uri.port_u16() {
                Some(port) => port,
                None => mode.default_port(),
            };

            let stream = TcpStream::connect((host, port)).map_err(|e| {
                WsError::ConnectionFailed(format!(
                    "failed to create tcp connection {}",
                    e.to_string()
                ))
            })?;

            tracing::debug!("tcp connection established");

            let mut stream = match mode {
                Mode::WS => WsStream::Plain(stream),
                Mode::WSS => {
                    #[cfg(feature = "tls_rustls")]
                    {
                        use crate::protocol::wrap_tls;
                        let tls_stream = wrap_tls(stream, host, &self.certs)?;
                        WsStream::Tls(tls_stream)
                    }

                    #[cfg(not(feature = "tls_rustls"))]
                    {
                        panic!("require `rustls`")
                    }
                }
            };
            let (key, resp) = req_handshake(
                &mut stream,
                &mode,
                &uri,
                protocols
                    .iter()
                    .cloned()
                    .collect::<Vec<String>>()
                    .join(" ,"),
                extensions
                    .iter()
                    .cloned()
                    .collect::<Vec<String>>()
                    .join(" ,"),
                *version,
                headers.clone(),
            )?;
            Ok((key, resp, stream))
        }

        pub fn connect<C, F>(&self, check_fn: F) -> Result<C, WsError>
        where
            F: Fn(String, http::Response<()>, WsStream<TcpStream>) -> Result<C, WsError>,
        {
            let (key, resp, stream) = self._connect()?;
            check_fn(key, resp, stream)
        }
    }

    impl ServerBuilder {
        pub fn accept<F1, F2, T, C, S>(
            stream: S,
            handshake_handler: F1,
            codec_factory: F2,
        ) -> Result<C, WsError>
        where
            S: Read + Write,
            F1: Fn(http::Request<()>) -> Result<(http::Request<()>, http::Response<T>), WsError>,
            F2: Fn(http::Request<()>, WsStream<S>) -> Result<C, WsError>,
            T: ToString + std::fmt::Debug,
        {
            let mut stream = WsStream::Plain(stream);
            let req = handle_handshake(&mut stream)?;
            let (req, resp) = handshake_handler(req)?;
            let mut resp_lines = vec![format!("{:?} {}", resp.version(), resp.status())];
            resp.headers().iter().for_each(|(k, v)| {
                resp_lines.push(format!("{}: {}", k, v.to_str().unwrap_or_default()))
            });
            resp_lines.push("\r\n".to_string());
            stream.write_all(resp_lines.join("\r\n").as_bytes())?;
            tracing::debug!("{:?}", &resp);
            if resp.status() != http::StatusCode::SWITCHING_PROTOCOLS {
                return Err(WsError::HandShakeFailed(resp.body().to_string()));
            }
            codec_factory(req, stream)
        }
    }
}

#[cfg(feature = "async")]
mod non_blocking {
    use std::fmt::Debug;

    use tokio::{
        io::{AsyncRead, AsyncWrite, AsyncWriteExt},
        net::TcpStream,
    };

    use crate::{
        errors::WsError,
        protocol::{async_handle_handshake, async_req_handshake, Mode},
        stream::WsAsyncStream,
        ServerBuilder,
    };

    use super::ClientBuilder;

    impl ClientBuilder {
        async fn _async_connect(
            &self,
        ) -> Result<(String, http::Response<()>, WsAsyncStream<TcpStream>), WsError> {
            let Self {
                uri,
                #[cfg(feature = "proxy")]
                proxy_uri,
                protocols,
                extensions,
                #[cfg(feature = "async_tls_rustls")]
                certs,
                version,
                headers,
            } = self;
            let uri = uri
                .parse::<http::Uri>()
                .map_err(|e| WsError::InvalidUri(format!("{} {}", uri, e.to_string())))?;
            let mode = if let Some(schema) = uri.scheme_str() {
                match schema.to_ascii_lowercase().as_str() {
                    "ws" => Ok(Mode::WS),
                    "wss" => Ok(Mode::WSS),
                    _ => Err(WsError::InvalidUri(format!("invalid schema {}", schema))),
                }
            } else {
                Err(WsError::InvalidUri("missing ws or wss schema".to_string()))
            }?;
            #[cfg(feature = "async_tls_rustls")]
            if mode == Mode::WS && !certs.is_empty() {
                tracing::warn!("setting tls cert has no effect on insecure ws")
            }
            let host = uri
                .host()
                .ok_or_else(|| WsError::InvalidUri(format!("can not find host {}", self.uri)))?;
            let port = match uri.port_u16() {
                Some(port) => port,
                None => mode.default_port(),
            };

            let stream;
            #[cfg(feature = "proxy")]
            {
                let ws_proxy: Option<super::proxy::Proxy> = match proxy_uri {
                    Some(uri) => Some(uri.parse()?),
                    None => None,
                };
                stream = match &ws_proxy {
                    Some(proxy_conf) => proxy_conf.connect((host, port)).await?,
                    None => TcpStream::connect((host, port)).await.map_err(|e| {
                        WsError::ConnectionFailed(format!(
                            "failed to create tcp connection {}",
                            e.to_string()
                        ))
                    })?,
                };
            }

            #[cfg(not(feature = "proxy"))]
            {
                stream = TcpStream::connect((host, port)).await.map_err(|e| {
                    WsError::ConnectionFailed(format!(
                        "failed to create tcp connection {}",
                        e.to_string()
                    ))
                })?;
            }

            tracing::debug!("tcp connection established");

            let mut stream = match mode {
                Mode::WS => WsAsyncStream::Plain(stream),
                Mode::WSS => {
                    #[cfg(feature = "async_tls_rustls")]
                    {
                        use crate::protocol::async_wrap_tls;
                        let tls_stream = async_wrap_tls(stream, host, &self.certs).await?;
                        WsAsyncStream::Tls(tls_stream)
                    }

                    #[cfg(not(feature = "async_tls_rustls"))]
                    {
                        panic!("require `rustls`")
                    }
                }
            };
            let (key, resp) = async_req_handshake(
                &mut stream,
                &mode,
                &uri,
                protocols
                    .iter()
                    .cloned()
                    .collect::<Vec<String>>()
                    .join(" ,"),
                extensions
                    .iter()
                    .cloned()
                    .collect::<Vec<String>>()
                    .join(" ,"),
                *version,
                headers.clone(),
            )
            .await?;
            Ok((key, resp, stream))
        }

        pub async fn async_connect<C, F>(&self, check_fn: F) -> Result<C, WsError>
        where
            F: Fn(String, http::Response<()>, WsAsyncStream<TcpStream>) -> Result<C, WsError>,
        {
            let (key, resp, stream) = self._async_connect().await?;
            check_fn(key, resp, stream)
        }
    }

    impl ServerBuilder {
        pub async fn async_accept<F1, F2, T, C, S>(
            stream: S,
            handshake_handler: F1,
            codec_factory: F2,
        ) -> Result<C, WsError>
        where
            S: AsyncRead + AsyncWrite + Unpin,
            F1: Fn(http::Request<()>) -> Result<(http::Request<()>, http::Response<T>), WsError>,
            F2: Fn(http::Request<()>, WsAsyncStream<S>) -> Result<C, WsError>,
            T: ToString + Debug,
        {
            let mut stream = WsAsyncStream::Plain(stream);
            let req = async_handle_handshake(&mut stream).await?;
            let (req, resp) = handshake_handler(req)?;
            let mut resp_lines = vec![format!("{:?} {}", resp.version(), resp.status())];
            resp.headers().iter().for_each(|(k, v)| {
                resp_lines.push(format!("{}: {}", k, v.to_str().unwrap_or_default()))
            });
            resp_lines.push("\r\n".to_string());
            stream.write_all(resp_lines.join("\r\n").as_bytes()).await?;
            tracing::debug!("{:?}", &resp);
            if resp.status() != http::StatusCode::SWITCHING_PROTOCOLS {
                return Err(WsError::HandShakeFailed(resp.body().to_string()));
            }
            codec_factory(req, stream)
        }
    }
}

pub struct ServerBuilder {}

pub trait DefaultCode {
    fn default_code(&self) -> OpCode;
}

impl DefaultCode for String {
    fn default_code(&self) -> OpCode {
        OpCode::Text
    }
}

impl DefaultCode for &[u8] {
    fn default_code(&self) -> OpCode {
        OpCode::Binary
    }
}

impl DefaultCode for BytesMut {
    fn default_code(&self) -> OpCode {
        OpCode::Binary
    }
}

impl DefaultCode for Bytes {
    fn default_code(&self) -> OpCode {
        OpCode::Binary
    }
}

impl DefaultCode for Frame {
    fn default_code(&self) -> OpCode {
        self.opcode()
    }
}

pub struct Message<T: AsRef<[u8]> + DefaultCode> {
    pub code: OpCode,
    pub data: T,

    /// available in close frame only
    ///
    /// see [status code](https://datatracker.ietf.org/doc/html/rfc6455#section-7.4)
    pub close_code: Option<u16>,
}

impl<T: AsRef<[u8]> + DefaultCode> Message<T> {
    pub fn into(self) -> T {
        self.data
    }
}

impl<T: AsRef<[u8]> + DefaultCode> From<(OpCode, T)> for Message<T> {
    fn from(data: (OpCode, T)) -> Self {
        let close_code = if data.0 == OpCode::Close {
            Some(1000)
        } else {
            None
        };
        Self {
            data: data.1,
            code: data.0,
            close_code,
        }
    }
}

impl<T: AsRef<[u8]> + DefaultCode> From<(u16, T)> for Message<T> {
    fn from(data: (u16, T)) -> Self {
        Self {
            code: OpCode::Close,
            close_code: Some(data.0),
            data: data.1,
        }
    }
}

impl<T: AsRef<[u8]> + DefaultCode> From<T> for Message<T> {
    fn from(data: T) -> Self {
        Self {
            code: data.default_code(),
            data,
            close_code: None,
        }
    }
}
