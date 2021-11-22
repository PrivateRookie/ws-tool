use std::collections::HashSet;
use std::fmt::Debug;
use std::path::PathBuf;

use crate::codec::{FrameCodec, FrameDecoder, FrameEncoder};
use crate::frame::{Frame, OpCode};
use bytes::BytesMut;
use futures::SinkExt;
use futures::StreamExt;
use protocol::handle_handshake;
use protocol::perform_handshake;
use stream::WsStream;
use tokio::io::{ReadHalf, WriteHalf};
use tokio::net::TcpStream;

/// client config
pub mod config;
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
use tokio_util::codec::{Framed, FramedRead, FramedWrite};

use crate::protocol::wrap_tls;
use crate::protocol::Mode;

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

pub struct ConnBuilder {
    uri: String,
    proxy_uri: Option<String>,
    protocols: Vec<String>,
    extensions: Vec<String>,
    certs: HashSet<PathBuf>,
}

impl ConnBuilder {
    pub fn new(uri: &str) -> Self {
        Self {
            uri: uri.to_string(),
            proxy_uri: None,
            protocols: Vec::new(),
            extensions: Vec::new(),
            certs: HashSet::new(),
        }
    }

    /// config  proxy
    pub fn proxy(self, uri: &str) -> Self {
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

    pub async fn build(&self) -> Result<Connection, WsError> {
        let Self {
            uri,
            proxy_uri,
            protocols,
            extensions,
            certs,
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
        let stream = match mode {
            Mode::WS => WsStream::Plain(stream),
            Mode::WSS => {
                let tls_stream = wrap_tls(stream, host, &self.certs).await?;
                WsStream::Tls(tls_stream)
            }
        };
        Ok(Connection {
            uri,
            mode,
            framed: Framed::new(stream, FrameCodec::default()),
            state: ConnectionState::Created,
            certs: certs.clone(),
            proxy: ws_proxy,
            protocols: protocols.to_owned(),
            extensions: extensions.to_owned(),
        })
    }
}

/// websocket client, use ConnBuilder to construct new client
#[derive(Debug)]
pub struct Connection {
    uri: http::Uri,
    mode: Mode,
    framed: Framed<stream::WsStream, FrameCodec>,
    certs: HashSet<PathBuf>,
    state: ConnectionState,
    proxy: Option<proxy::Proxy>,
    protocols: Vec<String>,
    extensions: Vec<String>,
}

impl Connection {
    pub async fn handshake(&mut self) -> Result<protocol::HandshakeResponse, WsError> {
        self.state = ConnectionState::HandShaking;
        let protocols = self.protocols.join(" ");
        let extensions = self.extensions.join(" ");
        let resp = perform_handshake(
            self.framed.get_mut(),
            &self.mode,
            &self.uri,
            protocols,
            extensions,
            13,
        )
        .await?;
        self.state = ConnectionState::Running;
        Ok(resp)
    }

    pub async fn write(&mut self, item: Frame) -> Result<(), WsError> {
        if self.state != ConnectionState::Running {
            return Err(WsError::InvalidConnState(self.state.clone()));
        }
        self.framed.send(item).await
    }

    pub async fn read(&mut self) -> Option<Result<Frame, WsError>> {
        match self.framed.next().await {
            Some(maybe_frame) => {
                let msg = match maybe_frame {
                    Ok(frame) => Ok(frame),
                    Err(err) => {
                        let (code, reason) = match &err {
                            WsError::ProtocolError { close_code, error } => {
                                (close_code, error.to_string())
                            }
                            _ => (&1002, err.to_string()),
                        };
                        let _ = self.close(*code, reason).await;
                        self.state = ConnectionState::Closed;
                        Err(err)
                    }
                };
                Some(msg)
            }
            None => None,
        }
    }

    pub async fn close(&mut self, code: u16, reason: String) -> Result<(), WsError> {
        self.state = ConnectionState::Closing;
        let mut payload = BytesMut::with_capacity(2 + reason.as_bytes().len());
        payload.extend_from_slice(&code.to_be_bytes());
        payload.extend_from_slice(reason.as_bytes());
        let close = Frame::new_with_payload(OpCode::Close, &payload);
        self.framed.send(close).await
    }

    pub fn split(
        self,
    ) -> (
        FramedRead<ReadHalf<WsStream>, FrameDecoder>,
        FramedWrite<WriteHalf<WsStream>, FrameEncoder>,
    ) {
        if self.state != ConnectionState::Running {
            panic!("should split after connection is running");
        }
        let Self { framed, .. } = self;
        let parts = framed.into_parts();
        let (read_stream, write_stream) = tokio::io::split(parts.io);
        let frame_r = FramedRead::new(read_stream, parts.codec.decoder);
        let frame_w = FramedWrite::new(write_stream, parts.codec.encoder);
        (frame_r, frame_w)
    }
}

#[derive(Debug)]
pub struct Server {
    pub framed: Framed<stream::WsStream, FrameCodec>,
    pub state: ConnectionState,
    pub protocols: Vec<String>,
    pub extensions: Vec<String>,
}

impl Server {
    pub fn from_stream(stream: TcpStream) -> Self {
        Self {
            state: ConnectionState::Created,
            framed: Framed::new(WsStream::Plain(stream), FrameCodec::default()),
            protocols: vec![],
            extensions: vec![],
        }
    }

    pub async fn handle_handshake(&mut self) -> Result<(), WsError> {
        let stream = self.framed.get_mut();
        handle_handshake(stream).await?;
        self.state = ConnectionState::Running;
        Ok(())
    }

    pub async fn read(&mut self) -> Option<Result<Frame, WsError>> {
        match self.framed.next().await {
            Some(maybe_frame) => {
                let msg = match maybe_frame {
                    Ok(frame) => Ok(frame),
                    Err(err) => {
                        let (code, reason) = match &err {
                            WsError::ProtocolError { close_code, error } => {
                                (close_code, error.to_string())
                            }
                            _ => (&1002, err.to_string()),
                        };
                        let _ = self.close(*code, reason).await;
                        self.state = ConnectionState::Closed;
                        Err(err)
                    }
                };
                Some(msg)
            }
            None => None,
        }
    }

    pub async fn write(&mut self, item: Frame) -> Result<(), WsError> {
        if self.state != ConnectionState::Running {
            return Err(WsError::InvalidConnState(self.state.clone()));
        }
        self.framed.send(item).await
    }

    pub async fn close(&mut self, code: u16, reason: String) -> Result<(), WsError> {
        self.state = ConnectionState::Closing;
        let mut payload = BytesMut::with_capacity(2 + reason.as_bytes().len());
        payload.extend_from_slice(&code.to_be_bytes());
        payload.extend_from_slice(reason.as_bytes());
        let close = Frame::new_with_payload(OpCode::Close, &payload);
        self.framed.send(close).await
    }

    pub fn split(
        self,
    ) -> (
        FramedRead<ReadHalf<WsStream>, FrameDecoder>,
        FramedWrite<WriteHalf<WsStream>, FrameEncoder>,
    ) {
        if self.state != ConnectionState::Running {
            panic!("should split after connection is running");
        }
        let Self { framed, .. } = self;
        let parts = framed.into_parts();
        let (read_stream, write_stream) = tokio::io::split(parts.io);
        let frame_r = FramedRead::new(read_stream, parts.codec.decoder);
        let frame_w = FramedWrite::new(write_stream, parts.codec.encoder);
        (frame_r, frame_w)
    }
}
