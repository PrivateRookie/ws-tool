use bytes::BytesMut;
use tracing::*;
use tracing_subscriber::util::SubscriberInitExt;
use ws_tool::{
    codec::{FrameCodec, StringCodec},
    errors::WsError,
    frame::OpCode,
    ClientConfig,
};

const AGENT: &str = "client";

fn get_case_count() -> Result<usize, WsError> {
    let uri = "ws://localhost:9002/getCaseCount";
    let mut client = ClientConfig::default()
        .connect_with(uri, StringCodec::check_fn)
        .unwrap();
    let msg = client.receive().unwrap().data.parse().unwrap();
    Ok(msg)
}

fn run_test(case: usize) -> Result<(), WsError> {
    info!("running test case {}", case);
    let url = format!("ws://localhost:9002/runCase?case={}&agent={}", case, AGENT);
    let (mut read, mut write) = ClientConfig::default()
        .connect_with(url, FrameCodec::check_fn)
        .unwrap()
        .split();
    loop {
        match read.receive() {
            Ok((header, data)) => {
                let code = header.code;
                match &code {
                    OpCode::Text | OpCode::Binary => {
                        write.send(code, data)?;
                    }
                    OpCode::Close => {
                        let mut data = BytesMut::new();
                        data.extend_from_slice(&1000u16.to_be_bytes());
                        write.send(OpCode::Close, &data).unwrap();
                        break;
                    }
                    OpCode::Ping => {
                        write.send(OpCode::Pong, data)?;
                    }
                    OpCode::Pong => {}
                    _ => {
                        unreachable!()
                    }
                }
            }
            Err(e) => match e {
                WsError::ProtocolError { close_code, error } => {
                    let mut data = BytesMut::new();
                    data.extend_from_slice(&close_code.to_be_bytes());
                    data.extend_from_slice(error.to_string().as_bytes());
                    if write.send(OpCode::Close, &data).is_err() {
                        break;
                    }
                }
                e => {
                    tracing::warn!("{e}");
                    let mut data = BytesMut::new();
                    data.extend_from_slice(&1000u16.to_be_bytes());
                    write.send(OpCode::Close, &data).ok();
                    break;
                }
            },
        }
    }

    Ok(())
}

fn update_report() -> Result<(), WsError> {
    let url = format!("ws://localhost:9002/updateReports?agent={}", AGENT);
    let mut client = ClientConfig::default().connect(url).unwrap();
    client.close(1000u16, &[]).map(|_| ())
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
