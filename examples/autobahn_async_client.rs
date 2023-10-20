use bytes::BytesMut;
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
    let mut client = ClientBuilder::new()
        .async_connect(
            "ws://localhost:9002/getCaseCount".parse().unwrap(),
            AsyncStringCodec::check_fn,
        )
        .await
        .unwrap();
    let msg = client.receive().await.unwrap().data.parse().unwrap();
    client.receive().await.unwrap();
    // send_close(&mut client, 1001, "".to_string()).unwrap();
    Ok(msg)
}

async fn run_test(case: usize) -> Result<(), WsError> {
    info!("running test case {}", case);
    let url: http::Uri = format!("ws://localhost:9002/runCase?case={}&agent={}", case, AGENT)
        .parse()
        .unwrap();
    let (mut read, mut write) = ClientBuilder::new()
        .async_connect(url, AsyncFrameCodec::check_fn)
        .await
        .unwrap()
        .split();
    loop {
        match read.receive().await {
            Ok((header, data)) => {
                let code = header.code;
                match &code {
                    OpCode::Text | OpCode::Binary => {
                        write.send(code, data).await?;
                    }
                    OpCode::Close => {
                        let mut data = BytesMut::new();
                        data.extend_from_slice(&1000u16.to_be_bytes());
                        write.send(OpCode::Close, &data).await.unwrap();
                        break;
                    }
                    OpCode::Ping => {
                        write.send(OpCode::Pong, data).await?;
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
                    if write.send(OpCode::Close, &data).await.is_err() {
                        break;
                    }
                }
                e => {
                    tracing::warn!("{e}");
                    write.send(OpCode::Close, &1000u16.to_be_bytes()).await.ok();
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
    let mut client = ClientBuilder::new()
        .async_connect(url, AsyncStringCodec::check_fn)
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
