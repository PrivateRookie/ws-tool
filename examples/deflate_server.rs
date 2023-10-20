use std::net::TcpListener;

use clap::Parser;
use tracing_subscriber::util::SubscriberInitExt;
use ws_tool::{
    codec::{deflate_handshake_handler, DeflateCodec},
    ServerBuilder,
};

/// websocket client connect to binance futures websocket
#[derive(Parser)]
struct Args {
    /// server host
    #[arg(long, default_value = "127.0.0.1")]
    host: String,
    /// server port
    #[arg(short, long, default_value = "9000")]
    port: u16,

    /// level
    #[arg(short, long, default_value = "debug")]
    level: tracing::Level,
}

fn main() {
    let args = Args::parse();
    tracing_subscriber::fmt::fmt()
        .with_max_level(args.level)
        .with_file(true)
        .with_line_number(true)
        .finish()
        .try_init()
        .expect("failed to init log");
    tracing::info!("binding on {}:{}", args.host, args.port);
    let listener = TcpListener::bind(format!("{}:{}", args.host, args.port)).unwrap();
    for stream in listener.incoming().flatten() {
        let (mut read, mut write) =
            ServerBuilder::accept(stream, deflate_handshake_handler, DeflateCodec::factory)
                .unwrap()
                .split();
        loop {
            match read.receive() {
                Ok((header, data)) => write.send(header.code, data).unwrap(),
                Err(e) => {
                    dbg!(e);
                    break;
                }
            }
        }
    }
}
