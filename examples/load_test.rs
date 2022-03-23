use std::{path::PathBuf, time};

use clap::Parser;
use tracing::Level;
use tracing_subscriber::util::SubscriberInitExt;
use ws_tool::{codec::AsyncWsBytesCodec, ClientBuilder};

/// websocket client demo with raw frame
#[derive(Parser)]
struct Args {
    uri: String,

    // client size
    #[clap(long, default_value = "1")]
    conn: usize,

    /// payload size kb
    #[clap(short, long, default_value = "1")]
    payload: usize,

    /// count
    #[clap(short, long, default_value = "5000")]
    num: usize,

    /// cert file path
    #[clap(short, long)]
    cert: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> Result<(), ()> {
    tracing_subscriber::fmt::fmt()
        .with_max_level(Level::INFO)
        .finish()
        .try_init()
        .expect("failed to init log");
    let args = Args::parse();
    let mut builder = ClientBuilder::new(&args.uri);
    if let Some(cert) = args.cert {
        builder = builder.cert(cert);
    }
    // if let Some(proxy) = args.proxy {
    //     builder = builder.proxy(&proxy)
    // }
    let total = args.num;
    let size = args.payload;
    let mut f = vec![];
    for _ in 0..args.conn {
        let mut client = builder
            .async_connect(AsyncWsBytesCodec::check_fn)
            .await
            .unwrap();
        let j = tokio::spawn(async move {
            let start = time::SystemTime::now();
            let mut payload = vec![0].repeat(size * 1024);
            for _ in 0..total {
                client.send(&mut payload[..]).await.unwrap();
                client.receive().await.unwrap();
            }
            let end = time::SystemTime::now();
            let elapse = end.duration_since(start);
            elapse
        });
        f.push(j)
    }
    let all = futures::future::join_all(f).await;
    for i in all {
        println!("{}", total as f64 / i.unwrap().unwrap().as_secs_f64())
    }
    Ok(())
}
