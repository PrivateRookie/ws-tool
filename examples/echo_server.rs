use clap::Parser;
use tracing_subscriber::util::SubscriberInitExt;
use ws_tool::codec::WsStringCodec;
use ws_tool::{codec::default_handshake_handler, ServerBuilder};

/// websocket client connect to binance futures websocket
#[derive(Parser)]
struct Args {
    /// server host
    #[clap(long, default_value = "127.0.0.1")]
    host: String,
    /// server port
    #[clap(short, long, default_value = "9000")]
    port: u16,

    /// level
    #[clap(short, long, default_value = "info")]
    level: tracing::Level,
}

#[tokio::main]
async fn main() -> Result<(), ()> {
    let args = Args::parse();
    tracing_subscriber::fmt::fmt()
        .with_max_level(args.level)
        .with_file(true)
        .with_line_number(true)
        .finish()
        .try_init()
        .expect("failed to init log");
    tracing::info!("binding on {}:{}", args.host, args.port);
    let listener = std::net::TcpListener::bind(format!("{}:{}", args.host, args.port)).unwrap();
    // loop {
    let (stream, addr) = listener.accept().unwrap();

    tracing::info!("got connect from {:?}", addr);
    let mut server = ServerBuilder::accept(
        stream,
        default_handshake_handler,
        // AsyncWsStringCodec::factory,
        WsStringCodec::factory,
    )
    .unwrap();

    loop {
        match server.receive() {
            Ok(msg) => server.send((msg.code, msg.data)).unwrap(),
            Err(e) => {
                dbg!(e);
                break;
            }
        }
    }

    Ok(())
}
