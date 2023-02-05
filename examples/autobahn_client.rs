use bytes::BytesMut;
use std::net::TcpStream;
use tracing::*;
use tracing_subscriber::util::SubscriberInitExt;
use ws_tool::{
    codec::{WsFrameCodec, WsStringCodec},
    errors::WsError,
    frame::OpCode,
    ClientBuilder,
};

const AGENT: &str = "ws-tool-client";

fn get_case_count() -> Result<usize, WsError> {
    let stream = TcpStream::connect("localhost:9002").unwrap();
    let mut client = ClientBuilder::new()
        .connect(
            "ws://localhost:9002/getCaseCount".parse().unwrap(),
            stream,
            WsStringCodec::check_fn,
        )
        .unwrap();
    let msg = client.receive().unwrap();
    client.receive().unwrap();
    // send_close(&mut client, 1001, "".to_string()).unwrap();
    Ok(msg.data.parse().unwrap())
}

fn run_test(case: usize) -> Result<(), WsError> {
    info!("running test case {}", case);
    let url: http::Uri = format!("ws://localhost:9002/runCase?case={}&agent={}", case, AGENT)
        .parse()
        .unwrap();
    let stream = TcpStream::connect("localhost:9002").unwrap();
    let mut client = ClientBuilder::new()
        .connect(url, stream, WsFrameCodec::check_fn)
        .unwrap();
    loop {
        match client.receive() {
            Ok(mut frame) => {
                let code = frame.header().opcode();
                match &code {
                    OpCode::Text | OpCode::Binary => {
                        client.send_mut(code, frame.payload_mut(), true)?;
                    }
                    OpCode::Close => {
                        let mut data = BytesMut::new();
                        data.extend_from_slice(&1000u16.to_be_bytes());
                        client.send_mut(OpCode::Close, &mut data, true).unwrap();
                        break;
                    }
                    OpCode::Ping => {
                        client.send_mut(OpCode::Pong, frame.payload_mut(), true)?;
                    }
                    OpCode::Pong => {}
                    OpCode::Continue | OpCode::ReservedNonControl | OpCode::ReservedControl => {
                        unreachable!()
                    }
                }
            }
            Err(e) => match e {
                WsError::ProtocolError { close_code, error } => {
                    let mut data = BytesMut::new();
                    data.extend_from_slice(&close_code.to_be_bytes());
                    data.extend_from_slice(error.to_string().as_bytes());
                    if client.send_mut(OpCode::Close, &mut data, true).is_err() {
                        break;
                    }
                }
                _ => {
                    let mut data = BytesMut::new();
                    data.extend_from_slice(&1000u16.to_be_bytes());
                    if client.send(OpCode::Close, &data).is_err() {
                        break;
                    }
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
        .connect(url, stream, WsStringCodec::check_fn)
        .unwrap();
    client.send((1000u16, String::new())).map(|_| ())
}

fn main() -> Result<(), ()> {
    tracing_subscriber::fmt::fmt()
        .with_max_level(Level::INFO)
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
