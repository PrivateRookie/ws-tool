use clap::Parser;
use tokio::io::BufStream;
use tracing_subscriber::util::SubscriberInitExt;
use ws_tool::{
    codec::{default_handshake_handler, AsyncWsBytesCodec},
    frame::OpCode,
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

#[tokio::main]
async fn main() -> Result<(), ()> {
    let args = Args::parse();
    tracing_subscriber::fmt::fmt()
        .with_max_level(args.level)
        .finish()
        .try_init()
        .expect("failed to init log");
    tracing::info!("binding on {}:{}", args.host, args.port);
    let listener = tokio::net::TcpListener::bind(format!("{}:{}", args.host, args.port))
        .await
        .unwrap();
    loop {
        let (stream, addr) = listener.accept().await.unwrap();
        stream.set_nodelay(true).unwrap();
        tokio::spawn(async move {
            tracing::info!("got connect from {:?}", addr);
            let mut server = ServerBuilder::async_accept(
                stream,
                default_handshake_handler,
                |req, remain, stream| {
                    let stream = if let Some(buf) = args.buffer {
                        BufStream::with_capacity(buf, buf, stream)
                    } else {
                        BufStream::new(stream)
                    };
                    AsyncWsBytesCodec::factory(req, remain, stream)
                },
            )
            .await
            .unwrap();
            loop {
                let mut msg = server.receive().await.unwrap();
                if msg.code == OpCode::Close {
                    break;
                }
                server.send(&mut msg.data[..]).await.unwrap();
            }
            tracing::info!("one conn down");
        });
    }
}
