use structopt::StructOpt;
use ws_tool::{frame::Frame, ConnBuilder, Server};

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
    pretty_env_logger::init();
    let args = Args::from_args();
    log::info!("binding on {}:{}", args.host, args.port);
    let listener = tokio::net::TcpListener::bind(format!("{}:{}", args.host, args.port))
        .await
        .unwrap();
    for (stream, addr) in listener.accept().await {
        log::info!("got connect from {:?}", addr);
        let mut server = Server::from_stream(stream);
        server.handle_handshake().await.unwrap();
        while let Some(x) = server.read().await {
            let frame = x.unwrap();
            let payload = frame.payload_data_unmask();
            let frame = Frame::new_with_payload(
                ws_tool::frame::OpCode::Text,
                format!("hello {}", String::from_utf8(payload.to_vec()).unwrap()).as_bytes(),
            );
            server.write(frame).await.unwrap();
        }
    }
    Ok(())
}
