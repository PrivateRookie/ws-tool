use clap::Parser;
use tracing_subscriber::util::SubscriberInitExt;
use ws_tool::{
    codec::{default_handshake_handler, BytesCodec, FrameConfig},
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
                    let mut server =
                        ServerBuilder::accept(stream, default_handshake_handler, |_req, stream| {
                            let stream = BufStream::with_capacity(buf, buf, stream);
                            let config = FrameConfig {
                                mask_send_frame: false,
                                resize_size: buf,
                                ..Default::default()
                            };
                            Ok(BytesCodec::new_with(stream, config))
                        })
                        .unwrap();
                    loop {
                        let msg = server.receive().unwrap();
                        if msg.code.is_close() {
                            break;
                        }

                        server.send(&msg.data[..]).unwrap();
                    }
                }
                None => {
                    let mut server = ServerBuilder::accept(
                        stream,
                        default_handshake_handler,
                        BytesCodec::factory,
                    )
                    .unwrap();
                    loop {
                        let msg = server.receive().unwrap();
                        if msg.code.is_close() {
                            break;
                        }

                        server.send(&msg.data[..]).unwrap();
                    }
                }
            }
            tracing::info!("{:?} conn down", addr);
        });
    }
}
