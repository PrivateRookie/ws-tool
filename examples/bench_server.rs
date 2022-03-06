use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::json;
use structopt::StructOpt;
use tracing::Level;
use tracing_subscriber::util::SubscriberInitExt;
use ws_tool::{
    codec::{default_handshake_handler, WsFrameCodec, WsStringCodec},
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

fn main() -> Result<(), ()> {
    tracing_subscriber::fmt::fmt()
        .with_max_level(Level::WARN)
        .finish()
        .try_init()
        .expect("failed to init log");
    let args = Args::from_args();
    tracing::info!("binding on {}:{}", args.host, args.port);
    let listener = std::net::TcpListener::bind(format!("{}:{}", args.host, args.port)).unwrap();
    loop {
        let (stream, addr) = listener.accept().unwrap();
        std::thread::spawn(move || {
            tracing::info!("got connect from {:?}", addr);
            let mut server =
                ServerBuilder::accept(stream, default_handshake_handler, WsFrameCodec::factory)
                    .unwrap();

            // server.send(get_event(0)).unwrap();

            loop {
                if let Ok(msg) = server.receive() {
                    if msg.opcode() == OpCode::Close {
                        break;
                    }
                    server
                        .send(msg.opcode(), &msg.payload_data_unmask())
                        .unwrap();
                    // if msg.code == OpCode::Text {
                    //     let req: Request = serde_json::from_str(&msg.data).unwrap();
                    //     server.send(get_event(req.c)).unwrap();
                    // }
                } else {
                    break;
                }
            }
            tracing::info!("one conn down");
        });
    }
}
