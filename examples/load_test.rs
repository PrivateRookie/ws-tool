use std::{
    path::PathBuf,
    thread::JoinHandle,
    time::{self, Duration},
};

use clap::Parser;
use tracing::Level;
use tracing_subscriber::util::SubscriberInitExt;
use ws_tool::{codec::WsBytesCodec, ClientBuilder};

/// websocket client demo with raw frame
#[derive(Parser)]
struct Args {
    uri: String,

    // client size
    #[clap(long, default_value = "1")]
    conn: usize,

    /// payload size (unit: kb)
    #[clap(short, long, default_value = "1")]
    payload: usize,

    /// count
    #[clap(short, long, default_value = "5000")]
    num: usize,

    /// cert file path
    #[clap(short, long)]
    cert: Option<PathBuf>,
}

fn main() -> Result<(), ()> {
    tracing_subscriber::fmt::fmt()
        .with_max_level(Level::INFO)
        .finish()
        .try_init()
        .expect("failed to init log");
    let args = Args::parse();

    // if let Some(proxy) = args.proxy {
    //     builder = builder.proxy(&proxy)
    // }
    let total = args.num;
    let size = args.payload;
    let handlers: Vec<JoinHandle<Duration>> = (0..args.conn)
        .map(|_| {
            let uri = args.uri.clone();
            let cert = args.cert.clone();
            std::thread::spawn(move || {
                let mut builder = ClientBuilder::new(uri);
                if let Some(cert) = cert {
                    builder = builder.cert(cert);
                }
                let mut client = builder.connect(WsBytesCodec::check_fn).unwrap();
                client.stream_mut().stream_mut().set_nodelay(true).unwrap();
                let start = time::SystemTime::now();
                let mut payload = vec![0].repeat(size * 1024);
                for _ in 0..total {
                    client.send(&mut payload[..]).unwrap();
                    client.receive().unwrap();
                }
                let end = time::SystemTime::now();
                let elapse = end.duration_since(start);
                elapse.unwrap()
            })
        })
        .collect();
    let mut success = vec![];
    let mut failed = vec![];
    for (idx, h) in handlers.into_iter().enumerate() {
        match h.join() {
            Ok(elapse) => {
                success.push((idx, elapse));
            }
            Err(e) => {
                failed.push((idx, e));
            }
        }
    }
    for (idx, s) in success {
        println!("[{idx:0>3} - OK] {:.3}", total as f64 / s.as_secs_f64());
    }
    for (idx, e) in failed {
        println!("[{idx:0>3} - ERR] {e:?}")
    }

    Ok(())
}
