use clap::Parser;
use tokio::io::BufWriter;
use tracing_subscriber::util::SubscriberInitExt;
use ws_tool::{
    codec::{default_handshake_handler, AsyncBytesCodec, FrameConfig},
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

    /// tokio runtime worker, if not not use current thread runtime, else
    /// use multi thread runtime
    jobs: Option<usize>,
}

fn main() {
    let args = Args::parse();
    tracing_subscriber::fmt::fmt()
        .with_max_level(args.level)
        .finish()
        .try_init()
        .expect("failed to init log");
    tracing::info!("binding on {}:{}", args.host, args.port);
    let rt = match args.jobs {
        Some(jobs) => tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .worker_threads(jobs)
            .build()
            .unwrap(),
        None => tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap(),
    };
    rt.block_on(run(args))
}

async fn run(args: Args) {
    let listener = tokio::net::TcpListener::bind(format!("{}:{}", args.host, args.port))
        .await
        .unwrap();
    loop {
        let (stream, addr) = listener.accept().await.unwrap();
        tokio::spawn(async move {
            tracing::info!("got connect from {:?}", addr);
            match args.buffer {
                Some(buf) => {
                    let mut server = ServerBuilder::async_accept(
                        stream,
                        default_handshake_handler,
                        |_req, stream| {
                            let stream = BufWriter::with_capacity(buf, stream);
                            let config = FrameConfig {
                                mask_send_frame: false,
                                resize_size: buf,
                                resize_thresh: buf / 3,
                                ..Default::default()
                            };
                            Ok(AsyncBytesCodec::new_with(stream, config))
                        },
                    )
                    .await
                    .unwrap();
                    loop {
                        let msg = server.receive().await.unwrap();
                        if msg.code.is_close() {
                            break;
                        }

                        server.send(&msg.data[..]).await.unwrap();
                    }
                    server.flush().await.unwrap();
                }
                None => {
                    let mut server = ServerBuilder::async_accept(
                        stream,
                        default_handshake_handler,
                        AsyncBytesCodec::factory,
                    )
                    .await
                    .unwrap();
                    loop {
                        let msg = server.receive().await.unwrap();
                        if msg.code.is_close() {
                            break;
                        }
                        server.send(&msg.data[..]).await.unwrap();
                    }
                }
            }
            tracing::info!("one conn down");
        });
    }
}
