use futures::SinkExt;
use structopt::StructOpt;
use tokio::{
    fs::File,
    io::{stdin, AsyncBufReadExt, AsyncWriteExt, BufReader, ReadHalf, WriteHalf},
};
use tokio_stream::StreamExt;
use tokio_util::codec::{FramedRead, FramedWrite};
use tracing::Level;
use tracing_subscriber::util::SubscriberInitExt;
use ws_tool::{
    codec::{default_string_check_fn, SplitSocket, WebSocketStringDecoder, WebSocketStringEncoder},
    stream::WsStream,
    ClientBuilder,
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
    tracing_subscriber::fmt::fmt()
        .with_max_level(Level::INFO)
        .finish()
        .try_init()
        .expect("failed to init log");
    let args = Args::from_args();
    let channels = args.channels.join("/");
    let mut builder = ClientBuilder::new(&format!(
        "wss://fstream.binance.com/stream?streams={}",
        channels
    ));
    if let Some(proxy) = args.proxy {
        builder = builder.proxy(&proxy)
    }
    let client = builder
        .connect_with_check(default_string_check_fn)
        .await
        .unwrap();
    let (read, write) = client.split();
    let (r, w) = tokio::join!(tokio::spawn(read_std(write)), tokio::spawn(read_msg(read)));
    r.unwrap();
    w.unwrap();
    Ok(())
}

async fn read_msg(read: FramedRead<ReadHalf<WsStream>, WebSocketStringDecoder>) {
    println!("start recving");
    let mut read = read;
    let mut file = File::create("output.json").await.unwrap();
    while let Some(Ok((_, msg))) = read.next().await {
        file.write_all(msg.as_bytes()).await.unwrap();
        file.flush().await.unwrap();
        // println!("{}", msg.trim());
    }
}

async fn read_std(write: FramedWrite<WriteHalf<WsStream>, WebSocketStringEncoder>) {
    let mut write = write;
    let mut stdin = BufReader::new(stdin());
    let mut input = String::new();
    loop {
        input.clear();
        stdin.read_line(&mut input).await.unwrap();
        println!("{}", input);
        write.send(input.clone()).await.unwrap();
        input.clear();
    }
}
