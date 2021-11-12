use futures::SinkExt;
use structopt::StructOpt;
use tokio::{
    fs::File,
    io::{
        stdin, AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader, BufStream,
        ReadHalf, WriteHalf,
    },
};
use tokio_stream::StreamExt;
use tokio_util::codec::{Decoder, Encoder, FramedRead, FramedWrite};
use ws_tool::{
    frame::{Frame, FrameDecoder, FrameEncoder},
    stream::WsStream,
    ConnBuilder,
};

/// websocket client connect to binance futures websocket
#[derive(StructOpt)]
struct Args {
    /// channel name, such as btcusdt@depth20
    channels: Vec<String>,

    /// proxy setting
    #[structopt(long)]
    proxy: Option<String>,
}

#[tokio::main]
async fn main() -> Result<(), ()> {
    pretty_env_logger::init();
    let args = Args::from_args();
    let channels = args.channels.join("/");
    let mut builder = ConnBuilder::new(&format!(
        "wss://fstream.binance.com/stream?streams={}",
        channels
    ));
    if let Some(proxy) = args.proxy {
        builder = builder.proxy(&proxy)
    }
    let mut client = builder.build().await.unwrap();
    client.handshake().await.unwrap();
    let (read, write) = client.split();
    let (r, w) = tokio::join!(tokio::spawn(read_std(write)), tokio::spawn(read_msg(read)));
    r.unwrap();
    w.unwrap();
    Ok(())
}

async fn read_msg(read: FramedRead<ReadHalf<WsStream>, FrameDecoder>) {
    println!("start recving");
    let mut read = read;
    let mut file = File::create("output.json").await.unwrap();
    while let Some(Ok(frame)) = read.next().await {
        let msg = String::from_utf8(frame.payload_data_unmask().to_vec()).unwrap();
        file.write_all(msg.as_bytes()).await.unwrap();
        file.flush().await.unwrap();
        // println!("{}", msg.trim());
    }
}

async fn read_std(write: FramedWrite<WriteHalf<WsStream>, FrameEncoder>) {
    let mut write = write;
    let mut stdin = BufReader::new(stdin());
    let mut input = String::new();
    loop {
        input.clear();
        stdin.read_line(&mut input).await.unwrap();
        println!("{}", input);
        let f = Frame::new_with_payload(ws_tool::frame::OpCode::Text, input.as_bytes());
        write.send(f).await.unwrap();
    }
}
