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
    client.connect().await?;
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
    client.connect().await?;
    loop {
        let frame = client.read_frame().await?;
        match frame.opcode() {
            OpCode::Text => {
                let payload = frame.payload_data_unmask();
                match String::from_utf8(payload.to_vec()) {
                    Ok(_) => {
                        let echo = Frame::new_with_payload(OpCode::Text, &payload);
                        client.write_frame(echo).await?
                    }
                    Err(_) => {
                        client.close(1007, "invalid utf-8 text".to_string()).await?;
                        return Err(WsError::ProtocolError("invalid utf-8 text".to_string()));
                    }
                }
            }
            OpCode::Binary => {
                let mut echo = Frame::new_with_opcode(frame.opcode());
                echo.set_payload(&frame.payload_data_unmask());
                client.write_frame(echo).await?;
            }
            OpCode::Close => {
                let payload = frame.payload_data_unmask();
                if payload.is_empty() {
                    break;
                } else {
                    if let Err(_) = String::from_utf8(payload[2..].to_vec()) {
                        client.close(1007, "invalid utf-8 text".to_string()).await?;
                        return Err(WsError::ProtocolError("invalid utf-8 text".to_string()));
                    } else {
                        break;
                    }
                }
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
    client.connect().await?;
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
