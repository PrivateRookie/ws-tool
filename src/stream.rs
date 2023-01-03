#[cfg(feature = "sync")]
mod blocking {

    mod stream {
        use std::io::{Read, Write};

        use crate::codec::Split;

        /// websocket stream
        pub struct WsStream<S: Read + Write>(pub(crate) S);

        impl<S: Read + Write> std::fmt::Debug for WsStream<S> {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.debug_struct("WsStream").finish()
            }
        }

        impl<S: Read + Write> WsStream<S> {
            /// create new ws stream
            pub fn new(stream: S) -> Self {
                Self(stream)
            }

            /// return mutable reference underlying stream
            pub fn stream_mut(&mut self) -> &mut S {
                &mut self.0
            }

            /// get immutable ref of underlying stream
            pub fn stream(&self) -> &S {
                &self.0
            }
        }

        impl<S: Read + Write> Read for WsStream<S> {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                self.0.read(buf)
            }
        }

        impl<S: Read + Write> Write for WsStream<S> {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                self.0.write(buf)
            }

            fn flush(&mut self) -> std::io::Result<()> {
                self.0.flush()
            }
        }

        /// websocket readonly stream
        pub struct WsReadStream<S: Read>(pub(crate) S);

        impl<S: Read> Read for WsReadStream<S> {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                self.0.read(buf)
            }
        }

        /// websocket writeable stream
        pub struct WsWriteStream<S: Write>(pub(crate) S);

        impl<S: Write> Write for WsWriteStream<S> {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                self.0.write(buf)
            }

            fn flush(&mut self) -> std::io::Result<()> {
                self.0.flush()
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
                let (read, write) = self.0.split();
                (WsReadStream(read), WsWriteStream(write))
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
    mod ws_stream {
        use std::pin::Pin;

        use tokio::io::{AsyncRead, AsyncWrite};

        use crate::codec::Split;

        /// websocket readonly async stream
        pub struct WsAsyncReadStream<S: AsyncRead>(pub(crate) S);

        impl<S: AsyncRead + Unpin> AsyncRead for WsAsyncReadStream<S> {
            fn poll_read(
                self: Pin<&mut Self>,
                cx: &mut std::task::Context<'_>,
                buf: &mut tokio::io::ReadBuf<'_>,
            ) -> std::task::Poll<std::io::Result<()>> {
                Pin::new(&mut self.get_mut().0).poll_read(cx, buf)
            }
        }

        /// websocket readonly async stream
        pub struct WsAsyncWriteStream<S: AsyncWrite>(pub(crate) S);

        impl<S: AsyncWrite + Unpin> AsyncWrite for WsAsyncWriteStream<S> {
            fn poll_write(
                self: std::pin::Pin<&mut Self>,
                cx: &mut std::task::Context<'_>,
                buf: &[u8],
            ) -> std::task::Poll<Result<usize, std::io::Error>> {
                Pin::new(&mut self.get_mut().0).poll_write(cx, buf)
            }

            fn poll_flush(
                self: std::pin::Pin<&mut Self>,
                cx: &mut std::task::Context<'_>,
            ) -> std::task::Poll<Result<(), std::io::Error>> {
                Pin::new(&mut self.get_mut().0).poll_flush(cx)
            }

            fn poll_shutdown(
                self: std::pin::Pin<&mut Self>,
                cx: &mut std::task::Context<'_>,
            ) -> std::task::Poll<Result<(), std::io::Error>> {
                Pin::new(&mut self.get_mut().0).poll_shutdown(cx)
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
                let (read, write) = self.0.split();
                (WsAsyncReadStream(read), WsAsyncWriteStream(write))
            }
        }

        /// async version of websocket stream
        #[derive(Debug)]
        pub struct WsAsyncStream<S: AsyncRead + AsyncWrite>(pub(crate) S);
        impl<S: AsyncWrite + AsyncRead> WsAsyncStream<S> {
            /// create new ws async stream
            pub fn new(stream: S) -> Self {
                Self(stream)
            }

            /// return mutable reference of underlying stream
            pub fn stream_mut(&mut self) -> &mut S {
                &mut self.0
            }

            /// get immutable ref of underlying stream
            pub fn stream(&self) -> &S {
                &self.0
            }
        }

        impl<S: AsyncRead + AsyncWrite + Unpin> AsyncRead for WsAsyncStream<S> {
            fn poll_read(
                self: Pin<&mut Self>,
                cx: &mut std::task::Context<'_>,
                buf: &mut tokio::io::ReadBuf<'_>,
            ) -> std::task::Poll<std::io::Result<()>> {
                Pin::new(&mut self.get_mut().0).poll_read(cx, buf)
            }
        }

        impl<S: AsyncRead + AsyncWrite + Unpin> AsyncWrite for WsAsyncStream<S> {
            fn poll_write(
                self: std::pin::Pin<&mut Self>,
                cx: &mut std::task::Context<'_>,
                buf: &[u8],
            ) -> std::task::Poll<Result<usize, std::io::Error>> {
                Pin::new(&mut self.get_mut().0).poll_write(cx, buf)
            }

            fn poll_flush(
                self: std::pin::Pin<&mut Self>,
                cx: &mut std::task::Context<'_>,
            ) -> std::task::Poll<Result<(), std::io::Error>> {
                Pin::new(&mut self.get_mut().0).poll_flush(cx)
            }

            fn poll_shutdown(
                self: std::pin::Pin<&mut Self>,
                cx: &mut std::task::Context<'_>,
            ) -> std::task::Poll<Result<(), std::io::Error>> {
                Pin::new(&mut self.get_mut().0).poll_shutdown(cx)
            }
        }
    }

    pub use ws_stream::*;
}

#[cfg(feature = "async")]
pub use non_blocking::*;
