use std::io::BufReader;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::{fmt::Debug, sync::Arc};

use stream::WsStream;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;

use tokio_rustls::{client::TlsStream, rustls::ClientConfig, TlsConnector};
use webpki::DNSNameRef;

pub mod errors;
pub mod frame;
pub mod stream;
pub mod proxy;
use errors::WsError;

#[derive(Debug)]
pub struct Client {
    pub uri: http::Uri,
    stream: stream::WsStream,
    pub state: ConnectionState,
}

pub struct ClientBuilder {
    uri: String,
    proxy_uri: Option<String>,
    protocols: Vec<String>,
}

impl ClientBuilder {
    pub fn new(uri: &str) -> Self {
        Self {
            uri: uri.to_string(),
            proxy_uri: None,
            protocols: vec![],
        }
    }

    /// config  proxy
    pub fn proxy(self, uri: &str) -> Self {
        Self {
            proxy_uri: Some(uri.to_string()),
            ..self
        }
    }


    /// add protocol in handshake http header
    pub fn protocols(self, protocols: Vec<String>) -> Self {
        Self { protocols, ..self }
    }

    pub fn build(self) -> Result<Client, WsError> {
        let Self {
            uri,
            proxy_uri,
            protocols,
        } = self;
        let uri = uri
            .parse::<http::Uri>()
            .map_err(|e| WsError::InvalidUri(format!("{} {}", uri, e.to_string())))?;
        let ws_proxy: Option<proxy::Proxy> = match proxy_uri {
            Some(uri) => Some(uri.parse()?),
            None => None,
        };
        todo!()
    }
}

/// websocket connection state
#[derive(Debug, Clone)]
pub enum ConnectionState {
    /// init state, tcp & tls connection creating state
    Connecting,
    /// perform websocket handshake
    HandShaking,
    /// websocket connection has been successfully established
    Running,
    /// client or peer has send "close frame"
    Closing,
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

/// create connection
pub async fn connect(uri: &str, cert: Option<PathBuf>) -> Result<WsStream, WsError> {
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
            let tls_stream = wrap_tls(stream, host, cert).await?;
            WsStream::Tls(tls_stream)
        }
    };
    handshake(&mut stream, &uri).await.unwrap();

    Ok(stream)
}

async fn wrap_tls(
    stream: TcpStream,
    host: &str,
    cert: Option<PathBuf>,
) -> Result<TlsStream<TcpStream>, WsError> {
    let mut config = ClientConfig::new();
    if let Some(cert_path) = cert {
        let mut pem = std::fs::File::open(cert_path).unwrap();
        let mut cert = BufReader::new(&mut pem);
        config.root_store.add_pem_file(&mut cert).unwrap();
    }
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
