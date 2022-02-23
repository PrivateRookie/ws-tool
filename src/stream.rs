#[cfg(feature = "blocking")]
mod blocking {

    #[cfg(feature = "tls_rutls")]
    mod stream {
        use rustls_connector::TlsStream;
        use std::{
            io::{Read, Write},
            net::TcpStream,
        };

        pub enum WsStream {
            Plain(TcpStream),
            Tls(TlsStream<TcpStream>),
        }

        impl WsStream {
            pub fn stream_mut(&mut self) -> &mut TcpStream {
                match self {
                    WsStream::Plain(s) => &mut s,
                    WsStream::Tls(tls) => tls.get_mut(),
                }
            }
        }

        impl Read for WsStream {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                match self {
                    WsStream::Plain(s) => s.read(buf),
                }
            }
        }

        impl Write for WsStream {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                match self {
                    WsStream::Plain(s) => s.write(buf),
                }
            }

            fn flush(&mut self) -> std::io::Result<()> {
                match self {
                    WsStream::Plain(s) => s.flush(),
                }
            }
        }
    }

    #[cfg(not(feature = "tls_rutls"))]
    mod stream {
        use std::{
            io::{Read, Write},
            net::TcpStream,
        };

        pub enum WsStream {
            Plain(TcpStream),
        }

        impl WsStream {
            pub fn stream_mut(&mut self) -> &mut TcpStream {
                match self {
                    WsStream::Plain(s) => s,
                }
            }
        }

        impl Read for WsStream {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                match self {
                    WsStream::Plain(s) => s.read(buf),
                }
            }
        }

        impl Write for WsStream {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                match self {
                    WsStream::Plain(s) => s.write(buf),
                }
            }

            fn flush(&mut self) -> std::io::Result<()> {
                match self {
                    WsStream::Plain(s) => s.flush(),
                }
            }
        }
    }

    pub use stream::WsStream;
}

#[cfg(feature = "blocking")]
pub use blocking::WsStream;

#[cfg(feature = "async")]
mod non_blocking {
    #[cfg(feature = "async_tls_rustls")]
    mod ws_stream {
        use tokio::io::{AsyncRead, AsyncWrite};
        use tokio::net::TcpStream;
        use tokio_rustls::client::TlsStream;

        #[derive(Debug)]
        pub enum WsAsyncStream {
            Plain(TcpStream),
            Tls(TlsStream<TcpStream>),
        }

        impl WsAsyncStream {
            pub fn set_nodelay(&mut self) -> std::io::Result<()> {
                match self {
                    WsAsyncStream::Plain(s) => s.set_nodelay(true),
                    WsAsyncStream::Tls(s) => s.get_mut().0.set_nodelay(true),
                }
            }
        }

        impl AsyncRead for WsAsyncStream {
            fn poll_read(
                self: std::pin::Pin<&mut Self>,
                cx: &mut std::task::Context<'_>,
                buf: &mut tokio::io::ReadBuf<'_>,
            ) -> std::task::Poll<std::io::Result<()>> {
                match self.get_mut() {
                    WsAsyncStream::Plain(stream) => std::pin::Pin::new(stream).poll_read(cx, buf),
                    WsAsyncStream::Tls(stream) => std::pin::Pin::new(stream).poll_read(cx, buf),
                }
            }
        }

        impl AsyncWrite for WsAsyncStream {
            fn poll_write(
                self: std::pin::Pin<&mut Self>,
                cx: &mut std::task::Context<'_>,
                buf: &[u8],
            ) -> std::task::Poll<Result<usize, std::io::Error>> {
                match self.get_mut() {
                    WsAsyncStream::Plain(stream) => std::pin::Pin::new(stream).poll_write(cx, buf),
                    WsAsyncStream::Tls(stream) => std::pin::Pin::new(stream).poll_write(cx, buf),
                }
            }

            fn poll_flush(
                self: std::pin::Pin<&mut Self>,
                cx: &mut std::task::Context<'_>,
            ) -> std::task::Poll<Result<(), std::io::Error>> {
                match self.get_mut() {
                    WsAsyncStream::Plain(stream) => std::pin::Pin::new(stream).poll_flush(cx),
                    WsAsyncStream::Tls(stream) => std::pin::Pin::new(stream).poll_flush(cx),
                }
            }

            fn poll_shutdown(
                self: std::pin::Pin<&mut Self>,
                cx: &mut std::task::Context<'_>,
            ) -> std::task::Poll<Result<(), std::io::Error>> {
                match self.get_mut() {
                    WsAsyncStream::Plain(stream) => std::pin::Pin::new(stream).poll_shutdown(cx),
                    WsAsyncStream::Tls(stream) => std::pin::Pin::new(stream).poll_shutdown(cx),
                }
            }
        }
    }

    #[cfg(not(feature = "async_tls_rustls"))]
    mod ws_stream {
        use tokio::io::{AsyncRead, AsyncWrite};
        use tokio::net::TcpStream;

        #[derive(Debug)]
        pub enum WsAsyncStream {
            Plain(TcpStream),
        }

        impl WsAsyncStream {
            pub fn set_nodelay(&mut self) -> std::io::Result<()> {
                match self {
                    WsAsyncStream::Plain(s) => s.set_nodelay(true),
                }
            }
        }

        impl AsyncRead for WsAsyncStream {
            fn poll_read(
                self: std::pin::Pin<&mut Self>,
                cx: &mut std::task::Context<'_>,
                buf: &mut tokio::io::ReadBuf<'_>,
            ) -> std::task::Poll<std::io::Result<()>> {
                match self.get_mut() {
                    WsAsyncStream::Plain(stream) => std::pin::Pin::new(stream).poll_read(cx, buf),
                }
            }
        }

        impl AsyncWrite for WsAsyncStream {
            fn poll_write(
                self: std::pin::Pin<&mut Self>,
                cx: &mut std::task::Context<'_>,
                buf: &[u8],
            ) -> std::task::Poll<Result<usize, std::io::Error>> {
                match self.get_mut() {
                    WsAsyncStream::Plain(stream) => std::pin::Pin::new(stream).poll_write(cx, buf),
                }
            }

            fn poll_flush(
                self: std::pin::Pin<&mut Self>,
                cx: &mut std::task::Context<'_>,
            ) -> std::task::Poll<Result<(), std::io::Error>> {
                match self.get_mut() {
                    WsAsyncStream::Plain(stream) => std::pin::Pin::new(stream).poll_flush(cx),
                }
            }

            fn poll_shutdown(
                self: std::pin::Pin<&mut Self>,
                cx: &mut std::task::Context<'_>,
            ) -> std::task::Poll<Result<(), std::io::Error>> {
                match self.get_mut() {
                    WsAsyncStream::Plain(stream) => std::pin::Pin::new(stream).poll_shutdown(cx),
                }
            }
        }
    }

    pub use ws_stream::WsAsyncStream;
}

#[cfg(feature = "async")]
pub use non_blocking::WsAsyncStream;
