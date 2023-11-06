use bytes::BytesMut;
use rand::random;
use tracing::*;
use tracing_subscriber::util::SubscriberInitExt;
use ws_tool::{
    codec::{AsyncStringCodec, WindowBit},
    errors::WsError,
    frame::{OpCode, OwnedFrame},
    ClientConfig,
};

const AGENT: &str = "async-deflate-client";

async fn get_case_count() -> Result<usize, WsError> {
    let mut client = ClientConfig::default()
        .async_connect_with(
            "ws://localhost:9002/getCaseCount",
            AsyncStringCodec::check_fn,
        )
        .await
        .unwrap();
    let msg = client.receive().await.unwrap();
    let msg = msg.data.parse().unwrap();
    Ok(msg)
}

fn mask_key() -> [u8; 4] {
    random()
}

async fn run_test(case: usize) -> Result<(), WsError> {
    info!("running test case {}", case);
    let url = format!("ws://localhost:9002/runCase?case={}&agent={}", case, AGENT);
    let (mut read, mut write) = ClientConfig {
        window: Some(WindowBit::Nine),
        ..Default::default()
    }
    .async_connect(url)
    .await
    .unwrap()
    .split();

    let now = std::time::Instant::now();
    loop {
        match read.receive().await {
            Ok((header, data)) => {
                let code = header.code;
                match &code {
                    OpCode::Text | OpCode::Binary => write.send(code, data).await?,
                    OpCode::Close => {
                        let mut data = BytesMut::new();
                        data.extend_from_slice(&1000u16.to_be_bytes());
                        write.send(OpCode::Close, &data).await.unwrap();
                        tracing::info!("case {case} elapsed {:?}", now.elapsed());
                        break;
                    }
                    OpCode::Ping => write.send(OpCode::Pong, data).await?,
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
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
                e => {
                    tracing::warn!("{e}");
                    write
                        .send_owned_frame(OwnedFrame::close_frame(mask_key(), 1000, &[]))
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
    let url = format!("ws://localhost:9002/updateReports?agent={}", AGENT);
    let mut client = ClientConfig::default().async_connect(url).await.unwrap();
    client.close(1000u16, &[]).await
}

#[tokio::main]
async fn main() -> Result<(), ()> {
    tracing_subscriber::fmt::fmt()
        .with_max_level(Level::INFO)
        .with_file(true)
        .with_line_number(true)
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
