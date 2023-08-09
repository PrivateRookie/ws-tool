#[cfg(feature = "sync")]
mod blocking {
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
    use tokio::io::{AsyncRead, AsyncWrite};

    #[allow(missing_docs)]
    pub trait AsyncRW: AsyncRead + AsyncWrite + Unpin {}

    impl<S: AsyncRead + AsyncWrite + Unpin> AsyncRW for S {}
}

#[cfg(feature = "async")]
pub use non_blocking::*;
