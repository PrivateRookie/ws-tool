use std::{process::exit, sync::Arc, thread::JoinHandle};

use clap::Parser;
use dashmap::DashMap;
use tracing::Level;
use tracing_subscriber::util::SubscriberInitExt;
use ws_tool::{codec::WsBytesCodec, stream::BufStream, ClientBuilder};

/// websocket client demo with raw frame
#[derive(Parser)]
struct Args {
    uri: String,

    // client size
    #[arg(short, long, default_value = "1")]
    conn: usize,

    /// payload size (unit: bytes)
    #[arg(short, long, default_value = "1")]
    payload: usize,

    /// sample rate (unit: second)
    #[arg(short, long, default_value = "10")]
    interval: u64,

    /// total sample times
    #[arg(short, long, default_value = "6")]
    total: usize,

    /// buffer size of stream
    #[arg(long)]
    buffer: Option<usize>,
}

fn main() -> Result<(), ()> {
    tracing_subscriber::fmt::fmt()
        .with_max_level(Level::INFO)
        .finish()
        .try_init()
        .expect("failed to init log");
    let args = Args::parse();

    let size = args.payload;
    let counter: Arc<DashMap<usize, (usize, usize)>> = Default::default();
    let interval = args.interval;
    let counter_c = counter.clone();
    std::thread::spawn(move || {
        for idx in 0..args.total {
            std::thread::sleep(std::time::Duration::from_secs(interval));
            let mut stat: Vec<Statistic> = counter_c
                .iter()
                .map(|item| {
                    let conn_idx = *item.key();
                    let (send, recv) = *item.value();
                    let send = send as f64 / (interval as f64 * (idx + 1) as f64);
                    let recv = recv as f64 / (interval as f64 * (idx + 1) as f64);
                    Statistic {
                        sample_idx: idx,
                        conn_idx,
                        send,
                        recv,
                    }
                })
                .collect();
            // {
            //     counter_c.clear();
            // }
            stat.sort_by(|a, b| a.conn_idx.cmp(&b.conn_idx));
            for item in stat {
                println!("{}", serde_json::to_string(&item).unwrap());
            }
        }
        exit(0);
    });
    let handlers: Vec<JoinHandle<()>> = (0..args.conn)
        .map(|idx| {
            let counter = counter.clone();
            let uri = args.uri.clone();
            std::thread::spawn(move || {
                let builder = ClientBuilder::new(uri);
                let mut client = builder
                    .connect(|key, resp, stream| {
                        let stream = if let Some(buffer) = args.buffer {
                            BufStream::with_capacity(buffer, buffer, stream)
                        } else {
                            BufStream::new(stream)
                        };
                        WsBytesCodec::check_fn(key, resp, stream)
                    })
                    .unwrap();
                client
                    .stream_mut()
                    .get_mut()
                    .stream_mut()
                    .set_nodelay(true)
                    .unwrap();
                let (mut r, mut w) = client.split();

                let counter_c = counter.clone();
                let w = std::thread::spawn(move || {
                    let mut payload = vec![0].repeat(size);
                    loop {
                        w.send(&mut payload[..]).unwrap();
                        let mut ent = counter_c.entry(idx).or_default();
                        ent.0 += 1;
                    }
                });
                let r = std::thread::spawn(move || loop {
                    r.receive().unwrap();
                    let mut ent = counter.entry(idx).or_default();
                    ent.1 += 1;
                });
                r.join().and_then(|_| w.join()).ok();
            })
        })
        .collect();
    for h in handlers {
        h.join().ok();
    }

    Ok(())
}

#[derive(Debug, Clone, serde::Serialize)]
struct Statistic {
    sample_idx: usize,
    conn_idx: usize,
    send: f64,
    recv: f64,
}
