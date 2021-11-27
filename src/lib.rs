use std::borrow::BorrowMut;
use std::collections::HashSet;
use std::fmt::Debug;
use std::ops::Deref;
use std::path::PathBuf;
use std::pin::Pin;

use crate::codec::{FrameCodec, FrameDecoder, FrameEncoder};
use crate::errors::ProtocolError;
use crate::frame::{Frame, OpCode};
use bytes::{Buf, Bytes, BytesMut};
use frame::{get_bit, parse_opcode, parse_payload_len};
use futures::{Sink, SinkExt};
use futures::{Stream, StreamExt};
use protocol::{handle_handshake, standard_handshake_req_check};
use protocol::{perform_handshake, standard_handshake_resp_check};
use stream::WsStream;
use tokio::io::{AsyncRead, AsyncWriteExt, ReadHalf, WriteHalf};
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
use tokio_util::codec::{Decoder, Encoder, Framed, FramedRead, FramedWrite};

use crate::protocol::Mode;
use crate::protocol::{cal_accept_key, wrap_tls};

pub trait WsConfig {
    fn check_rsv(&self) -> bool {
        true
    }

    fn mask_frame(&self) -> bool {
        true
    }
    fn validate_utf8(&self) -> bool {
        true
    }
    fn max_frame_payload_size(&self) -> usize {
        0
    }
    fn auto_fragment_size(&self) -> usize {
        0
    }
    fn open_handshake_timeout(&self) -> usize {
        0
    }
    fn close_handshake_timeout(&self) -> usize {
        0
    }
    fn tcp_no_delay(&self) -> bool {
        false
    }
    fn auto_ping_interval(&self) -> usize {
        0
    }
    fn auto_ping_timeout(&self) -> usize {
        0
    }

    fn auto_ping_payload(&self) -> Bytes {
        Bytes::new()
    }

    fn protocols(&self) -> HashSet<String> {
        HashSet::with_capacity(0)
    }

    fn extensions(&self) -> HashSet<String> {
        HashSet::with_capacity(0)
    }

    fn version(&self) -> u8 {
        13
    }

    fn payload_opcode(&self) -> OpCode {
        OpCode::Text
    }
}

impl WsConfig for () {}

pub struct WebSocketCodec<C>
where
    C: WsConfig + Unpin,
{
    pub config: C,
    pub state: ConnectionState,
    pub framed: Framed<WsStream, FrameCodec>,
}

pub struct WebSocketEncoder<C>
where
    C: WsConfig + Unpin,
{
    pub config: C,
    pub state: ConnectionState,
}

fn write_single_frame(payload: &[u8], fin: bool, mask: bool, code: OpCode, dst: &mut BytesMut) {
    let mut frame = Frame::new_with_opcode(code);
    frame.set_fin(fin);
    frame.set_mask(mask);
    frame.set_payload(payload);
    dst.extend_from_slice(&frame.0);
}

impl<'a, C: WsConfig + Unpin, T: Into<&'a [u8]>> Encoder<T> for WebSocketCodec<C> {
    type Error = WsError;

    fn encode(&mut self, item: T, dst: &mut BytesMut) -> Result<(), Self::Error> {
        if self.state != ConnectionState::Running {
            return Err(WsError::InvalidConnState(self.state.clone()));
        }
        let split_size = self.config.auto_fragment_size();
        let data: &[u8] = item.into();
        if split_size > 0 {
            let mut idx = 0;
            let mut fin = idx + split_size >= data.len();
            while (idx + split_size) <= data.len() {
                let end = data.len().min(idx + split_size);
                write_single_frame(
                    &data[idx..end],
                    fin,
                    self.config.mask_frame(),
                    self.config.payload_opcode(),
                    dst,
                );
                idx = end;
                fin = idx + split_size >= data.len();
            }
        } else {
            write_single_frame(
                data,
                true,
                self.config.mask_frame(),
                self.config.payload_opcode(),
                dst,
            )
        }
        Ok(())
    }
}

pub struct WebSocketDecoder<C>
where
    C: WsConfig + Unpin,
{
    pub config: C,
    pub state: ConnectionState,
    pub fragmented: bool,
    pub fragmented_data: BytesMut,
    pub fragmented_type: OpCode,
}

impl<C> WebSocketDecoder<C>
where
    C: WsConfig + Unpin,
{
    fn decode_single(&mut self, src: &mut BytesMut) -> Result<Option<BytesMut>, WsError> {
        if src.len() < 2 {
            return Ok(None);
        }
        // TODO check nonzero value according to extension negotiation
        let leading_bits = src[0] >> 4;
        if self.config.check_rsv() && !(leading_bits == 0b00001000 || leading_bits == 0b00000000) {
            return Err(WsError::ProtocolError {
                close_code: 1008,
                error: ProtocolError::InvalidLeadingBits(leading_bits),
            });
        }
        parse_opcode(src[0]).map_err(|e| WsError::ProtocolError {
            error: ProtocolError::InvalidOpcode(e),
            close_code: 1008,
        })?;
        let (payload_len, len_occ_bytes) =
            parse_payload_len(src.deref()).map_err(|e| WsError::ProtocolError {
                close_code: 1008,
                error: e,
            })?;
        let max_payload_size = self.config.max_frame_payload_size();
        if max_payload_size > 0 && payload_len > max_payload_size {
            return Err(WsError::ProtocolError {
                close_code: 1008,
                error: ProtocolError::PayloadTooLarge(max_payload_size),
            });
        }
        let mut expected_len = 1 + len_occ_bytes + payload_len;
        let mask = get_bit(&src, 1, 0);
        if mask {
            expected_len += 4;
        }
        if expected_len > src.len() {
            src.reserve(expected_len - src.len() + 1);
            Ok(None)
        } else {
            let mut data = BytesMut::with_capacity(expected_len);
            data.extend_from_slice(&src[..expected_len]);
            src.advance(expected_len);
            let mut ret = BytesMut::new();
            ret.extend_from_slice(&Frame(data).payload_data_unmask());
            Ok(Some(ret))
        }
    }
}

impl<C> Decoder for WebSocketDecoder<C>
where
    C: WsConfig + Unpin,
{
    type Item = BytesMut;
    type Error = WsError;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        todo!()
    }
}

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
    pub async fn handshake(&mut self) -> Result<http::Response<()>, WsError> {
        self.state = ConnectionState::HandShaking;
        let protocols = self.protocols.join(" ");
        let extensions = self.extensions.join(" ");
        let (key, resp) = perform_handshake(
            self.framed.get_mut(),
            &self.mode,
            &self.uri,
            protocols,
            extensions,
            13,
            Default::default(),
        )
        .await?;
        standard_handshake_resp_check(key.as_bytes(), &resp)?;
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
        let req = handle_handshake(stream).await?;
        if let Err(e) = standard_handshake_req_check(&req) {
            let resp = format!("HTTP/1.1 400 Bad Request\r\n\r\n{}", e.to_string());
            stream.write_all(resp.as_bytes()).await?;
            return Err(e);
        } else {
            let key = req.headers().get("sec-websocket-key").unwrap();
            let resp_lines = vec![
                "HTTP/1.1 101 Switching Protocols".to_string(),
                "upgrade: websocket".to_string(),
                "connection: upgrade".to_string(),
                format!("Sec-WebSocket-Accept: {}", cal_accept_key(key.as_bytes())),
                "\r\n".to_string(),
            ];
            stream.write_all(resp_lines.join("\r\n").as_bytes()).await?
        };
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
