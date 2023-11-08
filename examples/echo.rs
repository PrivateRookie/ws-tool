use std::{io::Write, path::PathBuf};

use clap::Parser;
use tracing::Level;
use tracing_subscriber::util::SubscriberInitExt;
use ws_tool::{codec::AsyncStringCodec, ClientConfig};

/// websocket client demo with raw frame
#[derive(Parser)]
struct Args {
    uri: String,
    /// cert file path
    #[arg(short, long)]
    cert: Option<Vec<PathBuf>>,
}

#[tokio::main]
async fn main() -> Result<(), ()> {
    run().await
}

async fn run() -> Result<(), ()> {
    tracing_subscriber::fmt::fmt()
        .with_max_level(Level::TRACE)
        .finish()
        .try_init()
        .expect("failed to init log");

    let args = Args::parse();
    let mut client = ClientConfig {
        certs: args.cert.unwrap_or_default(),
        ..Default::default()
    }
    .async_connect_with(args.uri, AsyncStringCodec::check_fn)
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
        client.send(&input).await.unwrap();
        match client.receive().await {
            Ok(item) => {
                println!("[RECV] > {}", item.data.trim());
                if item.data == "quit" {
                    client.close(1000, "bye").await.ok();
                    break;
                }
                input.clear()
            }
            Err(e) => {
                dbg!(e);
                break;
            }
        }
    }
    client.send((1000u16, "done")).await.ok();
    Ok(())
}
