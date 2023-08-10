use std::{
    collections::HashMap,
    net::TcpStream,
    num::ParseIntError,
    time::{Duration, Instant},
};

use clap::Parser;
use tracing::Level;
use tracing_subscriber::util::SubscriberInitExt;
use ws_tool::{
    codec::{DeflateCodec, PMDConfig, WindowBit},
    frame::OpCode::{Binary, Close},
    stream::BufStream,
    ClientBuilder,
};

/// websocket client demo with raw frame
#[derive(Parser)]
struct Args {
    uri: http::Uri,

    // client size
    #[arg(long, default_value = "1")]
    conn: usize,

    /// payload size (unit: bytes)
    #[arg(short, long, default_value = "1")]
    payload: usize,

    /// message count, unit (1k)
    #[arg(long, default_value = "10")]
    count: u64,

    /// total sample times
    #[arg(short, long, default_value = "6")]
    total: usize,

    #[arg(short, long, value_parser=parse_window, default_value="15")]
    window: WindowBit,

    /// buffer size of stream
    #[arg(short, long)]
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
    let count = args.count * 1000;
    let pmd_conf = PMDConfig {
        server_max_window_bits: args.window,
        client_max_window_bits: args.window,
        ..Default::default()
    };
    let handlers = (0..args.conn).map(|conn_idx| {
        let uri = args.uri.clone();
        let pmd_conf = pmd_conf.clone();
        std::thread::spawn(move || {
            let builder = ClientBuilder::new().extension(pmd_conf.ext_string());
            let stat: HashMap<usize, Duration> = (0..args.total)
                .map(|idx| {
                    // println!("CONN: {conn_idx:>03} ITER: {idx:>03} ...");
                    let now = Instant::now();
                    let uri = uri.clone();
                    let stream =
                        TcpStream::connect((uri.host().unwrap(), uri.port_u16().unwrap())).unwrap();
                    let client = builder
                        .with_stream(uri, stream, |key, resp, stream| {
                            let stream = if let Some(buffer) = args.buffer {
                                BufStream::with_capacity(buffer, buffer, stream)
                            } else {
                                BufStream::new(stream)
                            };
                            DeflateCodec::check_fn(key, resp, stream)
                        })
                        .unwrap();
                    let (mut r, mut w) = client.split();

                    let w = std::thread::spawn(move || {
                        let payload = vec![0].repeat(size);
                        for _ in 0..count {
                            w.send(Binary, &payload[..]).unwrap();
                        }
                        w.flush().unwrap();
                        w.send(Close, 1000u16.to_be_bytes().as_slice()).unwrap();
                    });
                    let r = std::thread::spawn(move || {
                        for _ in 0..count {
                            r.receive().unwrap();
                        }
                    });
                    r.join().and_then(|_| w.join()).unwrap();
                    let elapse = now.elapsed();
                    (idx, elapse)
                })
                .collect();
            (conn_idx, stat)
        })
    });
    let mut stat: Vec<Record> = vec![];
    for (connection, data) in handlers.into_iter().filter_map(|h| h.join().ok()) {
        for (iteration, v) in data {
            let duration = v.as_millis();
            let qps = count as f64 / duration as f64 * 1000.0;
            let record = Record {
                connection,
                iteration,
                count,
                duration,
                qps,
            };
            stat.push(record);
        }
    }
    stat.sort_by(|a, b| match a.connection.cmp(&b.connection) {
        std::cmp::Ordering::Equal => match a.iteration.cmp(&b.iteration) {
            std::cmp::Ordering::Equal => a.duration.cmp(&b.duration),
            ord => ord,
        },
        ord => ord,
    });

    let table = Table::new(stat).with(Style::markdown()).to_string();
    println!("\n{}", table);

    Ok(())
}

fn parse_window(s: &str) -> Result<WindowBit, String> {
    let v: u8 = s.parse().map_err(|e: ParseIntError| e.to_string())?;
    WindowBit::try_from(v).map_err(|e| e.to_string())
}

use tabled::{Table, Tabled, settings::Style};

#[derive(Tabled, PartialEq, PartialOrd)]
struct Record {
    connection: usize,
    iteration: usize,
    count: u64,
    #[tabled(rename = "Duration(ms)")]
    duration: u128,
    #[tabled(rename = "Message/sec", display_with = "fmt_qps")]
    qps: f64,
}

fn fmt_qps(qps: &f64) -> String {
    format!("{qps:.2}")
}
