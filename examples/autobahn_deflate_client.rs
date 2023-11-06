use rand::random;
use tracing::*;
use tracing_subscriber::util::SubscriberInitExt;
use ws_tool::{
    codec::{StringCodec, WindowBit},
    errors::WsError,
    frame::{OpCode, OwnedFrame},
    ClientConfig,
};

const AGENT: &str = "deflate-client";

fn get_case_count() -> Result<usize, WsError> {
    let uri = "ws://localhost:9002/getCaseCount";
    let mut client = ClientConfig::default()
        .connect_with(uri, StringCodec::check_fn)
        .unwrap();
    let msg = client.receive().unwrap().data.parse().unwrap();
    Ok(msg)
}

fn mask_key() -> [u8; 4] {
    random()
}

fn run_test(case: usize) -> Result<(), WsError> {
    info!("running test case {}", case);
    let url = format!("ws://localhost:9002/runCase?case={}&agent={}", case, AGENT);
    let (mut read, mut write) = ClientConfig {
        window: Some(WindowBit::Nine),
        ..Default::default()
    }
    .connect(url)
    .unwrap()
    .split();
    let now = std::time::Instant::now();
    loop {
        match read.receive() {
            Ok((header, data)) => {
                let code = header.code;
                match &code {
                    OpCode::Text | OpCode::Binary => write.send(code, data)?,
                    OpCode::Close => {
                        write.send(OpCode::Close, &1000u16.to_be_bytes()).unwrap();
                        tracing::info!("case {case} elapsed {:?}", now.elapsed());
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
                    if write
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
                    write
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
    let url = format!("ws://localhost:9002/updateReports?agent={}", AGENT);
    let mut client = ClientConfig::default().connect(url).unwrap();
    client.close(1000u16, &[]).map(|_| ())
}

fn main() -> Result<(), ()> {
    tracing_subscriber::fmt::fmt()
        .with_max_level(Level::INFO)
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
