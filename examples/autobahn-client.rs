use futures::SinkExt;
use tokio_stream::StreamExt;
use tracing::*;
use ws_tool::{
    codec::{default_frame_check_fn, default_string_check_fn, send_close},
    errors::WsError,
    frame::{Frame, OpCode},
    ClientBuilder,
};

const AGENT: &str = "ws-tool-client";

async fn get_case_count() -> Result<usize, WsError> {
    let mut client = ClientBuilder::new("ws://localhost:9002/getCaseCount")
        .connect_with_check(default_string_check_fn)
        .await
        .unwrap();
    let (_, count) = client.next().await.unwrap().unwrap();
    client.next().await.unwrap().unwrap();
    // send_close(&mut client, 1001, "".to_string()).await.unwrap();
    Ok(count.parse().unwrap())
}

async fn run_test(case: usize) -> Result<(), WsError> {
    info!("running test case {}", case);
    let url = format!("ws://localhost:9002/runCase?case={}&agent={}", case, AGENT);
    let mut client = ClientBuilder::new(&url)
        .connect_with_check(default_frame_check_fn)
        .await
        .unwrap();
    loop {
        if let Some(maybe_frame) = client.next().await {
            match maybe_frame {
                Ok(frame) => match frame.opcode() {
                    OpCode::Text | OpCode::Binary => {
                        let payload = frame.payload_data_unmask();
                        let echo = Frame::new_with_payload(frame.opcode(), &payload);
                        client.send(echo).await?;
                    }
                    OpCode::Close => {
                        send_close(&mut client, 1000, "".to_string()).await?;
                        break;
                    }
                    OpCode::Ping => {
                        let mut echo = Frame::new_with_opcode(OpCode::Pong);
                        echo.set_payload(&frame.payload_data_unmask());
                        client.send(echo).await?;
                    }
                    OpCode::Pong => {}
                    OpCode::Continue | OpCode::ReservedNonControl | OpCode::ReservedControl => {
                        unreachable!()
                    }
                },
                Err(e) => match e {
                    WsError::ProtocolError { close_code, error } => {
                        send_close(&mut client, close_code, error.to_string()).await?;
                    }
                    _ => {
                        send_close(&mut client, 1000, e.to_string()).await?;
                    }
                },
            }
        } else {
            send_close(&mut client, 1000, "".to_string()).await?;
            break;
        }
    }

    Ok(())
}

async fn update_report() -> Result<(), WsError> {
    let mut client = ClientBuilder::new(&format!(
        "ws://localhost:9002/updateReports?agent={}",
        AGENT
    ))
    .connect_with_check(default_frame_check_fn)
    .await
    .unwrap();
    send_close(&mut client, 1000, "".to_string()).await
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
