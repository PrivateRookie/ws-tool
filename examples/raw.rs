use std::{io::Write, path::PathBuf};


use bytes::BytesMut;
use structopt::StructOpt;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use ws_client::{
    connect,
    frame::{Frame, OpCode},
};

/// websocket client demo with raw frame
#[derive(StructOpt)]
struct Args {
    uri: String,
    /// cert file path
    #[structopt(short, long)]
    cert: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> Result<(), ()> {
    let args = Args::from_args();
    let mut stream = connect(&args.uri, args.cert).await.expect("invalid url");
    let mut resp = [0; 4096];
    let mut resp_bytes = BytesMut::new();
    let mut input = String::new();
    loop {
        print!("[SEND] > ");
        std::io::stdout().flush().unwrap();
        std::io::stdin().read_line(&mut input).unwrap();
        println!("{}", input == "quit");
        if input == "quit" {
            let close = Frame::new_with_opcode(OpCode::Close);
            stream.write_all(close.as_bytes()).await.unwrap();
            break;
        }
        let mut frame = Frame::new();
        frame.set_payload(input.as_bytes());
        stream.write_all(frame.as_bytes()).await.unwrap();
        resp_bytes.clear();
        loop {
            let num = stream.read(&mut resp).await.unwrap();
            resp_bytes.extend_from_slice(&resp[0..num]);
            if num <= 4096 {
                break;
            }
        }
        let frame = Frame::from_bytes_uncheck(&resp_bytes);
        if frame.opcode() == OpCode::Close {
            println!("remote close ...");
            break;
        }
        let msg = String::from_utf8(frame.payload_data_unmask().to_vec()).unwrap();
        println!("[RECV] > {}", msg.trim());
        if msg == "quit" {
            break;
        }
    }
    return Ok(());
}
