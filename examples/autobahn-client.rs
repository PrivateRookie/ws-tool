use log::*;
use ws_client::{
    errors::WsError,
    frame::{Frame, OpCode},
    ClientBuilder,
};

const AGENT: &str = "ws-client";

async fn get_case_count() -> Result<usize, WsError> {
    let mut client = ClientBuilder::new("ws://localhost:9001/getCaseCount").build()?;
    client.connect().await?;
    let frame = client.read_frame().await?;
    let unmask_data = frame.payload_data_unmask();
    let count = String::from_utf8(unmask_data.to_vec())
        .unwrap()
        .parse::<usize>()
        .unwrap();
    client.read_frame().await?;
    client.close().await?;
    Ok(count)
}

async fn run_test(case: usize) -> Result<(), WsError> {
    info!("running test case {}", case);
    let url = format!("ws://localhost:9001/runCase?case={}&agent={}", case, AGENT);
    let mut client = ClientBuilder::new(&url).build()?;
    client.connect().await?;
    let normal_close = loop {
        let frame = client.read_frame().await?;
        match frame.opcode() {
            OpCode::Text => {
                let payload = frame.payload_data_unmask();
                match String::from_utf8(payload.to_vec()) {
                    Ok(_) => {
                        let echo = Frame::new_with_payload(frame.opcode(), &payload);
                        client.write_frame(echo).await?;
                    }
                    Err(_) => break false,
                }
            }
            OpCode::Binary => {
                let mut echo = Frame::new_with_opcode(frame.opcode());
                echo.set_payload(&frame.payload_data_unmask());
                client.write_frame(echo).await?;
            }
            OpCode::Ping => {
                if frame.payload_data().len() > 125 {
                    break false;
                } else {
                    let mut echo = Frame::new_with_opcode(OpCode::Pong);
                    echo.set_payload(&frame.payload_data_unmask());
                    client.write_frame(echo).await?;
                }
            }
            OpCode::Pong => {}
            OpCode::Close => break true,
            OpCode::ReservedControl | OpCode::ReservedNonControl => {
                break false;
            }
            _ => {
                error!("unexpected frame {:?}", frame);
            }
        }
    };
    if normal_close {
        client.close().await
    } else {
        client.close_with_reason(1002, "").await
    }
}

async fn update_report() -> Result<(), WsError> {
    let mut client = ClientBuilder::new(&format!(
        "ws://localhost:9001/updateReports?agent={}",
        AGENT
    ))
    .build()?;
    client.connect().await?;
    client.close().await
}

#[tokio::main]
async fn main() -> Result<(), ()> {
    pretty_env_logger::init();
    let count = get_case_count().await.unwrap();
    info!("total case {}", count);
    for case in 1..=count {
        if let Err(e) = run_test(case).await {
            error!("case {} {}", case, e);
        }
    }
    update_report().await.unwrap();
    Ok(())
}
