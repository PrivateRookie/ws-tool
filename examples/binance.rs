use structopt::StructOpt;
use tokio_stream::StreamExt;
use tracing::Level;
use tracing_subscriber::util::SubscriberInitExt;
use ws_tool::{codec::default_string_check_fn, ClientBuilder};

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
        .with_max_level(Level::DEBUG)
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
    let mut client = builder
        .async_connect(default_string_check_fn)
        .await
        .unwrap();

    while let Some(Ok((_, msg))) = client.next().await {
        println!("{}", msg.trim());
    }
    Ok(())
}
