use structopt::StructOpt;
use tracing::Level;
use ws_tool::{frame::OpCode, Server};

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
        .with_max_level(Level::INFO)
        .finish();
    let args = Args::from_args();
    tracing::info!("binding on {}:{}", args.host, args.port);
    let listener = tokio::net::TcpListener::bind(format!("{}:{}", args.host, args.port))
        .await
        .unwrap();
    for (stream, addr) in listener.accept().await {
        tracing::info!("got connect from {:?}", addr);
        let mut server = Server::from_stream(stream);
        server.handle_handshake().await.unwrap();
        while let Some(x) = server.read().await {
            let frame = x.unwrap();
            if frame.opcode() == OpCode::Close {
                tracing::info!("client close");
                break;
            }
            let payload = frame.payload_data_unmask();
            let mut frame = ws_tool::frame::Frame::new_with_opcode(ws_tool::frame::OpCode::Text);
            frame.set_mask(false);
            frame.set_payload(String::from_utf8(payload.to_vec()).unwrap().as_bytes());
            server.write(frame).await.unwrap();
        }
    }
    Ok(())
}
