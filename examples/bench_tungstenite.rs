use clap::Parser;
use tracing_subscriber::util::SubscriberInitExt;

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

fn main() -> Result<(), ()> {
    let args = Args::parse();
    tracing_subscriber::fmt::fmt()
        .with_max_level(args.level)
        .finish()
        .try_init()
        .expect("failed to init log");
    tracing::info!("binding on {}:{}", args.host, args.port);
    let listener = std::net::TcpListener::bind(format!("{}:{}", args.host, args.port)).unwrap();
    // loop {
        let (stream, addr) = listener.accept().unwrap();
        std::thread::spawn(move || {
            tracing::info!("got connect from {:?}", addr);
            let mut ws = tungstenite::accept(stream).unwrap();
            loop {
                let msg = ws.read_message().unwrap();
                if msg.is_close() {
                    break;
                }
                ws.write_message(msg).unwrap();
            }
            tracing::info!("one conn down");
        }).join().unwrap();
    // }
    Ok(())
}
