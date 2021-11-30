use std::collections::{HashMap, HashSet};
use std::fmt::Debug;
use std::path::PathBuf;

use protocol::perform_handshake;
use stream::WsStream;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;

/// websocket error definitions
pub mod errors;
/// websocket transport unit
pub mod frame;
/// build connection & read/write frame utils
pub mod protocol;
/// connection proxy support
pub mod proxy;
/// stream definition
pub mod stream;

/// frame codec impl
pub mod codec;

use errors::WsError;
use tokio_util::codec::{Decoder, Encoder, Framed};

use crate::protocol::Mode;
use crate::protocol::{handle_handshake, wrap_tls};

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
    proxy_uri: Option<String>,
    protocols: HashSet<String>,
    extensions: HashSet<String>,
    certs: HashSet<PathBuf>,
    version: u8,
    headers: HashMap<String, String>,
}

impl ClientBuilder {
    pub fn new<S: ToString>(uri: S) -> Self {
        Self {
            uri: uri.to_string(),
            proxy_uri: None,
            protocols: HashSet::new(),
            extensions: HashSet::new(),
            headers: HashMap::new(),
            certs: HashSet::new(),
            version: 13,
        }
    }

    pub fn proxy<S: ToString>(self, uri: S) -> Self {
        Self {
            proxy_uri: Some(uri.to_string()),
            ..self
        }
    }

    /// add protocols
    pub fn protocol(mut self, protocol: String) -> Self {
        self.protocols.insert(protocol);
        self
    }

    /// set extension in handshake http header
    ///
    /// **NOTE** it will clear protocols set by `protocol` method
    pub fn protocols(self, protocols: HashSet<String>) -> Self {
        Self { protocols, ..self }
    }

    /// add protocols
    pub fn extension(mut self, extension: String) -> Self {
        self.extensions.insert(extension);
        self
    }

    /// set extension in handshake http header
    ///
    /// **NOTE** it will clear protocols set by `protocol` method
    pub fn extensions(self, extensions: HashSet<String>) -> Self {
        Self { extensions, ..self }
    }

    pub fn cert(mut self, cert: PathBuf) -> Self {
        self.certs.insert(cert);
        self
    }

    // set ssl certs in wss connection
    ///
    /// **NOTE** it will clear certs set by `cert` method
    pub fn certs(self, certs: HashSet<PathBuf>) -> Self {
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

    async fn _connect(&self) -> Result<(String, http::Response<()>, WsStream), WsError> {
        let Self {
            uri,
            proxy_uri,
            protocols,
            extensions,
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
        if mode == Mode::WS && !certs.is_empty() {
            tracing::warn!("setting tls cert has no effect on insecure ws")
        }
        let ws_proxy: Option<proxy::Proxy> = match proxy_uri {
            Some(uri) => Some(uri.parse()?),
            None => None,
        };

        let host = uri
            .host()
            .ok_or_else(|| WsError::InvalidUri(format!("can not find host {}", self.uri)))?;
        let port = match uri.port_u16() {
            Some(port) => port,
            None => mode.default_port(),
        };

        let stream = match &ws_proxy {
            Some(proxy_conf) => proxy_conf.connect((host, port)).await?,
            None => TcpStream::connect((host, port)).await.map_err(|e| {
                WsError::ConnectionFailed(format!(
                    "failed to create tcp connection {}",
                    e.to_string()
                ))
            })?,
        };
        tracing::debug!("tcp connection established");
        let mut stream = match mode {
            Mode::WS => WsStream::Plain(stream),
            Mode::WSS => {
                let tls_stream = wrap_tls(stream, host, &self.certs).await?;
                WsStream::Tls(tls_stream)
            }
        };
        let (key, resp) = perform_handshake(
            &mut stream,
            &mode,
            &uri,
            protocols.iter().cloned().collect::<Vec<String>>().join(" "),
            extensions
                .iter()
                .cloned()
                .collect::<Vec<String>>()
                .join(" "),
            *version,
            headers.clone(),
        )
        .await?;
        Ok((key, resp, stream))
    }

    pub async fn connect_with_check<C, EI, DI, F>(
        &self,
        check_fn: F,
    ) -> Result<Framed<WsStream, C>, WsError>
    where
        C: Encoder<EI, Error = WsError> + Decoder<Item = DI, Error = WsError>,
        F: Fn(String, http::Response<()>, WsStream) -> Result<Framed<WsStream, C>, WsError>,
    {
        let (key, resp, stream) = self._connect().await?;
        check_fn(key, resp, stream)
    }
}

pub struct ServerBuilder {}

impl ServerBuilder {
    pub async fn accept<C, EI, DI, F1, F2, T>(
        stream: TcpStream,
        handshake_handler: F1,
        codec_factory: F2,
    ) -> Result<Framed<WsStream, C>, WsError>
    where
        C: Encoder<EI, Error = WsError> + Decoder<Item = DI, Error = WsError>,
        F1: Fn(http::Request<()>) -> Result<(http::Request<()>, http::Response<T>), WsError>,
        F2: Fn(http::Request<()>, WsStream) -> Result<Framed<WsStream, C>, WsError>,
        T: ToString + Debug,
    {
        let mut stream = WsStream::Plain(stream);
        let req = handle_handshake(&mut stream).await?;
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
