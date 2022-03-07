use structopt::StructOpt;
use tracing::Level;
use tracing_subscriber::util::SubscriberInitExt;
use ws_tool::{
    codec::{default_handshake_handler, AsyncWsFrameCodec},
    ServerBuilder,
};

/// websocket client connect to binance futures websocket
#[derive(StructOpt)]
struct Args {
    /// server host
    #[structopt(long, default_value = "127.0.0.1")]
    host: String,
    /// server port
    #[structopt(short, long, default_value = "9000")]
    port: u16,
}

#[tokio::main]
async fn main() -> Result<(), ()> {
    tracing_subscriber::fmt::fmt()
        .with_max_level(Level::ERROR)
        .finish()
        .try_init()
        .expect("failed to init log");
    let args = Args::from_args();
    tracing::info!("binding on {}:{}", args.host, args.port);
    let listener = tokio::net::TcpListener::bind(format!("{}:{}", args.host, args.port))
        .await
        .unwrap();
    // loop {
    let (stream, addr) = listener.accept().await.unwrap();

    tracing::info!("got connect from {:?}", addr);
    let mut server = ServerBuilder::async_accept(
        stream,
        default_handshake_handler,
        // AsyncWsStringCodec::factory,
        AsyncWsFrameCodec::factory,
    )
    .await
    .unwrap();

    loop {
        if let Ok(msg) = server.receive().await {
            // if msg.code == OpCode::Close {
            //     break;
            // }
            // server.send(msg).await.unwrap();
            server
                .send(msg.opcode(), &msg.payload_data_unmask())
                .await
                .unwrap();
        } else {
            break;
        }
    }
    // }
    Ok(())
}
