#[cfg(feature = "sync")]
mod blocking {
    use std::{
        io::{BufReader, BufWriter, Read, Write},
        net::TcpStream,
    };

    use crate::codec::Split;
    #[allow(missing_docs)]
    pub trait RW: Read + Write {}

    impl<S: Read + Write> RW for S {}

    #[cfg(any(feature = "sync_tls_rustls", feature = "sync_tls_native"))]
    mod split {
        use std::{
            io::{ErrorKind, Read, Write},
            sync::{Arc, Mutex},
        };

        use crate::codec::Split;

        /// reader part of a stream
        pub struct ReadHalf<T> {
            /// inner stream
            pub inner: Arc<Mutex<T>>,
        }

        /// writer part of a stream
        pub struct WriteHalf<T> {
            /// inner stream
            pub inner: Arc<Mutex<T>>,
        }

        macro_rules! try_lock {
            ($lock:expr) => {
                match $lock.lock() {
                    Ok(guard) => guard,
                    Err(_) => {
                        return Err(std::io::Error::new(
                            ErrorKind::BrokenPipe,
                            format!("lock poisoned"),
                        ));
                    }
                }
            };
        }

        impl<T: Read> Read for ReadHalf<T> {
            fn read_vectored(
                &mut self,
                bufs: &mut [std::io::IoSliceMut<'_>],
            ) -> std::io::Result<usize> {
                try_lock!(self.inner).read_vectored(bufs)
            }

            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                try_lock!(self.inner).read(buf)
            }
        }

        impl<T: Write> Write for WriteHalf<T> {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                try_lock!(self.inner).write(buf)
            }

            fn flush(&mut self) -> std::io::Result<()> {
                try_lock!(self.inner).flush()
            }
        }

        #[cfg(feature = "sync_tls_rustls")]
        impl<S: Read + Write> Split for rustls_connector::TlsStream<S> {
            type R = ReadHalf<rustls_connector::TlsStream<S>>;

            type W = WriteHalf<rustls_connector::TlsStream<S>>;

            fn split(self) -> (Self::R, Self::W) {
                let inner = Arc::new(Mutex::new(self));
                let inner_c = inner.clone();
                (ReadHalf { inner }, WriteHalf { inner: inner_c })
            }
        }

        #[cfg(feature = "sync_tls_native")]
        impl<S: Read + Write> Split for native_tls::TlsStream<S> {
            type R = ReadHalf<native_tls::TlsStream<S>>;

            type W = WriteHalf<native_tls::TlsStream<S>>;

            fn split(self) -> (Self::R, Self::W) {
                let inner = Arc::new(Mutex::new(self));
                let inner_c = inner.clone();
                (ReadHalf { inner }, WriteHalf { inner: inner_c })
            }
        }
    }

    macro_rules! def {
        ($name:ident, $raw:ty, $rustls:ty, $native:ty, $doc:literal) => {
            #[doc=$doc]
            pub enum $name {
                /// raw tcp stream
                Raw($raw),
                /// rustls wrapped stream
                #[cfg(feature = "sync_tls_rustls")]
                Rustls($rustls),
                /// native tls wrapped stream
                #[cfg(feature = "sync_tls_native")]
                NativeTls($native),
            }

            impl std::fmt::Debug for $name {
                fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    match self {
                        Self::Raw(_) => f.debug_tuple("Raw").finish(),
                        #[cfg(feature = "sync_tls_rustls")]
                        Self::Rustls(_) => f.debug_tuple("Rustls").finish(),
                        #[cfg(feature = "sync_tls_native")]
                        Self::NativeTls(_) => f.debug_tuple("NativeTls").finish(),
                    }
                }
            }
        };
    }

    def!(
        SyncStreamRead,
        TcpStream,
        split::ReadHalf<rustls_connector::TlsStream<TcpStream>>,
        split::ReadHalf<native_tls::TlsStream<TcpStream>>,
        "a wrapper of most common use raw/ssl tcp based stream"
    );

    def!(
        SyncStreamWrite,
        TcpStream,
        split::WriteHalf<rustls_connector::TlsStream<TcpStream>>,
        split::WriteHalf<native_tls::TlsStream<TcpStream>>,
        "a wrapper of most common use raw/ssl tcp based stream"
    );

    def!(
        SyncStream,
        TcpStream,
        rustls_connector::TlsStream<TcpStream>,
        native_tls::TlsStream<TcpStream>,
        "a wrapper of most common use raw/ssl tcp based stream"
    );

    macro_rules! impl_read {
        ($name:ty) => {
            impl Read for $name {
                fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                    match self {
                        Self::Raw(s) => s.read(buf),
                        #[cfg(feature = "sync_tls_rustls")]
                        Self::Rustls(s) => s.read(buf),
                        #[cfg(feature = "sync_tls_native")]
                        Self::NativeTls(s) => s.read(buf),
                    }
                }

                fn read_vectored(
                    &mut self,
                    bufs: &mut [std::io::IoSliceMut<'_>],
                ) -> std::io::Result<usize> {
                    match self {
                        Self::Raw(s) => s.read_vectored(bufs),
                        #[cfg(feature = "sync_tls_rustls")]
                        Self::Rustls(s) => s.read_vectored(bufs),
                        #[cfg(feature = "sync_tls_native")]
                        Self::NativeTls(s) => s.read_vectored(bufs),
                    }
                }
            }
        };
    }

    impl_read!(SyncStream);
    impl_read!(SyncStreamRead);

    macro_rules! impl_write {
        ($item:ty) => {
            impl Write for $item {
                fn write_vectored(
                    &mut self,
                    bufs: &[std::io::IoSlice<'_>],
                ) -> std::io::Result<usize> {
                    match self {
                        Self::Raw(s) => s.write_vectored(bufs),
                        #[cfg(feature = "sync_tls_rustls")]
                        Self::Rustls(s) => s.write_vectored(bufs),
                        #[cfg(feature = "sync_tls_native")]
                        Self::NativeTls(s) => s.write_vectored(bufs),
                    }
                }

                fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                    match self {
                        Self::Raw(s) => s.write(buf),
                        #[cfg(feature = "sync_tls_rustls")]
                        Self::Rustls(s) => s.write(buf),
                        #[cfg(feature = "sync_tls_native")]
                        Self::NativeTls(s) => s.write(buf),
                    }
                }

                fn flush(&mut self) -> std::io::Result<()> {
                    match self {
                        Self::Raw(s) => s.flush(),
                        #[cfg(feature = "sync_tls_rustls")]
                        Self::Rustls(s) => s.flush(),
                        #[cfg(feature = "sync_tls_native")]
                        Self::NativeTls(s) => s.flush(),
                    }
                }
            }
        };
    }

    impl_write!(SyncStream);
    impl_write!(SyncStreamWrite);

    impl Split for SyncStream {
        type R = SyncStreamRead;

        type W = SyncStreamWrite;

        fn split(self) -> (Self::R, Self::W) {
            match self {
                Self::Raw(s) => {
                    let (read, write) = s.split();
                    (SyncStreamRead::Raw(read), SyncStreamWrite::Raw(write))
                }
                #[cfg(feature = "sync_tls_rustls")]
                Self::Rustls(s) => {
                    let s = std::sync::Arc::new(std::sync::Mutex::new(s));
                    (
                        SyncStreamRead::Rustls(split::ReadHalf { inner: s.clone() }),
                        SyncStreamWrite::Rustls(split::WriteHalf { inner: s }),
                    )
                }
                #[cfg(feature = "sync_tls_native")]
                Self::NativeTls(s) => {
                    let s = std::sync::Arc::new(std::sync::Mutex::new(s));
                    (
                        SyncStreamRead::NativeTls(split::ReadHalf { inner: s.clone() }),
                        SyncStreamWrite::NativeTls(split::WriteHalf { inner: s }),
                    )
                }
            }
        }
    }

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
        fn read_vectored(
            &mut self,
            bufs: &mut [std::io::IoSliceMut<'_>],
        ) -> std::io::Result<usize> {
            self.0.read_vectored(bufs)
        }

        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            self.0.read(buf)
        }
    }
    impl<S: Read + Write> Write for BufStream<S> {
        fn write_vectored(&mut self, bufs: &[std::io::IoSlice<'_>]) -> std::io::Result<usize> {
            self.0.get_mut().write_vectored(bufs)
        }

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
        fn read_vectored(
            &mut self,
            bufs: &mut [std::io::IoSliceMut<'_>],
        ) -> std::io::Result<usize> {
            self.0.get_mut().read_vectored(bufs)
        }

        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            self.0.get_mut().read(buf)
        }
    }

    impl<S: Write> Write for WrappedWriter<S> {
        fn write_vectored(&mut self, bufs: &[std::io::IoSlice<'_>]) -> std::io::Result<usize> {
            self.0.write_vectored(bufs)
        }

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
}

#[cfg(feature = "sync")]
pub use blocking::*;

#[cfg(feature = "async")]
mod non_blocking {
    use std::pin::Pin;

    use tokio::{
        io::{AsyncRead, AsyncWrite, ReadHalf, WriteHalf},
        net::TcpStream,
    };

    use crate::codec::Split;

    #[allow(missing_docs)]
    pub trait AsyncRW: AsyncRead + AsyncWrite + Unpin {}

    impl<S: AsyncRead + AsyncWrite + Unpin> AsyncRW for S {}

    /// a wrapper of most common use raw/ssl tcp based stream
    pub enum AsyncStream {
        /// raw tcp stream
        Raw(TcpStream),
        /// rustls wrapped stream
        #[cfg(feature = "async_tls_rustls")]
        Rustls(tokio_rustls::TlsStream<TcpStream>),
        /// native tls wrapped stream
        #[cfg(feature = "async_tls_native")]
        NativeTls(tokio_native_tls::TlsStream<TcpStream>),
    }

    impl Split for AsyncStream {
        type R = ReadHalf<Self>;

        type W = WriteHalf<Self>;

        fn split(self) -> (Self::R, Self::W) {
            tokio::io::split(self)
        }
    }

    impl AsyncRead for AsyncStream {
        fn poll_read(
            self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
            buf: &mut tokio::io::ReadBuf<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            match self.get_mut() {
                AsyncStream::Raw(s) => std::pin::Pin::new(s).poll_read(cx, buf),
                #[cfg(feature = "async_tls_rustls")]
                AsyncStream::Rustls(s) => std::pin::Pin::new(s).poll_read(cx, buf),
                #[cfg(feature = "async_tls_native")]
                AsyncStream::NativeTls(s) => std::pin::Pin::new(s).poll_read(cx, buf),
            }
        }
    }

    impl AsyncWrite for AsyncStream {
        fn poll_write(
            self: Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
            buf: &[u8],
        ) -> std::task::Poll<Result<usize, std::io::Error>> {
            match self.get_mut() {
                AsyncStream::Raw(s) => std::pin::Pin::new(s).poll_write(cx, buf),
                #[cfg(feature = "async_tls_rustls")]
                AsyncStream::Rustls(s) => std::pin::Pin::new(s).poll_write(cx, buf),
                #[cfg(feature = "async_tls_native")]
                AsyncStream::NativeTls(s) => std::pin::Pin::new(s).poll_write(cx, buf),
            }
        }

        fn poll_flush(
            self: Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Result<(), std::io::Error>> {
            match self.get_mut() {
                AsyncStream::Raw(s) => std::pin::Pin::new(s).poll_flush(cx),
                #[cfg(feature = "async_tls_rustls")]
                AsyncStream::Rustls(s) => std::pin::Pin::new(s).poll_flush(cx),
                #[cfg(feature = "async_tls_native")]
                AsyncStream::NativeTls(s) => std::pin::Pin::new(s).poll_flush(cx),
            }
        }

        fn poll_shutdown(
            self: Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Result<(), std::io::Error>> {
            match self.get_mut() {
                AsyncStream::Raw(s) => std::pin::Pin::new(s).poll_shutdown(cx),
                #[cfg(feature = "async_tls_rustls")]
                AsyncStream::Rustls(s) => std::pin::Pin::new(s).poll_shutdown(cx),
                #[cfg(feature = "async_tls_native")]
                AsyncStream::NativeTls(s) => std::pin::Pin::new(s).poll_shutdown(cx),
            }
        }
    }
}

#[cfg(feature = "async")]
pub use non_blocking::*;
