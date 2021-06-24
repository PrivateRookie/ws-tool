use std::collections::HashSet;
use std::fmt::Debug;
use std::path::PathBuf;

use bytes::BytesMut;
use config::WebsocketConfig;
use frame::OpCode;
use frame::{DefaultFrameCodec, Frame};
use log::trace;
use protocol::perform_handshake;
use protocol::read_frame;
use protocol::write_frame;
use stream::WsStream;
use tokio::io::AsyncReadExt;
use tokio::net::TcpStream;

pub mod config;
pub mod errors;
pub mod frame;
pub mod protocol;
pub mod proxy;
pub mod stream;
use errors::{ProtocolError, WsError};

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
            log::warn!("setting tls cert has no effect on insecure ws")
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
        log::debug!("tcp connection established");
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
            stream,
            codec: DefaultFrameCodec::default(),
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
pub struct Connection {
    uri: http::Uri,
    mode: Mode,
    codec: DefaultFrameCodec,
    stream: stream::WsStream,
    certs: HashSet<PathBuf>,
    state: ConnectionState,
    handshake_remaining: BytesMut,
    proxy: Option<proxy::Proxy>,
    protocols: Vec<String>,
    extensions: Vec<String>,
}

impl Connection {
    pub async fn handshake(&mut self) -> Result<protocol::HandshakeResponse, WsError> {
        self.state = ConnectionState::HandShaking;
        let protocols = self.protocols.join(" ");
        let extensions = self.extensions.join(" ");

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
            read_frame(&mut self.codec, &mut self.stream)
                .await
                .map(|(frame, _)| frame)
        } else {
            let mut stream = self.handshake_remaining.chain(&mut self.stream);
            let (frame, count) = read_frame(&mut self.codec, &mut stream).await?;
            let start_idx = count.min(self.handshake_remaining.len());
            self.handshake_remaining = BytesMut::from(&self.handshake_remaining[start_idx..]);
            Ok(frame)
        }?;
        trace!("{:?}", frame);
        Ok(frame)
    }

    async fn write(&mut self, frame: Frame) -> Result<(), WsError> {
        write_frame(&mut self.codec, &mut self.stream, frame).await
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
                        let reason = ProtocolError::MissInitialFragmentedFrame;
                        self.close(1002, reason.to_string()).await?;
                        return Err(WsError::ProtocolError(reason));
                    }
                    fragmented_data.extend_from_slice(&frame.payload_data_unmask());
                    if frame.fin() {
                        if String::from_utf8(fragmented_data.to_vec()).is_err() {
                            let reason = ProtocolError::InvalidUtf8;
                            self.close(1007, reason.to_string()).await?;
                            return Err(WsError::ProtocolError(reason));
                        }
                        let completed_frame =
                            Frame::new_with_payload(fragmented_type, &fragmented_data);
                        return Ok(completed_frame);
                    }
                }
                OpCode::Text | OpCode::Binary => {
                    if fragmented {
                        let reason = ProtocolError::NotContinueFrameAfterFragmented;
                        self.close(1002, reason.to_string()).await?;
                        return Err(WsError::ProtocolError(reason));
                    }
                    if !frame.fin() {
                        fragmented = true;
                        fragmented_type = opcode.clone();
                        let payload = frame.payload_data_unmask();
                        fragmented_data.extend_from_slice(&payload);
                    } else {
                        if opcode == OpCode::Text
                            && String::from_utf8(frame.payload_data_unmask().to_vec()).is_err()
                        {
                            let reason = ProtocolError::InvalidUtf8;
                            self.close(1007, reason.to_string()).await?;
                            return Err(WsError::ProtocolError(reason));
                        }
                        return Ok(frame);
                    }
                }
                OpCode::Close | OpCode::Ping | OpCode::Pong => {
                    if !frame.fin() {
                        let reason = ProtocolError::FragmentedControlFrame;
                        self.close(1002, reason.to_string()).await?;
                        return Err(WsError::ProtocolError(reason));
                    }
                    let payload_len = frame.payload_len();
                    if payload_len > 125 {
                        let reason = ProtocolError::ControlFrameTooBig(payload_len as usize);
                        self.close(1002, reason.to_string()).await?;
                        return Err(WsError::ProtocolError(reason));
                    }
                    if opcode == OpCode::Close {
                        if payload_len == 1 {
                            let reason = ProtocolError::InvalidCloseFramePayload;
                            self.close(1002, reason.to_string()).await?;
                            return Err(WsError::ProtocolError(reason));
                        }
                        if payload_len >= 2 {
                            let payload = frame.payload_data();

                            // check close code
                            let mut code_byte = [0u8; 2];
                            code_byte.copy_from_slice(&payload[..2]);
                            let code = u16::from_be_bytes(code_byte);
                            if code < 1000
                                || (1004..=1006).contains(&code)
                                || (1015..=2999).contains(&code)
                                || code >= 5000
                            {
                                let reason = ProtocolError::InvalidCloseCode(code);
                                self.close(1002, reason.to_string()).await?;
                                return Err(WsError::ProtocolError(reason));
                            }

                            // utf-8 validation
                            if String::from_utf8(payload[2..].to_vec()).is_err() {
                                let reason = ProtocolError::InvalidUtf8;
                                self.close(1007, reason.to_string()).await?;
                                return Err(WsError::ProtocolError(reason));
                            }
                        }
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

pub struct Client {
    pub conn: Connection,
    pub config: WebsocketConfig,
}
