use structopt::StructOpt;
use tracing::Level;
use tracing_subscriber::util::SubscriberInitExt;
use ws_tool::{codec::AsyncWsStringCodec, ClientBuilder};

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
        .async_connect(AsyncWsStringCodec::check_fn)
        .await
        .unwrap();

    while let Ok(msg) = client.receive().await {
        println!("{}", msg.data.trim());
    }
    Ok(())
}
