use std::io::{BufReader, Read};
use std::{fmt::Debug, sync::Arc};

use tokio::net::TcpStream;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio_rustls::{client::TlsStream, rustls::ClientConfig, TlsConnector};
use webpki::DNSNameRef;

pub mod errors;
pub mod frame;
use errors::WsError;

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
    let mut pem = std::fs::File::open("./scripts/target.pem").unwrap();
    let mut cert = BufReader::new(&mut pem);
    config.root_store.add_pem_file(&mut cert).unwrap();

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
