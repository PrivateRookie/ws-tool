use log::*;
use ws_client::{
    errors::WsError,
    frame::{Frame, OpCode},
    ConnBuilder,
};

const AGENT: &str = "ws-client";

async fn get_case_count() -> Result<usize, WsError> {
    let mut client = ConnBuilder::new("ws://localhost:9001/getCaseCount")
        .build()
        .await?;
    client.handshake().await?;
    let frame = client.read_frame().await?;
    let unmask_data = frame.payload_data_unmask();
    let count = String::from_utf8(unmask_data.to_vec())
        .unwrap()
        .parse::<usize>()
        .unwrap();
    client.read_frame().await?;
    client.close(1001, "".to_string()).await?;
    Ok(count)
}

async fn run_test(case: usize) -> Result<(), WsError> {
    info!("running test case {}", case);
    let url = format!("ws://localhost:9001/runCase?case={}&agent={}", case, AGENT);
    let mut client = ConnBuilder::new(&url).build().await?;
    client.handshake().await?;
    loop {
        let frame = client.read_frame().await?;
        match frame.opcode() {
            OpCode::Text => {
                let payload = frame.payload_data_unmask();
                let echo = Frame::new_with_payload(OpCode::Text, &payload);
                client.write_frame(echo).await?
            }
            OpCode::Binary => {
                let mut echo = Frame::new_with_opcode(frame.opcode());
                echo.set_payload(&frame.payload_data_unmask());
                client.write_frame(echo).await?;
            }
            OpCode::Close => {
                break;
            }
            OpCode::Ping => {
                let mut echo = Frame::new_with_opcode(OpCode::Pong);
                echo.set_payload(&frame.payload_data_unmask());
                client.write_frame(echo).await?;
            }
            OpCode::Pong => {}
            OpCode::Continue | OpCode::ReservedNonControl | OpCode::ReservedControl => {
                unreachable!()
            }
        }
    }
    client.close(1000, "".to_string()).await?;
    Ok(())
}

async fn update_report() -> Result<(), WsError> {
    let mut client = ConnBuilder::new(&format!(
        "ws://localhost:9001/updateReports?agent={}",
        AGENT
    ))
    .build()
    .await?;
    client.handshake().await?;
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
