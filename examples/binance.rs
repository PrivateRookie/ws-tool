use structopt::StructOpt;
use tracing::Level;
use tracing_subscriber::util::SubscriberInitExt;
use ws_tool::{codec::AsyncWsStringCodec, ClientBuilder};

/// websocket client connect to binance futures websocket
#[derive(StructOpt)]
struct Args {
    /// channel name, such as btcusdt@depth20
    channels: Vec<String>,

    /// proxy port
    #[structopt(long, default_value = "1088")]
    proxy_port: u16,

    /// proxy host
    #[structopt(long)]
    proxy_host: Option<String>,
    
    /// proxy auth fmt -> username:password
    #[structopt(long)]
    proxy_auth: Option<String>,
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
    if let Some(host) = args.proxy_host {
        let auth = args
            .proxy_auth
            .map(|auth| {
                let (user, passwd) = auth.split_once(":").expect("invalid auth format");
                hproxy::AuthCredential::Basic {
                    user: user.trim().into(),
                    passwd: passwd.trim().into(),
                }
            })
            .unwrap_or(hproxy::AuthCredential::None);
        builder = builder.http_proxy(hproxy::ProxyConfig {
            host,
            port: args.proxy_port,
            auth,
            keep_alive: true,
        })
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
