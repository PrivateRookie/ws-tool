use std::io::{Read, Write};

use bytes::BytesMut;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use ws_client::{connect, Frame};

#[tokio::main]
async fn main() -> Result<(), ()> {
    // let uri = "wss://echo.websocket.org";
    let uri = "ws://localhost:9001";
    let uri = "wss://echo.websocket.org";
    let mut stream = connect(uri).await.unwrap();
    let mut resp = [0; 4096];
    let mut resp_bytes = BytesMut::new();
    let mut input = String::new();
    loop {
        print!("[SEND] > ");
        std::io::stdout().flush().unwrap();
        std::io::stdin().read_line(&mut input).unwrap();
        println!("{}", input == "quit");
        if input == "quit" {
            let close = Frame::new_with_opcode(ws_client::OpCode::Close);
            stream.write_all(close.get_bytes()).await.unwrap();
            break;
        }
        let mut frame = Frame::new();
        frame.set_payload(input.as_bytes());
        stream.write_all(frame.get_bytes()).await.unwrap();
        resp_bytes.clear();
        loop {
            let num = stream.read(&mut resp).await.unwrap();
            resp_bytes.extend_from_slice(&resp[0..num]);
            if num <= 4096 {
                break;
            }
        }
        let frame = Frame::from_bytes_uncheck(&resp_bytes);
        let msg = String::from_utf8(frame.payload_data_unmask().to_vec()).unwrap();
        println!("[RECV] > {}", msg.trim());
        if msg == "quit" {
            break;
        }
    }
    return Ok(());
}
