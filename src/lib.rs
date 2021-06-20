use std::collections::HashSet;
use std::fmt::Debug;
use std::path::PathBuf;

use bytes::BytesMut;
use frame::Frame;
use frame::OpCode;
use log::trace;
use protocol::perform_handshake;
use protocol::read_frame;
use protocol::write_frame;
use stream::WsStream;
use tokio::io::AsyncReadExt;
use tokio::net::TcpStream;

pub mod errors;
pub mod frame;
pub mod protocol;
pub mod proxy;
pub mod stream;
use errors::WsError;

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
    protocols: HashSet<String>,
    extensions: HashSet<String>,
    certs: HashSet<PathBuf>,
}

impl ConnBuilder {
    pub fn new(uri: &str) -> Self {
        Self {
            uri: uri.to_string(),
            proxy_uri: None,
            protocols: HashSet::new(),
            extensions: HashSet::new(),
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

    pub async fn build(&self) -> Result<Client, WsError> {
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
            Err(WsError::InvalidUri(format!("missing ws or wss schema")))
        }?;
        if mode == Mode::WS && !certs.is_empty() {
            log::warn!("setting tls cert has no effect on insecure ws")
        }
        let ws_proxy: Option<proxy::Proxy> = match proxy_uri {
            Some(uri) => Some(uri.parse()?),
            None => None,
        };

        let host = uri.host().ok_or(WsError::InvalidUri(format!(
            "can not find host {}",
            self.uri
        )))?;
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
        log::debug!("tcp connection established");
        let stream = match mode {
            Mode::WS => WsStream::Plain(stream),
            Mode::WSS => {
                let tls_stream = wrap_tls(stream, host, &self.certs).await?;
                WsStream::Tls(tls_stream)
            }
        };
        Ok(Client {
            uri,
            mode,
            stream,
            state: ConnectionState::Created,
            certs: certs.clone(),
            handshake_remaining: BytesMut::with_capacity(0),
            proxy: ws_proxy,
            protocols: protocols.to_owned(),
            extensions: extensions.to_owned(),
        })
    }
}

/// websocket client, use ConnBuilder to construct new client
#[derive(Debug)]
pub struct Client {
    uri: http::Uri,
    mode: Mode,
    stream: stream::WsStream,
    certs: HashSet<PathBuf>,
    state: ConnectionState,
    handshake_remaining: BytesMut,
    proxy: Option<proxy::Proxy>,
    protocols: HashSet<String>,
    extensions: HashSet<String>,
}

impl Client {
    pub async fn connect(&mut self) -> Result<protocol::HandshakeResponse, WsError> {
        self.state = ConnectionState::HandShaking;
        let protocols = self
            .protocols
            .iter()
            .map(|p| p.to_string())
            .collect::<Vec<String>>()
            .join(" ");
        let extensions = self
            .extensions
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<String>>()
            .join(" ");

        let (resp, remaining_bytes) = perform_handshake(
            &mut self.stream,
            &self.mode,
            &self.uri,
            protocols,
            extensions,
            13,
        )
        .await?;
        self.handshake_remaining = remaining_bytes;
        self.state = ConnectionState::Running;
        Ok(resp)
    }

    async fn read(&mut self) -> Result<Frame, WsError> {
        let frame = if self.handshake_remaining.is_empty() {
            read_frame(&mut self.stream).await.map(|(frame, _)| frame)
        } else {
            let mut stream = self.handshake_remaining.chain(&mut self.stream);
            let (frame, count) = read_frame(&mut stream).await?;
            let start_idx = count.min(self.handshake_remaining.len());
            self.handshake_remaining = BytesMut::from(&self.handshake_remaining[start_idx..]);
            Ok(frame)
        }?;
        trace!("{:?}", frame);
        Ok(frame)
    }

    async fn write(&mut self, frame: Frame) -> Result<(), WsError> {
        write_frame(&mut self.stream, frame).await
    }

    pub async fn read_frame(&mut self) -> Result<Frame, WsError> {
        if self.state != ConnectionState::Running {
            return Err(WsError::InvalidConnState(self.state.clone()));
        }
        let mut fragmented = false;
        let mut fragmented_data = BytesMut::new();
        let mut fragmented_type = OpCode::Text;
        loop {
            let frame = self.read().await?;
            let opcode = frame.opcode();
            match opcode {
                OpCode::Continue => {
                    if !fragmented {
                        let reason = "missing first fragmented frame".to_string();
                        self.close(1002, reason.clone()).await?;
                        return Err(WsError::ProtocolError(reason));
                    }
                    fragmented_data.extend_from_slice(&frame.payload_data_unmask());
                    if frame.fin() {
                        let completed_frame =
                            Frame::new_with_payload(fragmented_type, &fragmented_data);
                        return Ok(completed_frame);
                    }
                }
                OpCode::Text | OpCode::Binary => {
                    if fragmented {
                        let reason = "not continue frame after first fragmented frame".to_string();
                        self.close(1002, reason.clone()).await?;
                        return Err(WsError::ProtocolError(reason));
                    }
                    if !frame.fin() {
                        fragmented = true;
                        fragmented_type = opcode;
                        fragmented_data.extend_from_slice(&frame.payload_data_unmask());
                    } else {
                        return Ok(frame);
                    }
                }
                OpCode::Close | OpCode::Ping | OpCode::Pong => {
                    if !frame.fin() {
                        let reason = "control frame can not be fragmented".to_string();
                        self.close(1002, reason.clone()).await?;
                        return Err(WsError::ProtocolError(reason));
                    }
                    let payload_len = frame.payload_len();
                    if payload_len > 125 {
                        let reason = format!("control frame is too big {}", payload_len);
                        self.close(1002, reason.clone()).await?;
                        return Err(WsError::ProtocolError(reason));
                    }
                    if opcode == OpCode::Close && payload_len == 1 {
                        let reason = format!("invalid close frame payload len {}", payload_len);
                        self.close(1002, reason.clone()).await?;
                        return Err(WsError::ProtocolError(reason));
                    }
                    if opcode == OpCode::Close || !fragmented {
                        return Ok(frame);
                    } else {
                        log::debug!("{:?} frame between fragmented data", opcode);
                        let echo =
                            Frame::new_with_payload(OpCode::Pong, &frame.payload_data_unmask());
                        self.write_frame(echo).await?;
                    }
                }
                OpCode::ReservedNonControl | OpCode::ReservedControl => {
                    self.close(1002, format!("can not handle {:?} frame", opcode))
                        .await?;
                    return Err(WsError::UnsupportedFrame(opcode));
                }
            }
        }
    }

    pub async fn write_frame(&mut self, frame: Frame) -> Result<(), WsError> {
        if self.state != ConnectionState::Running {
            return Err(WsError::InvalidConnState(self.state.clone()));
        }
        self.write(frame).await
    }

    pub async fn close(&mut self, code: u16, reason: String) -> Result<(), WsError> {
        self.state = ConnectionState::Closing;
        let mut payload = BytesMut::with_capacity(2 + reason.len());
        payload.extend_from_slice(&code.to_be_bytes());
        payload.extend_from_slice(reason.as_bytes());
        let close = Frame::new_with_payload(OpCode::Close, &payload);
        self.write(close).await
    }
}
