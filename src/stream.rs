#[cfg(feature = "sync")]
mod blocking {

    #[cfg(feature = "sync_tls_rustls")]
    mod stream {
        use rustls_connector::TlsStream;
        use std::io::{Read, Write};

        use crate::codec::Split;

        /// websocket stream
        pub enum WsStream<S: Read + Write> {
            /// plain tcp stream
            Plain(S),
            /// tls tcp stream
            Tls(TlsStream<S>),
        }

        impl<S: Read + Write> std::fmt::Debug for WsStream<S> {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                match self {
                    Self::Plain(_) => f.debug_struct("PlainWsStream").finish(),
                    Self::Tls(_) => f.debug_struct("TlsWsStream").finish(),
                }
            }
        }

        impl<S: Read + Write> WsStream<S> {
            /// return mutable reference underlying stream
            pub fn stream_mut(&mut self) -> &mut S {
                match self {
                    WsStream::Plain(s) => s,
                    WsStream::Tls(tls) => tls.get_mut(),
                }
            }
        }

        impl<S: Read + Write> Read for WsStream<S> {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                match self {
                    WsStream::Plain(s) => s.read(buf),
                    WsStream::Tls(s) => s.read(buf),
                }
            }
        }

        impl<S: Read + Write> Write for WsStream<S> {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                match self {
                    WsStream::Plain(s) => s.write(buf),
                    WsStream::Tls(s) => s.write(buf),
                }
            }

            fn flush(&mut self) -> std::io::Result<()> {
                match self {
                    WsStream::Plain(s) => s.flush(),
                    WsStream::Tls(s) => s.flush(),
                }
            }
        }

        /// websocket readonly stream
        pub enum WsReadStream<S: Read> {
            /// plaint read stream
            Plain(S),
            /// tls wrapped read stream
            Tls(S),
        }

        impl<S: Read> Read for WsReadStream<S> {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                match self {
                    WsReadStream::Plain(s) => s.read(buf),
                    WsReadStream::Tls(s) => s.read(buf),
                }
            }
        }

        /// websocket writeable stream
        pub enum WsWriteStream<S: Write> {
            /// plain stream
            Plain(S),
            /// tls wrapped write stream
            Tls(S),
        }

        impl<S: Write> Write for WsWriteStream<S> {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                match self {
                    WsWriteStream::Plain(s) => s.write(buf),
                    WsWriteStream::Tls(s) => s.write(buf),
                }
            }

            fn flush(&mut self) -> std::io::Result<()> {
                match self {
                    WsWriteStream::Plain(s) => s.flush(),
                    WsWriteStream::Tls(s) => s.flush(),
                }
            }
        }

        impl<R, W, S> Split for WsStream<S>
        where
            R: Read,
            W: Write,
            S: Read + Write + Split<R = R, W = W>,
        {
            type R = WsReadStream<R>;

            type W = WsWriteStream<W>;

            fn split(self) -> (Self::R, Self::W) {
                match self {
                    WsStream::Plain(s) => {
                        let (read, write) = s.split();
                        (WsReadStream::Plain(read), WsWriteStream::Plain(write))
                    }
                    WsStream::Tls(s) => {
                        let sock = s.sock;
                        let (read, write) = sock.split();
                        (WsReadStream::Tls(read), WsWriteStream::Tls(write))
                    }
                }
            }
        }
    }

    #[cfg(not(feature = "sync_tls_rustls"))]
    mod stream {
        use crate::codec::Split;
        use std::io::{Read, Write};

        /// websocket stream
        pub enum WsStream<S> {
            /// plain tcp stream
            Plain(S),
        }

        impl<S: Read + Write> std::fmt::Debug for WsStream<S> {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                match self {
                    Self::Plain(_) => f.debug_struct("PlainWsStream").finish(),
                }
            }
        }

        impl<S> WsStream<S> {
            /// return mutable reference underlying stream
            pub fn stream_mut(&mut self) -> &mut S {
                match self {
                    WsStream::Plain(s) => s,
                }
            }
        }

        impl<S: Read> Read for WsStream<S> {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                match self {
                    WsStream::Plain(s) => s.read(buf),
                }
            }
        }

        impl<S: Write> Write for WsStream<S> {
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

        /// websocket readonly stream
        pub enum WsReadStream<S: Read> {
            /// plain stream
            Plain(S),
        }

        impl<S: Read> Read for WsReadStream<S> {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                match self {
                    WsReadStream::Plain(s) => s.read(buf),
                }
            }
        }

        /// websocket writeable stream
        pub enum WsWriteStream<S: Write> {
            /// plain stream
            Plain(S),
        }

        impl<S: Write> Write for WsWriteStream<S> {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                match self {
                    WsWriteStream::Plain(s) => s.write(buf),
                }
            }

            fn flush(&mut self) -> std::io::Result<()> {
                match self {
                    WsWriteStream::Plain(s) => s.flush(),
                }
            }
        }

        impl<R, W, S> Split for WsStream<S>
        where
            R: Read,
            W: Write,
            S: Read + Write + Split<R = R, W = W>,
        {
            type R = WsReadStream<R>;

            type W = WsWriteStream<W>;

            fn split(self) -> (Self::R, Self::W) {
                match self {
                    WsStream::Plain(s) => {
                        let (read, write) = s.split();
                        (WsReadStream::Plain(read), WsWriteStream::Plain(write))
                    }
                }
            }
        }
    }

    use std::io::{BufReader, BufWriter, Read, Write};

    /// a buffered stream
    pub struct BufStream<S: Read + Write>(pub BufReader<WrappedWriter<S>>);

    impl<S: Read + Write> BufStream<S> {
        /// create buf stream with default buffer size
        pub fn new(stream: S) -> Self {
            Self(BufReader::new(WrappedWriter(BufWriter::new(stream))))
        }

        /// specify buf capacity
        pub fn with_capacity(read: usize, write: usize, stream: S) -> Self {
            let writer = BufWriter::with_capacity(write, stream);
            let reader = BufReader::with_capacity(read, WrappedWriter(writer));
            Self(reader)
        }

        /// get mut ref of underlaying stream
        pub fn get_mut(&mut self) -> &mut S {
            self.0.get_mut().0.get_mut()
        }
    }

    impl<S: Read + Write> std::fmt::Debug for BufStream<S> {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("BufStream").finish()
        }
    }

    impl<S: Read + Write> Read for BufStream<S> {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            self.0.read(buf)
        }
    }
    impl<S: Read + Write> Write for BufStream<S> {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.get_mut().write(buf)
        }

        fn flush(&mut self) -> std::io::Result<()> {
            self.0.get_mut().flush()
        }
    }

    /// simple wrapper of buf writer
    pub struct WrappedWriter<S: Write>(pub BufWriter<S>);

    impl<S: Read + Write> Read for WrappedWriter<S> {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            self.0.get_mut().read(buf)
        }
    }

    impl<S: Write> Write for WrappedWriter<S> {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.write(buf)
        }

        fn flush(&mut self) -> std::io::Result<()> {
            self.0.flush()
        }
    }

    impl<S, R, W> crate::codec::Split for BufStream<S>
    where
        R: Read,
        W: Write,
        S: Read + Write + crate::codec::Split<R = R, W = W> + std::fmt::Debug,
    {
        type R = BufReader<R>;

        type W = BufWriter<W>;

        fn split(self) -> (Self::R, Self::W) {
            let read_cap = self.0.capacity();
            let write_cap = self.0.get_ref().0.capacity();
            let inner = self.0.into_inner().0.into_inner().unwrap();
            let (r, w) = inner.split();
            (
                BufReader::with_capacity(read_cap, r),
                BufWriter::with_capacity(write_cap, w),
            )
        }
    }

    pub use stream::*;
}

#[cfg(feature = "sync")]
pub use blocking::*;

#[cfg(feature = "async")]
mod non_blocking {
    #[cfg(feature = "async_tls_rustls")]
    mod ws_stream {
        use std::pin::Pin;

        use tokio::io::{AsyncRead, AsyncWrite};
        use tokio_rustls::client::TlsStream;

        use crate::codec::Split;

        /// websocket readonly async stream
        pub enum WsAsyncReadStream<S: AsyncRead> {
            /// plain stream
            Plain(S),
            /// tls wrapped stream
            Tls(S),
        }

        impl<S: AsyncRead + Unpin> AsyncRead for WsAsyncReadStream<S> {
            fn poll_read(
                self: std::pin::Pin<&mut Self>,
                cx: &mut std::task::Context<'_>,
                buf: &mut tokio::io::ReadBuf<'_>,
            ) -> std::task::Poll<std::io::Result<()>> {
                match self.get_mut() {
                    WsAsyncReadStream::Plain(s) => Pin::new(s).poll_read(cx, buf),
                    WsAsyncReadStream::Tls(s) => Pin::new(s).poll_read(cx, buf),
                }
            }
        }

        /// websocket readonly async stream
        pub enum WsAsyncWriteStream<S: AsyncWrite> {
            /// plain stream
            Plain(S),
            /// tls wrapped stream
            Tls(S),
        }

        impl<S: AsyncWrite + Unpin> AsyncWrite for WsAsyncWriteStream<S> {
            fn poll_write(
                self: std::pin::Pin<&mut Self>,
                cx: &mut std::task::Context<'_>,
                buf: &[u8],
            ) -> std::task::Poll<Result<usize, std::io::Error>> {
                match self.get_mut() {
                    WsAsyncWriteStream::Plain(s) => Pin::new(s).poll_write(cx, buf),
                    WsAsyncWriteStream::Tls(s) => Pin::new(s).poll_write(cx, buf),
                }
            }

            fn poll_flush(
                self: std::pin::Pin<&mut Self>,
                cx: &mut std::task::Context<'_>,
            ) -> std::task::Poll<Result<(), std::io::Error>> {
                match self.get_mut() {
                    WsAsyncWriteStream::Plain(s) => Pin::new(s).poll_flush(cx),
                    WsAsyncWriteStream::Tls(s) => Pin::new(s).poll_flush(cx),
                }
            }

            fn poll_shutdown(
                self: std::pin::Pin<&mut Self>,
                cx: &mut std::task::Context<'_>,
            ) -> std::task::Poll<Result<(), std::io::Error>> {
                match self.get_mut() {
                    WsAsyncWriteStream::Plain(s) => Pin::new(s).poll_shutdown(cx),
                    WsAsyncWriteStream::Tls(s) => Pin::new(s).poll_shutdown(cx),
                }
            }
        }

        impl<R, W, S> Split for WsAsyncStream<S>
        where
            R: AsyncRead,
            W: AsyncWrite,
            S: AsyncRead + AsyncWrite + Split<R = R, W = W>,
        {
            type R = WsAsyncReadStream<R>;

            type W = WsAsyncWriteStream<W>;

            fn split(self) -> (Self::R, Self::W) {
                match self {
                    WsAsyncStream::Plain(s) => {
                        let (read, write) = s.split();
                        (
                            WsAsyncReadStream::Plain(read),
                            WsAsyncWriteStream::Plain(write),
                        )
                    }
                    WsAsyncStream::Tls(s) => {
                        let s = s.into_inner().0;
                        let (read, write) = s.split();
                        (WsAsyncReadStream::Tls(read), WsAsyncWriteStream::Tls(write))
                    }
                }
            }
        }

        /// async version of websocket stream
        #[derive(Debug)]
        pub enum WsAsyncStream<S: AsyncRead + AsyncWrite> {
            /// plain tcp stream
            Plain(S),
            /// tls stream
            Tls(TlsStream<S>),
        }

        impl<S: AsyncWrite + AsyncRead> WsAsyncStream<S> {
            /// return mutable reference of underlying stream
            pub fn stream_mut(&mut self) -> &mut S {
                match self {
                    WsAsyncStream::Plain(s) => s,
                    WsAsyncStream::Tls(s) => s.get_mut().0,
                }
            }
        }

        impl<S: AsyncRead + AsyncWrite + Unpin> AsyncRead for WsAsyncStream<S> {
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

        impl<S: AsyncRead + AsyncWrite + Unpin> AsyncWrite for WsAsyncStream<S> {
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
        use std::pin::Pin;
        use tokio::io::{AsyncRead, AsyncWrite};

        use crate::codec::Split;

        /// websocket readonly async stream
        pub enum WsAsyncReadStream<S: AsyncRead> {
            /// plain stream
            Plain(S),
        }

        impl<S: AsyncRead + Unpin> AsyncRead for WsAsyncReadStream<S> {
            fn poll_read(
                self: std::pin::Pin<&mut Self>,
                cx: &mut std::task::Context<'_>,
                buf: &mut tokio::io::ReadBuf<'_>,
            ) -> std::task::Poll<std::io::Result<()>> {
                match self.get_mut() {
                    WsAsyncReadStream::Plain(s) => Pin::new(s).poll_read(cx, buf),
                }
            }
        }

        /// websocket writable async stream
        pub enum WsAsyncWriteStream<S: AsyncWrite> {
            /// plain steam
            Plain(S),
        }

        impl<S: AsyncWrite + Unpin> AsyncWrite for WsAsyncWriteStream<S> {
            fn poll_write(
                self: std::pin::Pin<&mut Self>,
                cx: &mut std::task::Context<'_>,
                buf: &[u8],
            ) -> std::task::Poll<Result<usize, std::io::Error>> {
                match self.get_mut() {
                    WsAsyncWriteStream::Plain(s) => Pin::new(s).poll_write(cx, buf),
                }
            }

            fn poll_flush(
                self: std::pin::Pin<&mut Self>,
                cx: &mut std::task::Context<'_>,
            ) -> std::task::Poll<Result<(), std::io::Error>> {
                match self.get_mut() {
                    WsAsyncWriteStream::Plain(s) => Pin::new(s).poll_flush(cx),
                }
            }

            fn poll_shutdown(
                self: std::pin::Pin<&mut Self>,
                cx: &mut std::task::Context<'_>,
            ) -> std::task::Poll<Result<(), std::io::Error>> {
                match self.get_mut() {
                    WsAsyncWriteStream::Plain(s) => Pin::new(s).poll_shutdown(cx),
                }
            }
        }

        impl<R, W, S> Split for WsAsyncStream<S>
        where
            R: AsyncRead,
            W: AsyncWrite,
            S: AsyncRead + AsyncWrite + Split<R = R, W = W>,
        {
            type R = WsAsyncReadStream<R>;

            type W = WsAsyncWriteStream<W>;

            fn split(self) -> (Self::R, Self::W) {
                match self {
                    WsAsyncStream::Plain(s) => {
                        let (read, write) = s.split();
                        (
                            WsAsyncReadStream::Plain(read),
                            WsAsyncWriteStream::Plain(write),
                        )
                    }
                }
            }
        }

        /// websocket stream
        #[derive(Debug)]
        pub enum WsAsyncStream<S> {
            /// plain tcp stream
            Plain(S),
        }

        impl<S> WsAsyncStream<S> {
            /// return mutable reference underlying stream
            pub fn stream_mut(&mut self) -> &mut S {
                match self {
                    Self::Plain(s) => s,
                }
            }
        }

        impl<S: AsyncRead + Unpin> AsyncRead for WsAsyncStream<S> {
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

        impl<S: AsyncWrite + Unpin> AsyncWrite for WsAsyncStream<S> {
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

    pub use ws_stream::*;
}

#[cfg(feature = "async")]
pub use non_blocking::*;
