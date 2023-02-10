use std::{io::Write, path::PathBuf};

use clap::Parser;
use http::Uri;
use tokio::net::TcpStream;
use tracing::Level;
use tracing_subscriber::util::SubscriberInitExt;
use ws_tool::{codec::AsyncStringCodec, ClientBuilder};

/// websocket client demo with raw frame
#[derive(Parser)]
struct Args {
    uri: Uri,
    /// cert file path
    #[arg(short, long)]
    cert: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> Result<(), ()> {
    tracing_subscriber::fmt::fmt()
        .with_max_level(Level::DEBUG)
        .finish()
        .try_init()
        .expect("failed to init log");
    let args = Args::parse();

    let builder = ClientBuilder::new();
    let stream = TcpStream::connect((args.uri.host().unwrap(), args.uri.port_u16().unwrap()))
        .await
        .unwrap();
    let mut client = builder
        .async_connect(
            args.uri.to_string().try_into().unwrap(),
            stream,
            AsyncStringCodec::check_fn,
        )
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
        client.send(input.clone()).await.unwrap();
        match client.receive().await {
            Ok(item) => {
                println!("[RECV] > {}", item.data.trim());
                if item.data == "quit" {
                    break;
                }
                client.send((1000u16, "done".into())).await.ok();
                input.clear()
            }
            Err(e) => {
                dbg!(e);
                break;
            }
        }
    }
    Ok(())
}
