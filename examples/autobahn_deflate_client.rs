use rand::random;
use std::net::TcpStream;
use tracing::*;
use tracing_subscriber::util::SubscriberInitExt;
use ws_tool::{
    codec::{DeflateCodec, PMDConfig, StringCodec, WindowBit},
    errors::WsError,
    frame::{OpCode, OwnedFrame},
    ClientBuilder,
};

const AGENT: &str = "deflate-client";

fn get_case_count() -> Result<usize, WsError> {
    let stream = TcpStream::connect("localhost:9002").unwrap();
    let mut client = ClientBuilder::new()
        .connect(
            "ws://localhost:9002/getCaseCount".parse().unwrap(),
            stream,
            StringCodec::check_fn,
        )
        .unwrap();
    let msg = client.receive().unwrap();
    client.receive().unwrap();
    Ok(msg.data.parse().unwrap())
}

fn mask_key() -> [u8; 4] {
    random()
}

fn run_test(case: usize) -> Result<(), WsError> {
    info!("running test case {}", case);
    let url: http::Uri = format!("ws://localhost:9002/runCase?case={}&agent={}", case, AGENT)
        .parse()
        .unwrap();
    let stream = TcpStream::connect("localhost:9002").unwrap();
    let config = PMDConfig {
        server_max_window_bits: WindowBit::Nine,
        client_max_window_bits: WindowBit::Nine,
        ..PMDConfig::default()
    };
    let mut client = match ClientBuilder::new().extension(config.ext_string()).connect(
        url,
        stream,
        DeflateCodec::check_fn,
    ) {
        Ok(client) => client,
        Err(e) => {
            tracing::error!("{e}");
            return Ok(());
        }
    };
    let now = std::time::Instant::now();
    loop {
        match client.receive() {
            Ok(frame) => {
                let code = frame.header().opcode();
                match &code {
                    OpCode::Text | OpCode::Binary => client.send(code, frame.payload())?,
                    OpCode::Close => {
                        client.send_owned_frame(OwnedFrame::close_frame(mask_key(), 1000, &[]))?;
                        tracing::info!("case {case} elapsed {:?}", now.elapsed());
                        break;
                    }
                    OpCode::Ping => {
                        client.send(OpCode::Pong, frame.payload())?;
                    }
                    OpCode::Pong => {}
                    OpCode::Continue | OpCode::ReservedNonControl | OpCode::ReservedControl => {
                        unreachable!()
                    }
                }
            }
            Err(e) => match e {
                WsError::ProtocolError { close_code, error } => {
                    if client
                        .send_owned_frame(OwnedFrame::close_frame(
                            mask_key(),
                            close_code,
                            error.to_string().as_bytes(),
                        ))
                        .is_err()
                    {
                        break;
                    }
                }
                e => {
                    tracing::warn!("{e}");
                    client
                        .send_owned_frame(OwnedFrame::close_frame(mask_key(), 1000, &[]))
                        .ok();
                    break;
                }
            },
        }
    }

    Ok(())
}

fn update_report() -> Result<(), WsError> {
    let url: http::Uri = format!("ws://localhost:9002/updateReports?agent={}", AGENT)
        .parse()
        .unwrap();
    let stream = TcpStream::connect("localhost:9002").unwrap();
    let mut client = ClientBuilder::new()
        .connect(url, stream, StringCodec::check_fn)
        .unwrap();
    client.send((1000u16, String::new())).map(|_| ())
}

fn main() -> Result<(), ()> {
    tracing_subscriber::fmt::fmt()
        .with_max_level(Level::DEBUG)
        .with_file(true)
        .with_line_number(true)
        .finish()
        .try_init()
        .expect("failed to init log");
    let count = get_case_count().unwrap();
    info!("total case {}", count);
    for case in 1..=count {
        if let Err(e) = run_test(case) {
            error!("case {} {}", case, e);
        }
    }
    update_report().unwrap();
    Ok(())
}
