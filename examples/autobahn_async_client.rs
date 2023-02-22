use bytes::BytesMut;
use tokio::net::TcpStream;
use tracing::*;
use tracing_subscriber::util::SubscriberInitExt;
use ws_tool::{
    codec::{AsyncFrameCodec, AsyncStringCodec},
    errors::WsError,
    frame::OpCode,
    ClientBuilder,
};

const AGENT: &str = "async-client";

async fn get_case_count() -> Result<usize, WsError> {
    let stream = TcpStream::connect("localhost:9002").await.unwrap();
    let mut client = ClientBuilder::new()
        .async_connect(
            "ws://localhost:9002/getCaseCount".parse().unwrap(),
            stream,
            AsyncStringCodec::check_fn,
        )
        .await
        .unwrap();
    let msg = client.receive().await.unwrap();
    client.receive().await.unwrap();
    // send_close(&mut client, 1001, "".to_string()).unwrap();
    Ok(msg.data.parse().unwrap())
}

async fn run_test(case: usize) -> Result<(), WsError> {
    info!("running test case {}", case);
    let url: http::Uri = format!("ws://localhost:9002/runCase?case={}&agent={}", case, AGENT)
        .parse()
        .unwrap();
    let stream = TcpStream::connect("localhost:9002").await.unwrap();
    let mut client = ClientBuilder::new()
        .async_connect(url, stream, AsyncFrameCodec::check_fn)
        .await
        .unwrap();
    loop {
        match client.receive().await {
            Ok(frame) => {
                let code = frame.header().opcode();
                match &code {
                    OpCode::Text | OpCode::Binary => {
                        client.send(code, frame.payload()).await?;
                    }
                    OpCode::Close => {
                        let mut data = BytesMut::new();
                        data.extend_from_slice(&1000u16.to_be_bytes());
                        client.send(OpCode::Close, &data).await.unwrap();
                        break;
                    }
                    OpCode::Ping => {
                        client.send(OpCode::Pong, frame.payload()).await?;
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
                    if client.send(OpCode::Close, &data).await.is_err() {
                        break;
                    }
                }
                e => {
                    tracing::warn!("{e}");
                    client
                        .send(OpCode::Close, &1000u16.to_be_bytes())
                        .await
                        .ok();
                    break;
                }
            },
        }
    }

    Ok(())
}

async fn update_report() -> Result<(), WsError> {
    let url: http::Uri = format!("ws://localhost:9002/updateReports?agent={}", AGENT)
        .parse()
        .unwrap();
    let stream = TcpStream::connect("localhost:9002").await.unwrap();
    let mut client = ClientBuilder::new()
        .async_connect(url, stream, AsyncStringCodec::check_fn)
        .await
        .unwrap();
    client.send((1000u16, String::new())).await.map(|_| ())
}

#[tokio::main]
async fn main() -> Result<(), ()> {
    tracing_subscriber::fmt::fmt()
        .with_max_level(Level::INFO)
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
