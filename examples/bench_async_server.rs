use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::json;
use structopt::StructOpt;
use tracing::Level;
use tracing_subscriber::util::SubscriberInitExt;
use ws_tool::{
    codec::{default_handshake_handler, AsyncWsStringCodec, AsyncWsBytesCodec},
    frame::OpCode,
    ServerBuilder,
};

#[derive(Serialize, Deserialize, Debug)]
struct Request {
    c: i32,
}

fn get_timestamp() -> i64 {
    //Gets the current unix timestamp of the server
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(n) => return n.as_secs() as i64,
        Err(_e) => return 0,
    }
}

fn get_event(c: i32) -> String {
    let event = json!({
        "c": c,
        "ts": get_timestamp()
    });
    event.to_string()
}

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
        .finish()
        .try_init()
        .expect("failed to init log");
    let args = Args::from_args();
    tracing::info!("binding on {}:{}", args.host, args.port);
    let listener = tokio::net::TcpListener::bind(format!("{}:{}", args.host, args.port))
        .await
        .unwrap();
    loop {
        let (stream, addr) = listener.accept().await.unwrap();
        tokio::spawn(async move {
            tracing::info!("got connect from {:?}", addr);
            let mut server = ServerBuilder::async_accept(
                stream,
                default_handshake_handler,
                AsyncWsStringCodec::factory,
            )
            .await
            .unwrap();

            server.send(get_event(0)).await.unwrap();

            loop {
                if let Ok(msg) = server.receive().await {
                    if msg.code == OpCode::Close {
                        break;
                    }
                    if msg.code == OpCode::Text {
                        let req: Request = serde_json::from_str(&msg.data).unwrap();
                        server.send(get_event(req.c)).await.unwrap();
                    }
                } else {
                    break;
                }
            }
            tracing::info!("one conn down");
        });
    }
}
