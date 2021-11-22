use tracing::*;
use std::io::Result as IOResult;
use ws_tool::{
    frame::{Frame, OpCode},
    ConnBuilder,
};

const AGENT: &str = "ws-tool-client";

async fn get_case_count() -> IOResult<usize> {
    let mut client = ConnBuilder::new("ws://localhost:9002/getCaseCount")
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
    let url = format!("ws://localhost:9002/runCase?case={}&agent={}", case, AGENT);
    let mut client = ConnBuilder::new(&url).build().await.unwrap();
    client.handshake().await.unwrap();
    loop {
        if let Some(maybe_frame) = client.read().await {
            match maybe_frame {
                Ok(frame) => match frame.opcode() {
                    OpCode::Text | OpCode::Binary => {
                        let payload = frame.payload_data_unmask();
                        let echo = Frame::new_with_payload(frame.opcode(), &payload);
                        client.write(echo).await?;
                    }
                    OpCode::Close => {
                        client.close(1000, "".to_string()).await?;
                        break;
                    }
                    OpCode::Ping => {
                        let mut echo = Frame::new_with_opcode(OpCode::Pong);
                        echo.set_payload(&frame.payload_data_unmask());
                        client.write(echo).await?;
                    }
                    OpCode::Pong => {}
                    OpCode::Continue | OpCode::ReservedNonControl | OpCode::ReservedControl => {
                        unreachable!()
                    }
                },
                Err(e) => {
                    client.close(1000, e.to_string()).await?;
                }
            }
        } else {
            client.close(1000, "".to_string()).await?;
            break;
        }
    }

    Ok(())
}

async fn update_report() -> IOResult<()> {
    let mut client = ConnBuilder::new(&format!(
        "ws://localhost:9002/updateReports?agent={}",
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
