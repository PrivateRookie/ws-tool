use std::{io::Write, path::PathBuf, time};

use structopt::StructOpt;
use tracing::Level;
use tracing_subscriber::util::SubscriberInitExt;
use ws_tool::{codec::AsyncWsBytesCodec, codec::AsyncWsStringCodec, ClientBuilder};

/// websocket client demo with raw frame
#[derive(StructOpt)]
struct Args {
    uri: String,

    // client size
    #[structopt(short, long, default_value = "1")]
    conn: usize,

    /// payload size kb
    #[structopt(short, long, default_value = "1")]
    payload: usize,

    /// count
    #[structopt(short, long, default_value = "5000")]
    num: usize,

    /// cert file path
    #[structopt(short, long)]
    cert: Option<PathBuf>,

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
    let mut builder = ClientBuilder::new(&args.uri);
    if let Some(cert) = args.cert {
        builder = builder.cert(cert);
    }
    if let Some(proxy) = args.proxy {
        builder = builder.proxy(&proxy)
    }
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
            let payload = vec![0].repeat(size * 1024);
            for _ in 0..total {
                client.send(&payload[..]).await.unwrap();
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
