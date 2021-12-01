#[cfg(feature = "rustls")]
mod ws_stream {
    use tokio::io::{AsyncRead, AsyncWrite};
    use tokio::net::TcpStream;
    use tokio_rustls::client::TlsStream;

    #[derive(Debug)]
    pub enum WsStream {
        Plain(TcpStream),
        Tls(TlsStream<TcpStream>),
    }

    impl WsStream {
        pub fn set_nodelay(&mut self) -> std::io::Result<()> {
            match self {
                WsStream::Plain(s) => s.set_nodelay(true),
                WsStream::Tls(s) => s.get_mut().0.set_nodelay(true),
            }
        }
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
}

#[cfg(not(feature = "rustls"))]
mod ws_stream {
    use tokio::io::{AsyncRead, AsyncWrite};
    use tokio::net::TcpStream;

    #[derive(Debug)]
    pub enum WsStream {
        Plain(TcpStream),
    }

    impl WsStream {
        pub fn set_nodelay(&mut self) -> std::io::Result<()> {
            match self {
                WsStream::Plain(s) => s.set_nodelay(true),
            }
        }
    }

    impl AsyncRead for WsStream {
        fn poll_read(
            self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
            buf: &mut tokio::io::ReadBuf<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            match self.get_mut() {
                WsStream::Plain(stream) => std::pin::Pin::new(stream).poll_read(cx, buf),
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
            }
        }

        fn poll_flush(
            self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Result<(), std::io::Error>> {
            match self.get_mut() {
                WsStream::Plain(stream) => std::pin::Pin::new(stream).poll_flush(cx),
            }
        }

        fn poll_shutdown(
            self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Result<(), std::io::Error>> {
            match self.get_mut() {
                WsStream::Plain(stream) => std::pin::Pin::new(stream).poll_shutdown(cx),
            }
        }
    }
}

pub use ws_stream::WsStream;
