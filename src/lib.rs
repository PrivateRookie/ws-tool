//! rust websocket toolkit

#![warn(missing_docs)]
#![cfg_attr(docrs, feature(doc_auto_cfg))]

use std::collections::HashMap;

pub use http_shim as http;

/// websocket error definitions
pub mod errors;
/// websocket transport unit
pub mod frame;
/// build connection & read/write frame utils
pub mod protocol;

/// frame codec impl
pub mod codec;
/// connection helper function
pub mod connector;

/// helper message definition
mod message;
pub use message::*;
#[cfg(feature = "simple")]
/// simple api to create websocket connection
pub mod simple;
#[cfg(feature = "simple")]
pub use simple::ClientConfig;

/// helper stream definition
pub mod stream;

/// some helper extension
pub mod extension;

/// helper builder to construct websocket client
#[derive(Debug, Clone)]
pub struct ClientBuilder {
    protocols: Vec<String>,
    extensions: Vec<String>,
    #[cfg_attr(not(any(feature = "sync", feature = "async")), allow(dead_code))]
    version: u8,
    headers: HashMap<String, String>,
}

impl Default for ClientBuilder {
    fn default() -> Self {
        Self {
            protocols: vec![],
            extensions: vec![],
            headers: HashMap::new(),
            version: 13,
        }
    }
}

impl ClientBuilder {
    /// create builder with websocket url
    pub fn new() -> Self {
        Default::default()
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

    /// set websocket version, default 13
    pub fn version(self, version: u8) -> Self {
        Self { version, ..self }
    }

    /// add initial request header
    pub fn header<K: ToString, V: ToString>(mut self, name: K, value: V) -> Self {
        self.headers.insert(name.to_string(), value.to_string());
        self
    }

    /// set initial request headers
    ///
    /// **NOTE** it will clear header set by previous `header` method
    pub fn headers(self, headers: HashMap<String, String>) -> Self {
        Self { headers, ..self }
    }
}

#[cfg(feature = "sync")]
mod blocking {
    use crate::http;
    use std::{
        io::{Read, Write},
        net::TcpStream,
    };

    use crate::{
        connector::{get_scheme, tcp_connect},
        errors::WsError,
        protocol::{handle_handshake, req_handshake},
        ClientBuilder, ServerBuilder,
    };

    impl ClientBuilder {
        /// perform protocol handshake & check server response
        pub fn connect<C, F>(&self, uri: http::Uri, check_fn: F) -> Result<C, WsError>
        where
            F: FnMut(String, http::Response<()>, TcpStream) -> Result<C, WsError>,
        {
            let mode = get_scheme(&uri)?;
            if matches!(mode, crate::protocol::Mode::WSS) {
                panic!("can not perform ssl connection, use `rustls_connect` or `native_tls_connect` instead");
            }
            let stream = tcp_connect(&uri)?;
            self.with_stream(uri, stream, check_fn)
        }

        #[cfg(feature = "sync_tls_rustls")]
        /// perform protocol handshake via ssl with default certs & check server response
        pub fn rustls_connect<C, F>(&self, uri: http::Uri, check_fn: F) -> Result<C, WsError>
        where
            F: FnMut(
                String,
                http::Response<()>,
                rustls_connector::rustls::StreamOwned<
                    rustls_connector::rustls::ClientConnection,
                    TcpStream,
                >,
            ) -> Result<C, WsError>,
        {
            use crate::connector::{get_host, wrap_rustls};
            let mode = get_scheme(&uri)?;
            if matches!(mode, crate::protocol::Mode::WSS) {
                panic!("can not perform not ssl connection, use `connect` instead");
            }
            let stream = tcp_connect(&uri)?;
            let stream = wrap_rustls(stream, get_host(&uri)?, vec![])?;
            self.with_stream(uri, stream, check_fn)
        }

        #[cfg(feature = "sync_tls_native")]
        /// perform protocol handshake via ssl with default certs & check server response
        pub fn native_tls_connect<C, F>(&self, uri: http::Uri, check_fn: F) -> Result<C, WsError>
        where
            F: FnMut(
                String,
                http::Response<()>,
                native_tls::TlsStream<TcpStream>,
            ) -> Result<C, WsError>,
        {
            use crate::connector::{get_host, wrap_native_tls};
            let mode = get_scheme(&uri)?;
            if matches!(mode, crate::protocol::Mode::WSS) {
                panic!("can not perform not ssl connection, use `connect` instead");
            }
            let stream = tcp_connect(&uri)?;
            let stream = wrap_native_tls(stream, get_host(&uri)?, vec![])?;
            self.with_stream(uri, stream, check_fn)
        }

        /// ## Low level api
        /// perform protocol handshake & check server response
        pub fn with_stream<C, F, S>(
            &self,
            uri: http::Uri,
            mut stream: S,
            mut check_fn: F,
        ) -> Result<C, WsError>
        where
            S: Read + Write,
            F: FnMut(String, http::Response<()>, S) -> Result<C, WsError>,
        {
            get_scheme(&uri)?;
            let (key, resp) = req_handshake(
                &mut stream,
                &uri,
                &self.protocols,
                &self.extensions,
                self.version,
                self.headers.clone(),
            )?;
            check_fn(key, resp, stream)
        }
    }

    impl ServerBuilder {
        /// wait for protocol handshake from client
        /// checking handshake & construct server
        pub fn accept<F1, F2, T, C, S>(
            mut stream: S,
            mut handshake_handler: F1,
            mut codec_factory: F2,
        ) -> Result<C, WsError>
        where
            S: Read + Write,
            F1: FnMut(
                http::Request<()>,
            ) -> Result<
                (http::Request<()>, http::Response<T>),
                (http::Response<T>, WsError),
            >,
            F2: FnMut(http::Request<()>, S) -> Result<C, WsError>,
            T: ToString + std::fmt::Debug,
        {
            let req = handle_handshake(&mut stream)?;
            match handshake_handler(req) {
                Err((resp, e)) => {
                    write_resp(resp, &mut stream)?;
                    return Err(e);
                }
                Ok((req, resp)) => {
                    write_resp(resp, &mut stream)?;
                    codec_factory(req, stream)
                }
            }
        }
    }

    fn write_resp<S, T>(resp: http::Response<T>, stream: &mut S) -> Result<(), WsError>
    where
        S: Read + Write,
        T: ToString + std::fmt::Debug,
    {
        let mut resp_lines = vec![format!("{:?} {}", resp.version(), resp.status())];
        resp.headers().iter().for_each(|(k, v)| {
            resp_lines.push(format!("{}: {}", k, v.to_str().unwrap_or_default()))
        });
        resp_lines.push("\r\n".to_string());
        stream.write_all(resp_lines.join("\r\n").as_bytes())?;
        tracing::debug!("{:?}", &resp);
        Ok(if resp.status() != http::StatusCode::SWITCHING_PROTOCOLS {
            return Err(WsError::HandShakeFailed(resp.body().to_string()));
        })
    }
}

#[cfg(feature = "async")]
mod non_blocking {
    use crate::http;
    use std::fmt::Debug;

    use tokio::{
        io::{AsyncRead, AsyncWrite, AsyncWriteExt},
        net::TcpStream,
    };

    use crate::{
        connector::async_tcp_connect,
        errors::WsError,
        protocol::{async_handle_handshake, async_req_handshake},
        ServerBuilder,
    };

    use super::ClientBuilder;

    impl ClientBuilder {
        /// perform protocol handshake & check server response
        pub async fn async_connect<C, F>(&self, uri: http::Uri, check_fn: F) -> Result<C, WsError>
        where
            F: FnMut(String, http::Response<()>, TcpStream) -> Result<C, WsError>,
        {
            let stream = async_tcp_connect(&uri).await?;
            self.async_with_stream(uri, stream, check_fn).await
        }

        #[cfg(feature = "async_tls_rustls")]
        /// perform protocol handshake via ssl with default certs & check server response
        pub async fn async_rustls_connect<C, F>(
            &self,
            uri: http::Uri,
            check_fn: F,
        ) -> Result<C, WsError>
        where
            F: FnMut(
                String,
                http::Response<()>,
                tokio_rustls::client::TlsStream<tokio::net::TcpStream>,
            ) -> Result<C, WsError>,
        {
            use crate::connector::{async_wrap_rustls, get_host};
            let mode = crate::connector::get_scheme(&uri)?;
            if matches!(mode, crate::protocol::Mode::WSS) {
                panic!("can not perform not ssl connection, use `connect` instead");
            }
            let stream = async_tcp_connect(&uri).await?;
            let stream = async_wrap_rustls(stream, get_host(&uri)?, vec![]).await?;
            self.async_with_stream(uri, stream, check_fn).await
        }

        #[cfg(feature = "async_tls_native")]
        /// perform protocol handshake via ssl with default certs & check server response
        pub async fn async_native_tls_connect<C, F>(
            &self,
            uri: http::Uri,
            check_fn: F,
        ) -> Result<C, WsError>
        where
            F: FnMut(
                String,
                http::Response<()>,
                tokio_native_tls::TlsStream<TcpStream>,
            ) -> Result<C, WsError>,
        {
            use crate::connector::{async_wrap_native_tls, get_host};
            let mode = crate::connector::get_scheme(&uri)?;
            if matches!(mode, crate::protocol::Mode::WSS) {
                panic!("can not perform not ssl connection, use `connect` instead");
            }
            let stream = async_tcp_connect(&uri).await?;
            let stream = async_wrap_native_tls(stream, get_host(&uri)?, vec![]).await?;
            self.async_with_stream(uri, stream, check_fn).await
        }

        /// async version of connect
        ///
        /// perform protocol handshake & check server response
        pub async fn async_with_stream<C, F, S>(
            &self,
            uri: http::Uri,
            mut stream: S,
            mut check_fn: F,
        ) -> Result<C, WsError>
        where
            S: AsyncRead + AsyncWrite + Unpin,
            F: FnMut(String, http::Response<()>, S) -> Result<C, WsError>,
        {
            let (key, resp) = async_req_handshake(
                &mut stream,
                &uri,
                &self.protocols,
                &self.extensions,
                self.version,
                self.headers.clone(),
            )
            .await?;
            check_fn(key, resp, stream)
        }
    }

    impl ServerBuilder {
        /// async version
        ///
        /// wait for protocol handshake from client
        /// checking handshake & construct server
        pub async fn async_accept<F1, F2, T, C, S>(
            mut stream: S,
            mut handshake_handler: F1,
            mut codec_factory: F2,
        ) -> Result<C, WsError>
        where
            S: AsyncRead + AsyncWrite + Unpin,
            F1: FnMut(
                http::Request<()>,
            ) -> Result<
                (http::Request<()>, http::Response<T>),
                (http::Response<T>, WsError),
            >,
            F2: FnMut(http::Request<()>, S) -> Result<C, WsError>,
            T: ToString + Debug,
        {
            let req = async_handle_handshake(&mut stream).await?;
            match handshake_handler(req) {
                Ok((req, resp)) => {
                    async_write_resp(resp, &mut stream).await?;
                    codec_factory(req, stream)
                }
                Err((resp, e)) => {
                    async_write_resp(resp, &mut stream).await?;
                    return Err(e);
                }
            }
        }
    }

    async fn async_write_resp<S, T>(resp: http::Response<T>, stream: &mut S) -> Result<(), WsError>
    where
        S: AsyncRead + AsyncWrite + Unpin,
        T: ToString + Debug,
    {
        let mut resp_lines = vec![format!("{:?} {}", resp.version(), resp.status())];
        resp.headers().iter().for_each(|(k, v)| {
            resp_lines.push(format!("{}: {}", k, v.to_str().unwrap_or_default()))
        });
        resp_lines.push("\r\n".to_string());
        stream.write_all(resp_lines.join("\r\n").as_bytes()).await?;
        tracing::debug!("{:?}", &resp);
        Ok(if resp.status() != http::StatusCode::SWITCHING_PROTOCOLS {
            return Err(WsError::HandShakeFailed(resp.body().to_string()));
        })
    }
}

/// helper struct to config & construct websocket server
pub struct ServerBuilder {}
