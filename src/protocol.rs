use std::collections::{HashMap, HashSet};
use std::io::BufReader;
use std::path::PathBuf;
use std::{fmt::Debug, sync::Arc};

use crate::stream::WsStream;
use bytes::{BufMut, BytesMut};
use sha1::Digest;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use tokio_rustls::{client::TlsStream, rustls::ClientConfig, TlsConnector};
use webpki::DNSNameRef;

use crate::errors::WsError;

const GUID: &[u8] = b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

/// close status code to indicate reason for closure
#[derive(Debug, Clone)]
pub enum StatusCode {
    /// 1000 indicates a normal closure, meaning that the purpose for
    /// which the connection was established has been fulfilled.
    C1000,

    /// 1001 indicates that an endpoint is "going away", such as a server
    /// going down or a browser having navigated away from a page.
    C1001,

    /// 1002 indicates that an endpoint is terminating the connection due
    /// to a protocol error.
    C1002,

    /// 1003 indicates that an endpoint is terminating the connection
    /// because it has received a type of data it cannot accept (e.g., an
    /// endpoint that understands only text data MAY send this if it
    /// receives a binary message).
    C1003,

    /// Reserved.  The specific meaning might be defined in the future.
    C1004,

    /// 1005 is a reserved value and MUST NOT be set as a status code in a
    /// Close control frame by an endpoint.  It is designated for use in
    /// applications expecting a status code to indicate that no status
    /// code was actually present.
    C1005,

    /// 1006 is a reserved value and MUST NOT be set as a status code in a
    /// Close control frame by an endpoint.  It is designated for use in
    /// applications expecting a status code to indicate that the
    /// connection was closed abnormally, e.g., without sending or
    /// receiving a Close control frame.
    C1006,

    /// 1007 indicates that an endpoint is terminating the connection
    /// because it has received data within a message that was not
    /// consistent with the type of the message (e.g., non-UTF-8 \[RFC3629\]
    /// data within a text message).
    C1007,

    /// 1008 indicates that an endpoint is terminating the connection
    /// because it has received a message that violates its policy.  This
    /// is a generic status code that can be returned when there is no
    /// other more suitable status code (e.g., 1003 or 1009) or if there
    /// is a need to hide specific details about the policy.
    C1008,

    /// 1009 indicates that an endpoint is terminating the connection
    /// because it has received a message that is too big for it to
    /// process.
    C1009,

    /// 1010 indicates that an endpoint (client) is terminating the
    /// connection because it has expected the server to negotiate one or
    /// more extension, but the server didn't return them in the response
    /// message of the WebSocket handshake.  The list of extensions that
    /// are needed SHOULD appear in the /reason/ part of the Close frame.
    /// Note that this status code is not used by the server, because it
    /// can fail the WebSocket handshake instead.
    C1010,

    /// 1011 indicates that a server is terminating the connection because
    /// it encountered an
    C1011,

    /// 1015 is a reserved value and MUST NOT be set as a status code in a
    /// Close control frame by an endpoint.  It is designated for use in
    /// applications expecting a status code to indicate that the
    /// connection was closed due to a failure to perform a TLS handshake
    /// (e.g., the server certificate can't be verified).
    C1015,

    /// Status codes in the range 0-999 are not used.
    C0_999,

    // Status codes in the range 1000-2999 are reserved for definition by
    // this protocol, its future revisions, and extensions specified in a
    // permanent and readily available public specification.
    C1000_2999,

    /// Status codes in the range 3000-3999 are reserved for use by
    /// libraries, frameworks, and applications.  These status codes are
    /// registered directly with IANA.  The interpretation of these codes
    /// is undefined by this protocol.
    C3000_3999,

    /// Status codes in the range 4000-4999 are reserved for private use
    /// and thus can't be registered.  Such codes can be used by prior
    /// agreements between WebSocket applications.  The interpretation of
    /// these codes is undefined by this protocol.
    C4000_4999,

    Unknown,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Mode {
    WS,
    WSS,
}

impl Mode {
    pub fn default_port(&self) -> u16 {
        match self {
            Mode::WS => 80,
            Mode::WSS => 443,
        }
    }
}

pub(crate) async fn wrap_tls(
    stream: TcpStream,
    host: &str,
    certs: &HashSet<PathBuf>,
) -> Result<TlsStream<TcpStream>, WsError> {
    let mut config = ClientConfig::new();
    for cert_path in certs {
        let mut pem = std::fs::File::open(cert_path).map_err(|_| {
            WsError::CertFileNotFound(cert_path.to_str().unwrap_or_default().to_string())
        })?;
        let mut cert = BufReader::new(&mut pem);
        config.root_store.add_pem_file(&mut cert).map_err(|_| {
            WsError::CertFileNotFound(cert_path.to_str().unwrap_or_default().to_string())
        })?;
    }
    config
        .root_store
        .add_server_trust_anchors(&webpki_roots::TLS_SERVER_ROOTS);
    let domain =
        DNSNameRef::try_from_ascii_str(host).map_err(|e| WsError::TlsDnsFailed(e.to_string()))?;
    let connector = TlsConnector::from(Arc::new(config));
    let tls_stream = connector
        .connect(domain, stream)
        .await
        .map_err(|e| WsError::ConnectionFailed(e.to_string()))?;
    tracing::debug!("tls connection established");
    Ok(tls_stream)
}

pub fn gen_key() -> String {
    let r: [u8; 16] = rand::random();
    base64::encode(&r)
}

pub fn cal_accept_key(source: &[u8]) -> String {
    let mut sha1 = sha1::Sha1::default();
    sha1.update(source);
    sha1.update(GUID);
    base64::encode(&sha1.finalize())
}

#[derive(Debug)]
pub struct HandshakeResponse {
    pub code: u8,
    pub reason: String,
    pub headers: HashMap<String, String>,
}

/// perform http upgrade
///
/// **NOTE**: low level api
pub async fn perform_handshake(
    stream: &mut WsStream,
    mode: &Mode,
    uri: &http::Uri,
    protocols: String,
    extensions: String,
    version: u8,
) -> Result<HandshakeResponse, WsError> {
    let key = gen_key();
    let accept_key = cal_accept_key(&key.as_bytes());

    let mut req_builder = http::Request::builder()
        .uri(uri)
        .header(
            "Host",
            format!(
                "{}:{}",
                uri.host().unwrap_or_default(),
                uri.port_u16().unwrap_or_else(|| mode.default_port())
            ),
        )
        .header("Upgrade", "websocket")
        .header("Connection", "Upgrade")
        .header("Sec-Websocket-Key", &key)
        .header("Sec-WebSocket-Version", version.to_string());

    req_builder = if protocols.is_empty() {
        req_builder
    } else {
        req_builder.header("Sec-WebSocket-Protocol", protocols)
    };

    req_builder = if extensions.is_empty() {
        req_builder
    } else {
        req_builder.header("Sec-WebSocket-Extensions", extensions)
    };
    let req = req_builder.body(()).unwrap();
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
        path = uri
            .path_and_query()
            .map(|full_path| full_path.to_string())
            .unwrap_or_default(),
        version = http::Version::HTTP_11,
        headers = headers
    );
    stream.write_all(req_str.as_bytes()).await?;
    let mut read_bytes = BytesMut::with_capacity(1024);
    let mut buf: [u8; 1] = [0; 1];
    loop {
        let num = stream.read(&mut buf).await?;
        read_bytes.extend_from_slice(&buf[..num]);
        let header_complete = read_bytes.ends_with(&[b'\r', b'\n', b'\r', b'\n']);
        if header_complete || num == 0 {
            break;
        }
    }
    let mut headers = [httparse::EMPTY_HEADER; 64];
    let mut resp = httparse::Response::new(&mut headers);
    let _parse_status = resp
        .parse(&read_bytes)
        .map_err(|_| WsError::HandShakeFailed("invalid response".to_string()))?;
    if resp.code.unwrap_or_default() != 101 {
        return Err(WsError::HandShakeFailed(format!(
            "expect 101 response, got {:?} {:?}",
            resp.code, resp.reason
        )));
    }
    for header in resp.headers.iter() {
        if header.name.to_lowercase() == "sec-websocket-accept"
            && header.value != accept_key.as_bytes()
        {
            return Err(WsError::HandShakeFailed(format!(
                "mismatch key, expect {:?}, got {:?}",
                accept_key.as_bytes(),
                header.value
            )));
        }
    }
    let mut handshake_resp = HandshakeResponse {
        code: 101,
        reason: resp.reason.map(|r| r.to_string()).unwrap_or_default(),
        headers: HashMap::new(),
    };
    resp.headers.iter().for_each(|header| {
        handshake_resp.headers.insert(
            header.name.to_string(),
            String::from_utf8_lossy(header.value).to_string(),
        );
    });
    tracing::debug!("protocol handshake complete");
    Ok(handshake_resp)
}

pub async fn handle_handshake(stream: &mut WsStream) -> Result<http::Request<()>, WsError> {
    let mut req_bytes = BytesMut::with_capacity(1024);
    let mut buf = [0u8];
    loop {
        stream.read_exact(&mut buf).await?;
        req_bytes.put_u8(buf[0]);
        if req_bytes.ends_with(&[b'\r', b'\n', b'\r', b'\n']) {
            break;
        }
    }
    let mut headers = [httparse::EMPTY_HEADER; 64];
    let mut req = httparse::Request::new(&mut headers);
    let _parse_status = req
        .parse(&req_bytes)
        .map_err(|_| WsError::HandShakeFailed("invalid request".to_string()))?;
    let mut req_builder = http::Request::builder()
        .method(req.method.unwrap_or_default())
        .uri(req.path.unwrap_or_default())
        .version(match req.version.unwrap_or_else(|| 1) {
            0 => http::Version::HTTP_10,
            1 => http::Version::HTTP_11,
            v => {
                tracing::warn!("unknown http 1.{} version", v);
                http::Version::HTTP_11
            }
        });
    for header in req.headers.iter() {
        req_builder = req_builder.header(header.name, header.value);
    }
    Ok(req_builder.body(()).unwrap())
}

/// perform rfc standard check
pub fn standard_handshake_req_check(req: &http::Request<()>) -> Result<(), WsError> {
    if let Some(val) = req.headers().get("upgrade") {
        if val != "websocket" {
            return Err(WsError::HandShakeFailed(format!(
                "expect `websocket`, got {:?}",
                val
            )));
        }
    } else {
        return Err(WsError::HandShakeFailed(
            "missing `upgrade` header".to_string(),
        ));
    }

    if let Some(val) = req.headers().get("sec-websocket-key") {
        if val.is_empty() {
            return Err(WsError::HandShakeFailed(
                "empty sec-websocket-key".to_string(),
            ));
        }
    } else {
        return Err(WsError::HandShakeFailed(
            "missing `sec-websocket-key` header".to_string(),
        ));
    }
    Ok(())
}
