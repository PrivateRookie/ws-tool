use std::collections::HashMap;
use std::fmt::Debug;

use bytes::BytesMut;
use sha1::Digest;

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

#[cfg(feature = "blocking")]
mod blocking {
    use std::{
        collections::HashMap,
        io::{Read, Write},
    };

    use bytes::{BufMut, BytesMut};

    use crate::{errors::WsError, stream::WsStream};

    use super::{handle_parse_handshake, perform_parse_req, prepare_handshake, Mode};

    #[cfg(feature = "tls_rustls")]
    mod tls {
        use std::{collections::HashSet, io::Read, net::TcpStream, path::PathBuf};

        use rustls_connector::{RustlsConnectorConfig, TlsStream};

        use crate::errors::WsError;

        pub fn wrap_tls(
            stream: TcpStream,
            host: &str,
            certs: &HashSet<PathBuf>,
        ) -> Result<TlsStream<TcpStream>, WsError> {
            let mut config = RustlsConnectorConfig::new_with_webpki_roots_certs();
            let mut cert_data = vec![];
            for cert_path in certs {
                let mut pem = std::fs::File::open(cert_path).map_err(|_| {
                    WsError::CertFileNotFound(cert_path.to_str().unwrap_or_default().to_string())
                })?;
                let mut data = vec![];
                if let Err(e) = pem.read_to_end(&mut data) {
                    tracing::error!(
                        "failed to read cert file {} {}",
                        cert_path.display(),
                        e.to_string()
                    );
                    continue;
                }
                cert_data.push(data);
            }
            config.add_parsable_certificates(&cert_data);
            let connector = config.connector_with_no_client_auth();
            let tls_stream = connector
                .connect(host, stream)
                .map_err(|e| WsError::ConnectionFailed(e.to_string()))?;
            tracing::debug!("tls connection established");
            Ok(tls_stream)
        }
    }

    #[cfg(feature = "tls_rustls")]
    pub use tls::wrap_tls;

    /// perform http upgrade
    ///
    /// **NOTE**: low level api
    pub fn req_handshake(
        stream: &mut WsStream,
        mode: &Mode,
        uri: &http::Uri,
        protocols: String,
        extensions: String,
        version: u8,
        extra_headers: HashMap<String, String>,
    ) -> Result<(String, http::Response<()>), WsError> {
        let (key, req_str) =
            prepare_handshake(protocols, extensions, extra_headers, uri, mode, version);
        stream.write_all(req_str.as_bytes())?;
        let mut read_bytes = BytesMut::with_capacity(1024);
        let mut buf: [u8; 1] = [0; 1];
        loop {
            let num = stream.read(&mut buf)?;
            read_bytes.extend_from_slice(&buf[..num]);
            let header_complete = read_bytes.ends_with(&[b'\r', b'\n', b'\r', b'\n']);
            if header_complete || num == 0 {
                break;
            }
        }
        perform_parse_req(read_bytes, key)
    }

    pub fn handle_handshake(stream: &mut WsStream) -> Result<http::Request<()>, WsError> {
        let mut req_bytes = BytesMut::with_capacity(1024);
        let mut buf = [0u8];
        loop {
            stream.read_exact(&mut buf)?;
            req_bytes.put_u8(buf[0]);
            if req_bytes.ends_with(&[b'\r', b'\n', b'\r', b'\n']) {
                break;
            }
        }
        handle_parse_handshake(req_bytes)
    }
}

#[cfg(feature = "blocking")]
pub use blocking::*;

#[cfg(feature = "async")]
mod non_blocking {
    use std::collections::HashMap;

    use bytes::{BufMut, BytesMut};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use crate::{errors::WsError, protocol::prepare_handshake, stream::WsAsyncStream};

    use super::{handle_parse_handshake, perform_parse_req, Mode};

    #[cfg(feature = "async_tls_rustls")]
    mod tls {
        use std::io::BufReader;
        use std::{collections::HashSet, path::PathBuf};
        // use std::path::PathBuf;
        use crate::errors::WsError;
        use std::sync::Arc;
        use tokio::net::TcpStream;
        use tokio_rustls::{client::TlsStream, rustls::ClientConfig, TlsConnector};
        use webpki::DNSNameRef;

        pub async fn async_wrap_tls(
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
            let domain = DNSNameRef::try_from_ascii_str(host)
                .map_err(|e| WsError::TlsDnsFailed(e.to_string()))?;
            let connector = TlsConnector::from(Arc::new(config));
            let tls_stream = connector
                .connect(domain, stream)
                .await
                .map_err(|e| WsError::ConnectionFailed(e.to_string()))?;
            tracing::debug!("tls connection established");
            Ok(tls_stream)
        }
    }

    #[cfg(feature = "async_tls_rustls")]
    pub use tls::async_wrap_tls;

    /// perform http upgrade
    ///
    /// **NOTE**: low level api
    pub async fn async_req_handshake(
        stream: &mut WsAsyncStream,
        mode: &Mode,
        uri: &http::Uri,
        protocols: String,
        extensions: String,
        version: u8,
        extra_headers: HashMap<String, String>,
    ) -> Result<(String, http::Response<()>), WsError> {
        let (key, req_str) =
            prepare_handshake(protocols, extensions, extra_headers, uri, mode, version);
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
        perform_parse_req(read_bytes, key)
    }

    pub async fn async_handle_handshake(
        stream: &mut WsAsyncStream,
    ) -> Result<http::Request<()>, WsError> {
        let mut req_bytes = BytesMut::with_capacity(1024);
        let mut buf = [0u8];
        loop {
            stream.read_exact(&mut buf).await?;
            req_bytes.put_u8(buf[0]);
            if req_bytes.ends_with(&[b'\r', b'\n', b'\r', b'\n']) {
                break;
            }
        }
        handle_parse_handshake(req_bytes)
    }
}

#[cfg(feature = "async")]
pub use non_blocking::*;

/// generate random key
pub fn gen_key() -> String {
    let r: [u8; 16] = rand::random();
    base64::encode(&r)
}

/// cal accept key
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

pub fn standard_handshake_resp_check(key: &[u8], resp: &http::Response<()>) -> Result<(), WsError> {
    tracing::debug!("{:?}", resp);
    if resp.status() != http::StatusCode::SWITCHING_PROTOCOLS {
        return Err(WsError::HandShakeFailed(format!(
            "expect 101 response, got {}",
            resp.status()
        )));
    }
    let expect_key = cal_accept_key(key);
    if let Some(accept_key) = resp.headers().get("sec-websocket-accept") {
        if accept_key.to_str().unwrap_or_default() != expect_key {
            return Err(WsError::HandShakeFailed("mismatch key".to_string()));
        }
    } else {
        return Err(WsError::HandShakeFailed(
            "missing `sec-websocket-accept` header".to_string(),
        ));
    }
    Ok(())
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

fn prepare_handshake(
    protocols: String,
    extensions: String,
    extra_headers: HashMap<String, String>,
    uri: &http::Uri,
    mode: &Mode,
    version: u8,
) -> (String, String) {
    let key = gen_key();
    let mut headers = vec![
        format!(
            "Host: {}:{}",
            uri.host().unwrap_or_default(),
            uri.port_u16().unwrap_or_else(|| mode.default_port())
        ),
        "Upgrade: websocket".to_string(),
        "Connection: Upgrade".to_string(),
        format!("Sec-Websocket-Key: {}", key),
        format!("Sec-WebSocket-Version: {}", version.to_string()),
    ];
    if !protocols.is_empty() {
        headers.push(format!("Sec-WebSocket-Protocol: {}", protocols))
    }
    if !extensions.is_empty() {
        headers.push(format!("Sec-WebSocket-Extensions: {}", extensions))
    }
    for (k, v) in extra_headers.iter() {
        headers.push(format!("{}: {}", k, v));
    }
    let req_str = format!(
        "{method} {path} {version:?}\r\n{headers}\r\n\r\n",
        method = http::Method::GET,
        path = uri
            .path_and_query()
            .map(|full_path| full_path.to_string())
            .unwrap_or_default(),
        version = http::Version::HTTP_11,
        headers = headers.join("\r\n")
    );
    tracing::debug!("handshake request\n{}", req_str);
    (key, req_str)
}

fn perform_parse_req(
    read_bytes: BytesMut,
    key: String,
) -> Result<(String, http::Response<()>), WsError> {
    let mut headers = [httparse::EMPTY_HEADER; 64];
    let mut resp = httparse::Response::new(&mut headers);
    let _parse_status = resp
        .parse(&read_bytes)
        .map_err(|_| WsError::HandShakeFailed("invalid response".to_string()))?;
    let mut resp_builder = http::Response::builder()
        .status(resp.code.unwrap_or_default())
        .version(match resp.version.unwrap_or_else(|| 1) {
            0 => http::Version::HTTP_10,
            1 => http::Version::HTTP_11,
            v => {
                tracing::warn!("unknown http 1.{} version", v);
                http::Version::HTTP_11
            }
        });
    for header in resp.headers.iter() {
        resp_builder = resp_builder.header(header.name, header.value);
    }
    tracing::debug!("protocol handshake complete");
    Ok((key, resp_builder.body(()).unwrap()))
}

fn handle_parse_handshake(req_bytes: BytesMut) -> Result<http::Request<()>, WsError> {
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
