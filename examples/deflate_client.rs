use std::io::Write;

use clap::Parser;
use http::Uri;
use rand::random;
use tracing::Level;
use tracing_subscriber::util::SubscriberInitExt;
use ws_tool::{
    codec::{DeflateCodec, PMDConfig, WindowBit},
    frame::OwnedFrame,
    ClientBuilder,
};

#[derive(Parser)]
struct Args {
    uri: String,
}

fn main() {
    tracing_subscriber::fmt::fmt()
        .with_max_level(Level::DEBUG)
        .with_line_number(true)
        .with_file(true)
        .finish()
        .try_init()
        .expect("failed to init log");
    let arg = Args::parse();
    let uri: Uri = arg.uri.parse().unwrap();
    let config = PMDConfig {
        server_max_window_bits: WindowBit::Nine,
        client_max_window_bits: WindowBit::Nine,
        ..PMDConfig::default()
    };
    let mut client = ClientBuilder::new()
        .extension(config.ext_string())
        .connect(uri, DeflateCodec::check_fn)
        .unwrap();
    let mut input = String::new();
    loop {
        print!("[SEND] > ");
        std::io::stdout().flush().unwrap();
        std::io::stdin().read_line(&mut input).unwrap();
        if &input == "quit\n" {
            break;
        }
        let frame = OwnedFrame::text_frame(random::<[u8; 4]>(), input.trim_end());
        client.send_owned_frame(frame).unwrap();
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
                    println!("[RECV] control frame");
                }
                input.clear()
            }
            Err(e) => {
                dbg!(e);
                break;
            }
        }
    }
}
