use std::io::{Read, Write};

use rcgen::generate_simple_self_signed;

use bytes::BytesMut;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use ws_client::{
    connect,
    frame::{Frame, OpCode},
};

#[tokio::main]
async fn main() -> Result<(), ()> {
    // let subject_alt_names = vec!["target".to_string(), "localhost".to_string()];
    // let cert = generate_simple_self_signed(subject_alt_names).unwrap();
    // // The certificate is now valid for localhost and the domain "hello.world.example"
    // println!("{}", cert.serialize_pem().unwrap());
    // println!("{}", cert.serialize_private_key_pem());
    // let uri = "wss://echo.websocket.org";
    let uri = "ws://localhost:9001";
    let uri = "wss://target:8765";
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
        let msg = String::from_utf8(frame.payload_data_unmask().to_vec()).unwrap();
        println!("[RECV] > {}", msg.trim());
        if msg == "quit" {
            break;
        }
    }
    return Ok(());
}
