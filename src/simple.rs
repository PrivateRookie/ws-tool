use http::Uri;
use crate::{
    codec::{PMDConfig, WindowBit},
    connector::{get_host, get_scheme},
    errors::WsError,
    protocol::Mode,
    ClientBuilder,
};
use std::{collections::HashMap, path::PathBuf};

/// client connection config
pub struct ClientConfig {
    /// read buffer size
    pub read_buf: usize,
    /// write buffer size
    pub write_buf: usize,
    /// custom certification path
    pub certs: Vec<PathBuf>,
    /// deflate window size, if none, deflate will be disabled
    pub window: Option<WindowBit>,
    /// enable/disable deflate context taker over parameter
    pub context_take_over: bool,
    /// extra header when perform websocket protocol handshake
    pub extra_headers: HashMap<String, String>,
    /// modified socket option after create tcp socket, this function will be applied
    /// before start tls session
    pub set_socket_fn: Box<dyn FnMut(&std::net::TcpStream) -> Result<(), WsError> + Send + 'static>,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            read_buf: Default::default(),
            write_buf: Default::default(),
            certs: Default::default(),
            window: Default::default(),
            context_take_over: Default::default(),
            extra_headers: Default::default(),
            set_socket_fn: Box::new(|_| Ok(())),
        }
    }
}

impl ClientConfig {
    /// use default buffer size 8192
    pub fn buffered() -> Self {
        Self {
            read_buf: 8192,
            write_buf: 8192,
            ..Default::default()
        }
    }

    /// perform websocket handshake, use custom codec
    pub fn connect_with<C, F>(
        &mut self,
        uri: impl TryInto<Uri, Error = http::uri::InvalidUri>,
        mut check_fn: F,
    ) -> Result<C, WsError>
    where
        F: FnMut(
            String,
            http::Response<()>,
            crate::stream::BufStream<crate::stream::SyncStream>,
        ) -> Result<C, WsError>,
    {
        let (uri, mode, builder) = self.prepare(uri)?;
        let stream = crate::connector::tcp_connect(&uri)?;
        (self.set_socket_fn)(&stream)?;
        let check_fn = |key, resp, stream| {
            let stream =
                crate::stream::BufStream::with_capacity(self.read_buf, self.write_buf, stream);
            check_fn(key, resp, stream)
        };
        match mode {
            Mode::WS => builder.with_stream(uri, crate::stream::SyncStream::Raw(stream), check_fn),
            Mode::WSS => {
                let host = get_host(&uri)?;
                if cfg!(feature = "sync_tls_rustls") {
                    #[cfg(feature = "sync_tls_rustls")]
                    {
                        let stream =
                            crate::connector::wrap_rustls(stream, host, self.certs.clone())?;
                        builder.with_stream(
                            uri,
                            crate::stream::SyncStream::Rustls(stream),
                            check_fn,
                        )
                    }
                    #[cfg(not(feature = "sync_tls_rustls"))]
                    {
                        panic!("")
                    }
                } else if cfg!(feature = "sync_tls_native") {
                    #[cfg(feature = "sync_tls_native")]
                    {
                        let stream =
                            crate::connector::wrap_native_tls(stream, host, self.certs.clone())?;
                        builder.with_stream(
                            uri,
                            crate::stream::SyncStream::NativeTls(stream),
                            check_fn,
                        )
                    }
                    #[cfg(not(feature = "sync_tls_native"))]
                    {
                        panic!("")
                    }
                } else {
                    panic!("for ssl connection, sync_tls_native or sync_tls_rustls feature is required")
                }
            }
        }
    }

    /// perform websocket handshake
    #[cfg(feature = "sync")]
    pub fn connect(
        &mut self,
        uri: impl TryInto<Uri, Error = http::uri::InvalidUri>,
    ) -> Result<
        crate::codec::DeflateCodec<crate::stream::BufStream<crate::stream::SyncStream>>,
        WsError,
    > {
        self.connect_with(uri, crate::codec::DeflateCodec::check_fn)
    }

    /// perform websocket handshake
    #[cfg(feature = "async")]
    #[allow(unused)]
    pub async fn async_connect_with<C, F>(
        &mut self,
        uri: impl TryInto<Uri, Error = http::uri::InvalidUri>,
        mut check_fn: F,
    ) -> Result<C, WsError>
    where
        F: FnMut(
            String,
            http::Response<()>,
            tokio::io::BufStream<crate::stream::AsyncStream>,
        ) -> Result<C, WsError>,
    {
        let (uri, mode, builder) = self.prepare(uri)?;
        let stream = crate::connector::async_tcp_connect(&uri).await?;
        let stream = stream.into_std()?;
        (self.set_socket_fn)(&stream)?;
        let stream = tokio::net::TcpStream::from_std(stream)?;
        let check_fn = |key, resp, stream: crate::stream::AsyncStream| {
            let stream = tokio::io::BufStream::with_capacity(self.read_buf, self.write_buf, stream);
            check_fn(key, resp, stream)
        };
        match mode {
            Mode::WS => {
                builder
                    .async_with_stream(uri, crate::stream::AsyncStream::Raw(stream), check_fn)
                    .await
            }
            Mode::WSS => {
                let host = get_host(&uri)?;
                if cfg!(feature = "async_tls_rustls") {
                    #[cfg(feature = "async_tls_rustls")]
                    {
                        let stream =
                            crate::connector::async_wrap_rustls(stream, host, self.certs.clone())
                                .await?;
                        builder
                            .async_with_stream(
                                uri,
                                crate::stream::AsyncStream::Rustls(
                                    tokio_rustls::TlsStream::Client(stream),
                                ),
                                check_fn,
                            )
                            .await
                    }
                    #[cfg(not(feature = "async_tls_rustls"))]
                    {
                        panic!("")
                    }
                } else if cfg!(feature = "async_tls_native") {
                    #[cfg(feature = "async_tls_native")]
                    {
                        let stream = crate::connector::async_wrap_native_tls(
                            stream,
                            host,
                            self.certs.clone(),
                        )
                        .await?;
                        builder
                            .async_with_stream(
                                uri,
                                crate::stream::AsyncStream::NativeTls(stream),
                                check_fn,
                            )
                            .await
                    }
                    #[cfg(not(feature = "async_tls_native"))]
                    {
                        panic!("")
                    }
                } else {
                    panic!("for ssl connection, async_tls_native or async_tls_rustls feature is required")
                }
            }
        }
    }

    /// perform websocket handshake
    #[cfg(feature = "async")]
    pub async fn async_connect(
        &mut self,
        uri: impl TryInto<Uri, Error = http::uri::InvalidUri>,
    ) -> Result<
        crate::codec::AsyncDeflateCodec<tokio::io::BufStream<crate::stream::AsyncStream>>,
        WsError,
    > {
        self.async_connect_with(uri, crate::codec::AsyncDeflateCodec::check_fn)
            .await
    }

    fn prepare(
        &mut self,
        uri: impl TryInto<Uri, Error = http::uri::InvalidUri>,
    ) -> Result<(Uri, Mode, ClientBuilder), WsError> {
        let uri = uri
            .try_into()
            .map_err(|e| WsError::InvalidUri(e.to_string()))?;
        let mode = get_scheme(&uri)?;
        let mut builder = ClientBuilder::new();
        let pmd_conf = self.window.map(|w| PMDConfig {
            server_no_context_takeover: self.context_take_over,
            client_no_context_takeover: self.context_take_over,
            server_max_window_bits: w,
            client_max_window_bits: w,
        });
        if let Some(conf) = pmd_conf {
            builder = builder.extension(conf.ext_string())
        }
        for (k, v) in &self.extra_headers {
            builder = builder.header(k, v);
        }
        Ok((uri, mode, builder))
    }
}
