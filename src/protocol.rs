use http;
use bytes::BytesMut;
use sha1::Digest;
use std::collections::HashMap;
use std::fmt::Debug;

use crate::errors::WsError;

const GUID: &[u8] = b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

/// helper struct for using close code
pub struct StatusCode;

impl StatusCode {
    /// 1000 indicates a normal closure, meaning that the purpose for
    /// which the connection was established has been fulfilled.
    pub fn normal() -> u16 {
        1000
    }

    /// 1001 indicates that an endpoint is "going away", such as a server
    /// going down or a browser having navigated away from a page.
    pub fn going_away() -> u16 {
        1001
    }

    /// 1002 indicates that an endpoint is terminating the connection due
    /// to a protocol error.
    pub fn protocol_error() -> u16 {
        1002
    }

    /// 1003 indicates that an endpoint is terminating the connection
    /// because it has received a type of data it cannot accept (e.g., an
    /// endpoint that understands only text data MAY send this if it
    /// receives a binary message).
    pub fn terminate() -> u16 {
        1003
    }
    /// Reserved.  The specific meaning might be defined in the future.
    pub fn reserved() -> u16 {
        1004
    }

    /// 1005 is a reserved value and MUST NOT be set as a status code in a
    /// Close control frame by an endpoint.  It is designated for use in
    /// applications expecting a status code to indicate that no status
    /// code was actually present.
    pub fn app_reserved() -> u16 {
        1005
    }

    /// 1006 is a reserved value and MUST NOT be set as a status code in a
    /// Close control frame by an endpoint.  It is designated for use in
    /// applications expecting a status code to indicate that the
    /// connection was closed abnormally, e.g., without sending or
    /// receiving a Close control frame.
    pub fn abnormal_reserved() -> u16 {
        1006
    }

    /// 1007 indicates that an endpoint is terminating the connection
    /// because it has received data within a message that was not
    /// consistent with the type of the message (e.g., non-UTF-8 \[RFC3629\]
    /// data within a text message).
    pub fn non_consistent() -> u16 {
        1007
    }

    /// 1008 indicates that an endpoint is terminating the connection
    /// because it has received a message that violates its policy.  This
    /// is a generic status code that can be returned when there is no
    /// other more suitable status code (e.g., 1003 or 1009) or if there
    /// is a need to hide specific details about the policy.
    pub fn violate_policy() -> u16 {
        1008
    }

    /// 1009 indicates that an endpoint is terminating the connection
    /// because it has received a message that is too big for it to
    /// process.
    pub fn too_big() -> u16 {
        1009
    }

    /// 1010 indicates that an endpoint (client) is terminating the
    /// connection because it has expected the server to negotiate one or
    /// more extension, but the server didn't return them in the response
    /// message of the WebSocket handshake.  The list of extensions that
    /// are needed SHOULD appear in the /reason/ part of the Close frame.
    /// Note that this status code is not used by the server, because it
    /// can fail the WebSocket handshake instead.
    pub fn require_ext() -> u16 {
        1010
    }

    /// 1011 indicates that a server is terminating the connection because
    /// it encountered an unexpected condition that prevented it from
    /// fulfilling the request.
    pub fn unexpected_condition() -> u16 {
        1011
    }

    /// 1015 is a reserved value and MUST NOT be set as a status code in a
    /// Close control frame by an endpoint.  It is designated for use in
    /// applications expecting a status code to indicate that the
    /// connection was closed due to a failure to perform a TLS handshake
    /// (e.g., the server certificate can't be verified).
    pub fn platform_fail() -> u16 {
        1015
    }
}

/// websocket connection mode
#[derive(Debug, PartialEq, Eq)]
pub enum Mode {
    /// plain mode `ws://great.nice`
    WS,
    /// tls mode `wss://secret.wow`
    WSS,
}

impl Mode {
    /// return corresponding port of websocket mode
    pub fn default_port(&self) -> u16 {
        match self {
            Mode::WS => 80,
            Mode::WSS => 443,
        }
    }
}

#[cfg(feature = "sync")]
mod blocking {
    use http;
    use std::{
        collections::HashMap,
        io::{Read, Write},
    };

    use bytes::{BufMut, BytesMut};

    use crate::errors::WsError;

    use super::{handle_parse_handshake, perform_parse_req, prepare_handshake};

    /// perform http upgrade
    ///
    /// **NOTE**: low level api
    pub fn req_handshake<S: Read + Write>(
        stream: &mut S,
        uri: &http::Uri,
        protocols: &[String],
        extensions: &[String],
        version: u8,
        extra_headers: HashMap<String, String>,
    ) -> Result<(String, http::Response<()>), WsError> {
        let (key, req_str) = prepare_handshake(protocols, extensions, extra_headers, uri, version);
        stream.write_all(req_str.as_bytes())?;
        stream.flush()?;
        let mut read_bytes = BytesMut::with_capacity(1024);
        let mut buf: [u8; 1] = [0; 1];
        loop {
            stream.read_exact(&mut buf)?;
            read_bytes.put_u8(buf[0]);
            let header_complete = read_bytes.ends_with(&[b'\r', b'\n', b'\r', b'\n']);
            if header_complete {
                break;
            }
        }
        perform_parse_req(read_bytes, key)
    }

    /// handle protocol handshake
    pub fn handle_handshake<S: Read + Write>(stream: &mut S) -> Result<http::Request<()>, WsError> {
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

#[cfg(feature = "sync")]
pub use blocking::*;

#[cfg(feature = "async")]
mod non_blocking {
    use http;
    use std::collections::HashMap;

    use bytes::{BufMut, BytesMut};
    use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

    use crate::{errors::WsError, protocol::prepare_handshake};

    use super::{handle_parse_handshake, perform_parse_req};

    /// perform http upgrade
    ///
    /// **NOTE**: low level api
    pub async fn async_req_handshake<S: AsyncRead + AsyncWrite + Unpin>(
        stream: &mut S,
        uri: &http::Uri,
        protocols: &[String],
        extensions: &[String],
        version: u8,
        extra_headers: HashMap<String, String>,
    ) -> Result<(String, http::Response<()>), WsError> {
        let (key, req_str) = prepare_handshake(protocols, extensions, extra_headers, uri, version);
        stream.write_all(req_str.as_bytes()).await?;
        let mut read_bytes = BytesMut::with_capacity(1024);
        let mut buf = [0u8];
        loop {
            stream.read_exact(&mut buf).await?;
            read_bytes.put_u8(buf[0]);
            let header_complete = read_bytes.ends_with(&[b'\r', b'\n', b'\r', b'\n']);
            if header_complete {
                break;
            }
        }
        perform_parse_req(read_bytes, key)
    }

    /// async version of handling protocol handshake
    pub async fn async_handle_handshake<S: AsyncRead + AsyncWrite + Unpin>(
        stream: &mut S,
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
    base64::encode(r)
}

/// cal accept key
pub fn cal_accept_key(source: &[u8]) -> String {
    let mut sha1 = sha1::Sha1::default();
    sha1.update(source);
    sha1.update(GUID);
    base64::encode(sha1.finalize())
}

/// perform standard protocol handshake response check
///
/// 1. check status code
/// 2. check `sec-websocket-accept` header & value
pub fn standard_handshake_resp_check(key: &[u8], resp: &http::Response<()>) -> Result<(), WsError> {
    tracing::debug!("handshake response {:?}", resp);
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
                "expect `websocket`, got {val:?}"
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

/// build protocol http reqeust
///
/// return (key, request_str)
pub fn prepare_handshake(
    protocols: &[String],
    extensions: &[String],
    extra_headers: HashMap<String, String>,
    uri: &http::Uri,
    version: u8,
) -> (String, String) {
    let key = gen_key();
    let mut headers = vec![
        format!(
            "Host: {}{}",
            uri.host().unwrap_or_default(),
            uri.port_u16().map(|p| format!(":{p}")).unwrap_or_default()
        ),
        "Upgrade: websocket".to_string(),
        "Connection: Upgrade".to_string(),
        format!("Sec-Websocket-Key: {key}"),
        format!("Sec-WebSocket-Version: {version}"),
    ];
    for pro in protocols {
        headers.push(format!("Sec-WebSocket-Protocol: {pro}"))
    }
    for ext in extensions {
        headers.push(format!("Sec-WebSocket-Extensions: {ext}"))
    }
    for (k, v) in extra_headers.iter() {
        headers.push(format!("{k}: {v}"));
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

/// parse protocol response
pub fn perform_parse_req(
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
        .version(match resp.version.unwrap_or(1) {
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

/// parse http request, used by server building
pub fn handle_parse_handshake(req_bytes: BytesMut) -> Result<http::Request<()>, WsError> {
    let mut headers = [httparse::EMPTY_HEADER; 64];
    let mut req = httparse::Request::new(&mut headers);
    let _parse_status = req
        .parse(&req_bytes)
        .map_err(|_| WsError::HandShakeFailed("invalid request".to_string()))?;
    let mut req_builder = http::Request::builder()
        .method(req.method.unwrap_or_default())
        .uri(req.path.unwrap_or_default())
        .version(match req.version.unwrap_or(1) {
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
    req_builder
        .body(())
        .map_err(|e| WsError::HandShakeFailed(e.to_string()))
}
