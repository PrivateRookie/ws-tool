use std::{io::Write, net::TcpStream};

use clap::Parser;
use http::Uri;
use tracing::Level;
use tracing_subscriber::util::SubscriberInitExt;
use ws_tool::{
    codec::{PMDConfig, WsDeflateCodec},
    frame::{OpCode, ReadFrame},
    ClientBuilder,
};

#[derive(Parser)]
struct Args {
    uri: String,
}

fn main() {
    tracing_subscriber::fmt::fmt()
        .with_max_level(Level::DEBUG)
        .finish()
        .try_init()
        .expect("failed to init log");
    let arg = Args::parse();
    let uri: Uri = arg.uri.parse().unwrap();
    let stream =
        TcpStream::connect(format!("{}:{}", uri.host().unwrap(), uri.port().unwrap())).unwrap();
    let config = PMDConfig::default();
    let mut client = ClientBuilder::new()
        .extension(config.ext_string())
        .connect(uri, stream, WsDeflateCodec::check_fn)
        .unwrap();
    let mut input = String::new();
    loop {
        print!("[SEND] > ");
        std::io::stdout().flush().unwrap();
        std::io::stdin().read_line(&mut input).unwrap();
        if &input == "quit\n" {
            break;
        }
        let mut data = input.as_bytes().to_vec();
        client
            .send_read_frame(ReadFrame::new(
                true,
                false,
                false,
                false,
                OpCode::Text,
                false,
                &mut data[..],
            ))
            .unwrap();
        match client.receive() {
            Ok(item) => {
                if item.header().opcode().is_data() {
                    let payload = item.payload().to_vec();
                    let msg = String::from_utf8(payload).unwrap();
                    println!("[RECV] > {}", msg);
                    if msg == "quit" {
                        break;
                    }
                } else {
                    println!("[RECV] controm frame");
                }
                input.clear()
            }
            Err(e) => {
                dbg!(e);
                break;
            }
        }
    }
    loop {}
}
