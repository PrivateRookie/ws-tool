use std::{io::Write, path::PathBuf};

use clap::Parser;
use http::Uri;
use tracing::Level;
use tracing_subscriber::util::SubscriberInitExt;
use ws_tool::{
    codec::AsyncStringCodec,
    connector::{async_tcp_connect, async_wrap_rustls, get_host, get_scheme},
    protocol::Mode,
    stream::AsyncRW,
    ClientBuilder,
};

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
    run().await
}

async fn run() -> Result<(), ()> {
    tracing_subscriber::fmt::fmt()
        .with_max_level(Level::DEBUG)
        .finish()
        .try_init()
        .expect("failed to init log");
    let args = Args::parse();
    let mode = get_scheme(&args.uri).unwrap();
    let stream = async_tcp_connect(&args.uri).await.unwrap();
    let stream: Box<dyn AsyncRW> = match mode {
        Mode::WS => Box::new(stream),
        Mode::WSS => {
            let stream = async_wrap_rustls(stream, get_host(&args.uri).unwrap(), vec![])
                .await
                .unwrap();
            Box::new(stream)
        }
    };
    let builder = ClientBuilder::new();
    let mut client = builder
        .async_with_stream(
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
