use clap::Parser;
use tracing_subscriber::util::SubscriberInitExt;
use ws_tool::{
    codec::{deflate_handshake_handler, DeflateCodec},
    stream::BufStream,
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

    /// buffer size
    #[arg(short, long)]
    buffer: Option<usize>,
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
    loop {
        let (stream, addr) = listener.accept().unwrap();
        std::thread::spawn(move || {
            tracing::info!("got connect from {:?}", addr);
            match args.buffer {
                Some(buf) => {
                    let (mut read, mut write) =
                        ServerBuilder::accept(stream, deflate_handshake_handler, |req, stream| {
                            let stream = BufStream::with_capacity(buf, buf, stream);
                            DeflateCodec::factory(req, stream)
                        })
                        .unwrap()
                        .split();
                    loop {
                        let (header, data) = read.receive().unwrap();
                        if header.code.is_close() {
                            break;
                        }
                        write.send(header.code, data).unwrap();
                    }
                }
                None => {
                    let (mut read, mut write) = ServerBuilder::accept(
                        stream,
                        deflate_handshake_handler,
                        DeflateCodec::factory,
                    )
                    .unwrap()
                    .split();
                    loop {
                        let (header, data) = read.receive().unwrap();
                        if header.code.is_close() {
                            break;
                        }
                        write.send(header.code, data).unwrap();
                    }
                }
            }

            tracing::info!("one conn down");
        });
    }
}
