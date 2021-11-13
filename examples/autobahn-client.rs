use log::*;
use std::io::Result as IOResult;
use ws_tool::{
    frame::{Frame, OpCode},
    ConnBuilder,
};

const AGENT: &str = "ws-tool-client";

async fn get_case_count() -> IOResult<usize> {
    let mut client = ConnBuilder::new("ws://localhost:9001/getCaseCount")
        .build()
        .await
        .unwrap();
    client.handshake().await.unwrap();
    let frame = client.read().await.unwrap().unwrap();
    let unmask_data = frame.payload_data_unmask();
    let count = String::from_utf8(unmask_data.to_vec())
        .unwrap()
        .parse::<usize>()
        .unwrap();
    client.read().await.unwrap().unwrap();
    client.close(1001, "".to_string()).await.unwrap();
    Ok(count)
}

async fn run_test(case: usize) -> IOResult<()> {
    info!("running test case {}", case);
    let url = format!("ws://localhost:9001/runCase?case={}&agent={}", case, AGENT);
    let mut client = ConnBuilder::new(&url).build().await.unwrap();
    client.handshake().await.unwrap();
    while let Some(maybe_frame) = client.read().await {
        if maybe_frame.is_err() {
            dbg!(&maybe_frame);
        }
        let frame = maybe_frame?;
        match frame.opcode() {
            OpCode::Text => {
                let payload = frame.payload_data_unmask();
                let echo = Frame::new_with_payload(OpCode::Text, &payload);
                log::debug!("wrote text resp");
                client.write(echo).await?
            }
            OpCode::Binary => {
                let mut echo = Frame::new_with_opcode(frame.opcode());
                echo.set_payload(&frame.payload_data_unmask());
                log::debug!("wrote bin resp");
                client.write(echo).await?;
            }
            OpCode::Close => {
                break;
            }
            OpCode::Ping => {
                let mut echo = Frame::new_with_opcode(OpCode::Pong);
                echo.set_payload(&frame.payload_data_unmask());
                log::debug!("wrote ping resp");
                client.write(echo).await?;
            }
            OpCode::Pong => {}
            OpCode::Continue | OpCode::ReservedNonControl | OpCode::ReservedControl => {
                unreachable!()
            }
        }
    }
    client.close(1000, "".to_string()).await?;
    info!("run test case done");
    Ok(())
}

async fn update_report() -> IOResult<()> {
    let mut client = ConnBuilder::new(&format!(
        "ws://localhost:9001/updateReports?agent={}",
        AGENT
    ))
    .build()
    .await
    .unwrap();
    client.handshake().await.unwrap();
    client.close(1000, "".to_string()).await
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
