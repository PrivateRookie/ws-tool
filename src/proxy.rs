use std::{
    net::{SocketAddr, ToSocketAddrs},
    str::FromStr,
};

use crate::errors::WsError;

#[derive(Debug, Clone)]
pub enum ProxySchema {
    Socket5,
    Http,
}

#[derive(Debug, Clone)]
pub struct Proxy {
    socket: SocketAddr,
    schema: ProxySchema,
}

impl FromStr for Proxy {
    type Err = WsError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let uri = s
            .parse::<http::Uri>()
            .map_err(|e| WsError::InvalidUri(e.to_string()))?;
        let schema_str = uri.scheme_str().unwrap_or_default().to_lowercase();
        let schema = match schema_str.as_str() {
            "sock5" => Ok(ProxySchema::Socket5),
            "http" => Ok(ProxySchema::Http),
            _ => Err(WsError::UnsupportedProxy(schema_str)),
        }?;
        let host = uri.host().unwrap_or_default();
        let port = uri.port_u16().unwrap_or_default();

        let trimmed_host = host.trim_start_matches('[').trim_end_matches(']');
        let mut socket_iter = format!("[{}]:{}", trimmed_host, port)
            .to_socket_addrs()
            .map_err(|e| WsError::InvalidProxy(format!("{} {}", s, e.to_string())))?;
        let socket = socket_iter
            .next()
            .ok_or(WsError::InvalidProxy(format!("{} empty proxy socket", s)))?;
        Ok(Self { schema, socket })
    }
}
