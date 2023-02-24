mod binary;
#[cfg(any(
    feature = "deflate",
    feature = "deflate_ng",
    feature = "deflate_static"
))]
mod deflate;
mod frame;
mod text;

pub use binary::*;
#[cfg(any(
    feature = "deflate",
    feature = "deflate_ng",
    feature = "deflate_static"
))]
pub use deflate::*;
pub use frame::*;
pub use text::*;

/// split something into two parts
pub trait Split {
    /// read half type
    type R;
    /// write half type
    type W;
    /// consume and return parts
    fn split(self) -> (Self::R, Self::W);
}

#[cfg(feature = "sync")]
mod blocking {
    use super::Split;
    use std::{
        io::{Read, Write},
        net::TcpStream,
    };

    // impl<R: Read> Read for ReadHalf<R> {
    //     fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
    //         self.inner.read(buf)
    //     }
    // }

    // impl<W: Write> Write for WriteHalf<W> {
    //     fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
    //         self.inner.write(buf)
    //     }

    //     fn flush(&mut self) -> std::io::Result<()> {
    //         self.inner.flush()
    //     }
    // }

    pub struct TcpReadHalf(pub TcpStream);

    impl Read for TcpReadHalf {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            self.0.read(buf)
        }
    }

    pub struct TcpWriteHalf(pub TcpStream);

    impl Write for TcpWriteHalf {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.write(buf)
        }

        fn flush(&mut self) -> std::io::Result<()> {
            self.0.flush()
        }
    }

    impl Split for TcpStream {
        type R = TcpStream;
        type W = TcpStream;
        fn split(self) -> (Self::R, Self::W) {
            let cloned = self.try_clone().expect("failed to split tcp stream");
            (self, cloned)
        }
    }
}

#[cfg(feature = "async")]
mod non_blocking {
    use tokio::net::{
        tcp::{OwnedReadHalf, OwnedWriteHalf},
        TcpStream,
    };

    use super::Split;

    // impl<R: AsyncRead + Unpin> AsyncRead for ReadHalf<R> {
    //     fn poll_read(
    //         mut self: std::pin::Pin<&mut Self>,
    //         cx: &mut std::task::Context<'_>,
    //         buf: &mut tokio::io::ReadBuf<'_>,
    //     ) -> std::task::Poll<std::io::Result<()>> {
    //         std::pin::Pin::new(self.inner_mut()).poll_read(cx, buf)
    //     }
    // }

    // impl<W: AsyncWrite + Unpin> AsyncWrite for WriteHalf<W> {
    //     fn poll_write(
    //         mut self: std::pin::Pin<&mut Self>,
    //         cx: &mut std::task::Context<'_>,
    //         buf: &[u8],
    //     ) -> std::task::Poll<Result<usize, std::io::Error>> {
    //         std::pin::Pin::new(self.inner_mut()).poll_write(cx, buf)
    //     }

    //     fn poll_flush(
    //         mut self: std::pin::Pin<&mut Self>,
    //         cx: &mut std::task::Context<'_>,
    //     ) -> std::task::Poll<Result<(), std::io::Error>> {
    //         std::pin::Pin::new(self.inner_mut()).poll_flush(cx)
    //     }

    //     fn poll_shutdown(
    //         mut self: std::pin::Pin<&mut Self>,
    //         cx: &mut std::task::Context<'_>,
    //     ) -> std::task::Poll<Result<(), std::io::Error>> {
    //         std::pin::Pin::new(self.inner_mut()).poll_shutdown(cx)
    //     }
    // }

    impl Split for TcpStream {
        type R = OwnedReadHalf;

        type W = OwnedWriteHalf;

        fn split(self) -> (Self::R, Self::W) {
            self.into_split()
        }
    }
}
