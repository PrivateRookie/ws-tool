use std::{io::Write, iter::FromIterator, path::PathBuf};

use bytes::BytesMut;
use futures::SinkExt;
use structopt::StructOpt;
use tokio_stream::StreamExt;
use tracing::Level;
use tracing_subscriber::util::SubscriberInitExt;
use ws_tool::{
    codec::{default_deflate_check_fn, default_string_check_fn, DeflateConfig},
    frame::OpCode,
    ClientBuilder,
};

/// websocket client demo with raw frame
#[derive(StructOpt)]
struct Args {
    uri: String,
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
        .with_max_level(Level::DEBUG)
        .finish()
        .try_init()
        .expect("failed to init log");
    let args = Args::from_args();
    let mut builder =
        ClientBuilder::new(&args.uri).extension(DeflateConfig::default().build_header());
    if let Some(cert) = args.cert {
        builder = builder.cert(cert);
    }
    if let Some(proxy) = args.proxy {
        builder = builder.proxy(&proxy)
    }
    let mut client = builder
        .connect_with_check(default_deflate_check_fn)
        .await
        .unwrap();

    let mut input = String::new();
    loop {
        print!("[SEND] > ");
        std::io::stdout().flush().unwrap();
        std::io::stdin().read_line(&mut input).unwrap();
        if &input == "quit\n" {
            break;
        }
        client
            .send((OpCode::Text, BytesMut::from_iter(input.trim().as_bytes())))
            .await
            .unwrap();
        if let Some(Ok((code, msg))) = client.next().await {
            if code == OpCode::Close {
                break;
            }
            tracing::info!("{:?}", msg);
            println!("[RECV] > {}", String::from_utf8(msg[..].to_vec()).unwrap());
            if &msg == "quit" {
                break;
            }
            input.clear()
        } else {
            break;
        }
    }
    Ok(())
}
