use std::{
    net::{SocketAddr, ToSocketAddrs},
    str::FromStr,
};
use tokio::net::TcpStream;

use crate::errors::WsError;

#[derive(Debug, Clone)]
pub enum ProxySchema {
    Socks5,
    Http,
}

#[derive(Debug, Clone)]
pub struct Proxy {
    pub socket: SocketAddr,
    pub schema: ProxySchema,
}

impl FromStr for Proxy {
    type Err = WsError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let uri = s
            .parse::<http::Uri>()
            .map_err(|e| WsError::InvalidUri(e.to_string()))?;
        let schema_str = uri.scheme_str().unwrap_or_default().to_lowercase();
        let schema = match schema_str.as_str() {
            "socks5" => Ok(ProxySchema::Socks5),
            "http" => Ok(ProxySchema::Http),
            _ => Err(WsError::UnsupportedProxy(schema_str)),
        }?;
        let host = uri.host().unwrap_or_default();
        let port = uri.port_u16().unwrap_or_default();

        let mut socket_iter = (host, port)
            .to_socket_addrs()
            .map_err(|e| WsError::InvalidProxy(format!("{} {}", s, e)))?;
        let socket = socket_iter
            .next()
            .ok_or_else(|| WsError::InvalidProxy(format!("{} empty proxy socket", s)))?;
        Ok(Self { socket, schema })
    }
}

impl Proxy {
    pub(crate) async fn connect(&self, target: (&str, u16)) -> Result<TcpStream, WsError> {
        match &self.schema {
            ProxySchema::Socks5 => {
                let stream = tokio_socks::tcp::Socks5Stream::connect(self.socket, target)
                    .await
                    .map_err(|e| WsError::ProxyError(e.to_string()))?;
                Ok(stream.into_inner())
            }
            ProxySchema::Http => {
                let mut stream = TcpStream::connect(self.socket).await.map_err(|e| {
                    WsError::ConnectionFailed(format!(
                        "failed to create tcp connection {}",
                        e
                    ))
                })?;
                async_http_proxy::http_connect_tokio(&mut stream, target.0, target.1)
                    .await
                    .map_err(|e| WsError::ProxyError(e.to_string()))?;
                Ok(stream)
            }
        }
    }
}
