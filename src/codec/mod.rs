use crate::errors::WsError;
use crate::stream::WsStream;
use tokio::io::{ReadHalf, WriteHalf};
use tokio_util::codec::{Decoder, Encoder, FramedRead, FramedWrite};

mod binary;
mod frame;
mod text;
#[cfg(feature="deflate")]
mod deflate;
pub use binary::*;
pub use frame::*;
pub use text::*;
#[cfg(feature="deflate")]
pub use deflate::*;
pub trait SplitSocket<EI, DI, E, D>
where
    E: Encoder<EI, Error = WsError>,
    D: Decoder<Item = DI, Error = WsError>,
{
    fn split(
        self,
    ) -> (
        FramedRead<ReadHalf<WsStream>, D>,
        FramedWrite<WriteHalf<WsStream>, E>,
    );
}
