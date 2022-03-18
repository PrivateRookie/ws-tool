use bytes::BytesMut;
use tracing::*;
use tracing_subscriber::util::SubscriberInitExt;
use ws_tool::{
    codec::{AsyncWsFrameCodec, AsyncWsStringCodec},
    errors::WsError,
    frame::OpCode,
    ClientBuilder,
};

const AGENT: &str = "ws-tool-client";

async fn get_case_count() -> Result<usize, WsError> {
    let mut client = ClientBuilder::new("ws://localhost:9002/getCaseCount")
        .async_connect(AsyncWsStringCodec::check_fn)
        .await
        .unwrap();
    let msg = client.receive().await.unwrap();
    client.receive().await.unwrap();
    // send_close(&mut client, 1001, "".to_string()).await.unwrap();
    Ok(msg.data.parse().unwrap())
}

async fn run_test(case: usize) -> Result<(), WsError> {
    info!("running test case {}", case);
    let url = format!("ws://localhost:9002/runCase?case={}&agent={}", case, AGENT);
    let mut client = ClientBuilder::new(&url)
        .async_connect(AsyncWsFrameCodec::check_fn)
        .await
        .unwrap();
    loop {
        match client.receive().await {
            Ok(frame) => {
                let (header, data) = frame.split();
                let code = header.opcode();
                match &code {
                    OpCode::Text | OpCode::Binary => {
                        client.send(code, &data).await?;
                    }
                    OpCode::Close => {
                        let mut data = BytesMut::new();
                        data.extend_from_slice(&1000u16.to_be_bytes());
                        client.send(OpCode::Close, &data).await.unwrap();
                        break;
                    }
                    OpCode::Ping => {
                        client.send(code, &data).await?;
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
                    data.extend_from_slice(&error.to_string().as_bytes());
                    client
                        .send(OpCode::Close, vec![&close_code.to_be_bytes()[..], &data])
                        .await
                        .unwrap();
                }
                _ => {
                    let mut data = BytesMut::new();
                    data.extend_from_slice(&1000u16.to_be_bytes());
                    client.send(OpCode::Close, &data).await.unwrap();
                }
            },
        }
    }

    Ok(())
}

async fn update_report() -> Result<(), WsError> {
    let mut client = ClientBuilder::new(&format!(
        "ws://localhost:9002/updateReports?agent={}",
        AGENT
    ))
    .async_connect(AsyncWsStringCodec::check_fn)
    .await
    .unwrap();
    client.send((1000u16, String::new())).await.map(|_| ())
}

#[tokio::main]
async fn main() -> Result<(), ()> {
    tracing_subscriber::fmt::fmt()
        .with_max_level(Level::DEBUG)
        .finish()
        .try_init()
        .expect("failed to init log");
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
