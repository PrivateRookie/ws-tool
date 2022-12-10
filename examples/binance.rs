use clap::Parser;
use tracing::Level;
use tracing_subscriber::util::SubscriberInitExt;
use ws_tool::{codec::AsyncWsStringCodec, ClientBuilder};

/// websocket client connect to binance futures websocket
#[derive(Parser)]
struct Args {
    /// channel name, such as btcusdt@depth20
    channels: Vec<String>,

    /// http proxy port
    #[arg(long, default_value = "1088")]
    hp_port: u16,

    /// http proxy host
    #[arg(long)]
    hp_host: Option<String>,

    ///http proxy auth fmt -> username:password
    #[arg(long)]
    hp_auth: Option<String>,
    /// socks5 proxy port
    #[arg(long, default_value = "1087")]
    sp_port: u16,

    /// socks5 proxy host
    #[arg(long)]
    sp_host: Option<String>,

    /// socks5 proxy auth fmt -> username:password
    #[arg(long)]
    sp_auth: Option<String>,
}

#[tokio::main]
async fn main() -> Result<(), ()> {
    tracing_subscriber::fmt::fmt()
        .with_max_level(Level::INFO)
        .finish()
        .try_init()
        .expect("failed to init log");
    let args = Args::parse();
    let channels = args.channels.join("/");
    let mut builder = ClientBuilder::new(&format!(
        "wss://fstream.binance.com/stream?streams={}",
        channels
    ));
    if let Some(host) = args.hp_host {
        let auth = args
            .hp_auth
            .map(|auth| {
                let (user, passwd) = auth.split_once(':').expect("invalid auth format");
                hproxy::AuthCredential::Basic {
                    user: user.trim().into(),
                    passwd: passwd.trim().into(),
                }
            })
            .unwrap_or(hproxy::AuthCredential::None);

        builder = builder.http_proxy(hproxy::ProxyConfig {
            host,
            port: args.hp_port,
            auth,
            keep_alive: true,
        })
    }
    if let Some(host) = args.sp_host {
        let auth = args
            .sp_auth
            .map(|auth| {
                let (user, passwd) = auth.split_once(':').expect("invalid auth format");
                sproxy::AuthCredential::Basic {
                    user: user.trim().into(),
                    passwd: passwd.trim().into(),
                }
            })
            .unwrap_or(sproxy::AuthCredential::None);

        builder = builder.socks5_proxy(sproxy::ProxyConfig {
            host,
            port: args.sp_port,
            auth,
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
