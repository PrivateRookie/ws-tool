use std::{io::Write, path::PathBuf};

use structopt::StructOpt;
use ws_client::{frame::Frame, ConnBuilder};

/// websocket client demo with raw frame
#[derive(StructOpt)]
struct Args {
    uri: String,
    /// cert file path
    #[structopt(short, long)]
    cert: Option<PathBuf>,

    /// proxy setting
    #[structopt(long)]
    proxy: Option<String>,
}

#[tokio::main]
async fn main() -> Result<(), ()> {
    pretty_env_logger::init();
    let args = Args::from_args();
    let mut builder = ConnBuilder::new(&args.uri);
    if let Some(cert) = args.cert {
        builder = builder.cert(cert);
    }
    if let Some(proxy) = args.proxy {
        builder = builder.proxy(&proxy)
    }
    let mut client = builder.build().await.unwrap();
    client.handshake().await.unwrap();

    let mut input = String::new();
    loop {
        print!("[SEND] > ");
        std::io::stdout().flush().unwrap();
        std::io::stdin().read_line(&mut input).unwrap();
        if &input == "quit\n" {
            break;
        }
        let mut frame = Frame::default();
        frame.set_payload(input.trim().as_bytes());
        client.write_frame(frame).await.unwrap();
        let resp = client.read_frame().await.unwrap();
        let msg = String::from_utf8(resp.payload_data_unmask().to_vec()).unwrap();
        println!("[RECV] > {}", msg.trim());
        if &msg == "quit" {
            break;
        }
        input.clear()
    }
    client.close(1000, "".to_string()).await.unwrap();
    return Ok(());
}
