use std::net::TcpListener;

use clap::Parser;
use tracing_subscriber::util::SubscriberInitExt;
use ws_tool::{
    codec::{default_handshake_handler, WsDeflateCodec},
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
    #[arg(short, long, default_value = "info")]
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
    for conn in listener.incoming() {
        if let Ok(stream) = conn {
            let mut server =
                ServerBuilder::accept(stream, default_handshake_handler, WsDeflateCodec::factory)
                    .unwrap();
            loop {
                match server.receive() {
                    Ok(msg) => server.send_read_frame(msg).unwrap(),
                    Err(e) => {
                        dbg!(e);
                        break;
                    }
                }
            }
        }
    }
}
