use std::{fmt::Debug, sync::Arc};

use bytes::{Bytes, BytesMut};
use tokio::net::TcpStream;

use thiserror::Error;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio_rustls::{client::TlsStream, rustls::ClientConfig, TlsConnector};
use webpki::DNSNameRef;

const DEFAULT_FRAME: [u8; 14] = [0b10000001, 0b10000000, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];

#[derive(Debug, Error)]
pub enum WsError {
    #[error("invalid uri `{0}`")]
    InvalidUri(String),
    #[error("connection failed `{0}`")]
    ConnectionFailed(String),
    #[error("tls dns lookup failed `{0}`")]
    TlsDnsFailed(String),
    #[error("protocol error `{0}`")]
    ProtocolError(String),
}

#[derive(Debug, Error)]
pub enum ProtocolError {
    #[error("insufficient data len {0}")]
    InsufficientLen(usize),
}

#[derive(Debug)]
enum Mode {
    WS,
    WSS,
}

impl Mode {
    fn default_port(&self) -> u16 {
        match self {
            Mode::WS => 80,
            Mode::WSS => 443,
        }
    }
}

pub enum WsStream {
    Plain(TcpStream),
    Tls(TlsStream<TcpStream>),
}

impl AsyncRead for WsStream {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match self.get_mut() {
            WsStream::Plain(stream) => std::pin::Pin::new(stream).poll_read(cx, buf),
            WsStream::Tls(stream) => std::pin::Pin::new(stream).poll_read(cx, buf),
        }
    }
}

impl AsyncWrite for WsStream {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<Result<usize, std::io::Error>> {
        match self.get_mut() {
            WsStream::Plain(stream) => std::pin::Pin::new(stream).poll_write(cx, buf),
            WsStream::Tls(stream) => std::pin::Pin::new(stream).poll_write(cx, buf),
        }
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), std::io::Error>> {
        match self.get_mut() {
            WsStream::Plain(stream) => std::pin::Pin::new(stream).poll_flush(cx),
            WsStream::Tls(stream) => std::pin::Pin::new(stream).poll_flush(cx),
        }
    }

    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), std::io::Error>> {
        match self.get_mut() {
            WsStream::Plain(stream) => std::pin::Pin::new(stream).poll_shutdown(cx),
            WsStream::Tls(stream) => std::pin::Pin::new(stream).poll_shutdown(cx),
        }
    }
}

/// Defines the interpretation of the "Payload data".  If an unknown
/// opcode is received, the receiving endpoint MUST _Fail the
/// WebSocket Connection_.  The following values are defined.
/// - x0 denotes a continuation frame
/// - x1 denotes a text frame
/// - x2 denotes a binary frame
/// - x3-7 are reserved for further non-control frames
/// - x8 denotes a connection close
/// - x9 denotes a ping
/// - xA denotes a pong
/// - xB-F are reserved for further control frames
#[derive(Debug, Clone)]
pub enum OpCode {
    Continue,
    Text,
    Binary,
    ReservedNonControl,
    Close,
    Ping,
    Pong,
    ReservedControl,
}

impl OpCode {
    pub fn as_u8(&self) -> u8 {
        match self {
            OpCode::Continue => 0,
            OpCode::Text => 1,
            OpCode::Binary => 2,
            OpCode::ReservedNonControl => 3,
            OpCode::Close => 8,
            OpCode::Ping => 9,
            OpCode::Pong => 10,
            OpCode::ReservedControl => 11,
        }
    }
}

/// websocket data frame
#[derive(Clone)]
pub struct Frame {
    raw: BytesMut,
}

impl Frame {
    pub fn from_bytes_uncheck(source: &[u8]) -> Self {
        let raw = BytesMut::from(source);
        Self { raw }
    }

    pub fn from_bytes(source: &[u8]) -> Result<Self, ProtocolError> {
        if source.len() < 2 {
            return Err(ProtocolError::InsufficientLen(source.len()));
        }

        todo!()
    }

    #[inline]
    fn get_bit(&self, byte_index: usize, bit_index: usize) -> bool {
        let b = self.raw[byte_index];
        return 1 & (b >> (7 - bit_index)) != 0;
    }

    #[inline]
    fn set_bit(&mut self, byte_index: usize, bit_index: usize, val: bool) {
        let b = self.raw[byte_index];
        let op = if val {
            1 << (7 - bit_index)
        } else {
            u8::MAX - (1 << (7 - bit_index))
        };
        self.raw[byte_index] = b | op
    }

    #[inline]
    pub fn fin(&self) -> bool {
        self.get_bit(0, 0)
    }

    #[inline]
    pub fn set_fin(&mut self, val: bool) {
        self.set_bit(0, 0, val)
    }

    #[inline]
    pub fn rsv1(&self) -> bool {
        self.get_bit(0, 1)
    }

    #[inline]
    pub fn set_rsv1(&mut self, val: bool) {
        self.set_bit(0, 1, val)
    }

    #[inline]
    pub fn rsv2(&self) -> bool {
        self.get_bit(0, 2)
    }

    #[inline]
    pub fn set_rsv2(&mut self, val: bool) {
        self.set_bit(0, 2, val)
    }

    #[inline]
    pub fn rsv3(&self) -> bool {
        self.get_bit(0, 3)
    }

    #[inline]
    pub fn set_rsv3(&mut self, val: bool) {
        self.set_bit(0, 3, val)
    }

    pub fn opcode(&self) -> OpCode {
        let mut val = self.raw[0];
        val = (val << 4) >> 4;
        match val {
            0 => OpCode::Continue,
            1 => OpCode::Text,
            2 => OpCode::Binary,
            3..=7 => OpCode::ReservedNonControl,
            8 => OpCode::Close,
            9 => OpCode::Ping,
            10 => OpCode::Pong,
            11..=15 => OpCode::ReservedControl,
            _ => unreachable!(format!("unexpected opcode {}", val)),
        }
    }

    fn set_opcode(&mut self, code: OpCode) {
        let leading_bits = self.raw[0] | 0b11110000;
        self.raw[0] = leading_bits | code.as_u8()
    }

    #[inline]
    pub fn mask(&self) -> bool {
        self.get_bit(1, 0)
    }

    #[inline]
    fn payload_len_with_occ(&self) -> (usize, u64) {
        let mut len = self.raw[1];
        len = (len << 1) >> 1;
        match len {
            0..=125 => (1, len as u64),
            126 => {
                let mut arr = [0u8; 2];
                arr[0] = self.raw[2];
                arr[1] = self.raw[3];
                (1 + 2, u16::from_be_bytes(arr) as u64)
            }
            127 => {
                let mut arr = [0u8; 8];
                for idx in 0..8 {
                    arr[idx] = self.raw[idx + 2];
                }
                (1 + 8, u64::from_be_bytes(arr))
            }
            _ => unreachable!(),
        }
    }

    pub fn payload_len(&self) -> u64 {
        self.payload_len_with_occ().1
    }

    fn set_payload_len(&mut self, len: u64) -> usize {
        let mut leading_byte = self.raw[1];
        match len {
            0..=125 => {
                leading_byte &= 128;
                self.raw[1] = leading_byte | (len as u8);
                1
            }
            126..=65535 => {
                leading_byte &= 128;
                self.raw[1] = leading_byte | 126;
                let len_arr = (len as u16).to_be_bytes();
                self.raw[2] = len_arr[0];
                self.raw[3] = len_arr[1];
                3
            }
            _ => {
                leading_byte &= 128;
                self.raw[1] = leading_byte | 127;
                let len_arr = (len as u64).to_be_bytes();
                for idx in 0..8 {
                    self.raw[idx + 2] = len_arr[idx];
                }
                9
            }
        }
    }

    pub fn masking_key(&self) -> Option<[u8; 4]> {
        if self.mask() {
            let len_occupied = self.payload_len_with_occ().0;
            let mut arr = [0u8; 4];
            for idx in 0..4 {
                arr[idx] = self.raw[1 + len_occupied + idx];
            }
            Some(arr)
        } else {
            None
        }
    }

    pub fn set_masking_key(&mut self) -> Option<[u8; 4]> {
        if self.mask() {
            let masking_key: [u8; 4] = rand::random();
            let (len_occupied, _) = self.payload_len_with_occ();
            self.raw[(1 + len_occupied)..(5 + len_occupied)].copy_from_slice(&masking_key);
            Some(masking_key)
        } else {
            None
        }
    }

    /// return unmask(if masked) payload data
    pub fn payload_data_unmask(&self) -> Bytes {
        match self.masking_key() {
            Some(masking_key) => {
                let slice = self
                    .payload_data()
                    .iter()
                    .enumerate()
                    .map(|(idx, num)| num ^ masking_key[idx % 4])
                    .collect::<Vec<u8>>();
                Bytes::copy_from_slice(&slice)
            }
            None => Bytes::copy_from_slice(self.payload_data()),
        }
    }

    pub fn payload_data(&self) -> &[u8] {
        let mut start_idx = 1;
        let (len_occupied, len) = self.payload_len_with_occ();
        start_idx += len_occupied;
        if self.mask() {
            start_idx += 4;
        }
        &self.raw[start_idx..start_idx + (len as usize)]
    }
}

/// helper construct methods
impl Frame {
    pub fn new() -> Self {
        let mut raw = BytesMut::with_capacity(200);
        raw.extend_from_slice(&DEFAULT_FRAME);
        Self { raw }
    }

    // TODO should init with const array to avoid computing?
    pub fn new_with_opcode(opcode: OpCode) -> Self {
        let mut frame = Frame::new();
        frame.set_opcode(opcode);
        frame
    }

    pub fn set_payload(&mut self, payload: &[u8]) {
        let len = payload.len();
        let offset = self.set_payload_len(len as u64);
        let mask = self.mask();
        let mut start_idx = 1 + offset;
        let mut end_idx = 1 + offset + len;
        if mask {
            let masking_key = self.set_masking_key().unwrap();
            start_idx += 4;
            end_idx += 4;
            self.raw.resize(end_idx, 0x0);
            let data = payload
                .iter()
                .enumerate()
                .map(|(idx, v)| v ^ masking_key[idx % 4])
                .collect::<Vec<u8>>();
            self.raw[start_idx..end_idx].copy_from_slice(&data);
        } else {
            self.raw.resize(end_idx, 0x0);
            self.raw[start_idx..end_idx].copy_from_slice(payload)
        }
    }

    pub fn get_bytes(&self) -> &[u8] {
        let (occ, len) = self.payload_len_with_occ();
        let mut end = 1 + occ + len as usize;
        if self.mask() {
            end += 4
        }
        &self.raw[..end]
    }
}

impl Debug for Frame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Websocket Frame")?;
        writeln!(f, "============================================")?;
        writeln!(
            f,
            "fin {}, rsv1 {}, rsv2 {} , rsv3 {}",
            self.fin(),
            self.rsv1(),
            self.rsv2(),
            self.rsv3()
        )?;
        writeln!(
            f,
            "opcode {:?} mask {} payload_len {}",
            self.opcode(),
            self.mask(),
            self.payload_len()
        )?;
        if self.mask() {
            writeln!(f, "masking key {:x?}", self.masking_key().unwrap())?;
        }
        writeln!(f, "============================================")?;
        let data = self.payload_data_unmask();
        writeln!(
            f,
            "payload data \n {:?}",
            data.iter().map(|c| *c as char).collect::<String>()
        )?;
        writeln!(f, "============================================")?;
        Ok(())
    }
}

/// create connection
pub async fn connect(uri: &str) -> Result<WsStream, WsError> {
    let uri = uri
        .parse::<http::Uri>()
        .map_err(|e| WsError::InvalidUri(format!("{} {}", uri, e.to_string())))?;
    let host = uri
        .host()
        .ok_or(WsError::InvalidUri(format!("can not find host {}", uri)))?;
    let mode = if let Some(schema) = uri.scheme_str() {
        match schema.to_ascii_lowercase().as_str() {
            "ws" => Ok(Mode::WS),
            "wss" => Ok(Mode::WSS),
            _ => Err(WsError::InvalidUri(format!("invalid schema {}", schema))),
        }
    } else {
        Err(WsError::InvalidUri(format!("missing ws or wss schema")))
    }?;
    let port = match uri.port_u16() {
        Some(port) => port,
        None => mode.default_port(),
    };
    let stream = TcpStream::connect(format!("{}:{}", host, port))
        .await
        .map_err(|e| {
            WsError::ConnectionFailed(format!("failed to create tcp connection {}", e.to_string()))
        })?;
    log::debug!("tcp connection established");
    let mut stream = match mode {
        Mode::WS => WsStream::Plain(stream),
        Mode::WSS => {
            let tls_stream = wrap_tls(stream, host).await?;
            WsStream::Tls(tls_stream)
        }
    };
    handshake(&mut stream, &uri).await.unwrap();

    Ok(stream)
}

async fn wrap_tls(stream: TcpStream, host: &str) -> Result<TlsStream<TcpStream>, WsError> {
    let mut config = ClientConfig::new();
    config
        .root_store
        .add_server_trust_anchors(&webpki_roots::TLS_SERVER_ROOTS);
    let domain =
        DNSNameRef::try_from_ascii_str(host).map_err(|e| WsError::TlsDnsFailed(e.to_string()))?;
    let connector = TlsConnector::from(Arc::new(config));
    let tls_stream = connector.connect(domain, stream).await.map_err(|e| {
        dbg!(&e);
        WsError::ConnectionFailed(e.to_string())
    })?;
    log::debug!("tls connection established");
    Ok(tls_stream)
}

async fn handshake(stream: &mut WsStream, uri: &http::Uri) -> Result<(), WsError> {
    let req = http::Request::builder()
        .uri(uri)
        .header(
            "Host",
            format!(
                "{}:{}",
                uri.host().unwrap_or_default(),
                uri.port_u16().unwrap_or(80)
            ),
        )
        .header("Upgrade", "websocket")
        .header("Connection", "Upgrade")
        .header("Sec-Websocket-Key", &gen_key())
        .header("Sec-WebSocket-Protocol", "")
        .header("Sec-WebSocket-Version", "13")
        .body(())
        .unwrap();
    let headers = req
        .headers()
        .iter()
        .map(|(k, v)| format!("{}: {}", k, v.to_str().unwrap_or_default()))
        .collect::<Vec<String>>()
        .join("\r\n");
    let method = http::Method::GET;
    let req_str = format!(
        "{method} {path} {version:?}\r\n{headers}\r\n\r\n",
        method = method,
        path = uri.path(),
        version = http::Version::HTTP_11,
        headers = headers
    );
    stream.write_all(req_str.as_bytes()).await.unwrap();
    let mut resp = [0; 4096];
    stream.read(&mut resp).await.unwrap();
    Ok(())
}

fn gen_key() -> String {
    let r: [u8; 16] = rand::random();
    base64::encode(&r)
}

// #[test]
// fn test_frame() {
//     let source = include_bytes!("../tests/ws_nihao.bin");
//     let frame = Frame::from_bytes(source);
//     dbg!(frame);
// }

#[test]
fn index() {
    let mut buf = BytesMut::new();
    buf.extend_from_slice(&[1, 2, 3, 4, 5]);
    println!("{}, {}", buf[0], buf[6]);
}
