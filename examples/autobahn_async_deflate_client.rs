use bytes::BytesMut;
use rand::random;
use tracing::*;
use tracing_subscriber::util::SubscriberInitExt;
use ws_tool::{
    codec::{AsyncDeflateCodec, AsyncStringCodec, PMDConfig, WindowBit},
    errors::WsError,
    frame::{OpCode, OwnedFrame},
    ClientBuilder,
};

const AGENT: &str = "async-deflate-client";

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
    Ok(msg)
}

fn mask_key() -> [u8; 4] {
    random()
}

async fn run_test(case: usize) -> Result<(), WsError> {
    info!("running test case {}", case);
    let url: http::Uri = format!("ws://localhost:9002/runCase?case={}&agent={}", case, AGENT)
        .parse()
        .unwrap();
    let config = PMDConfig {
        server_max_window_bits: WindowBit::Nine,
        client_max_window_bits: WindowBit::Nine,
        ..PMDConfig::default()
    };
    let (mut read, mut write) = match ClientBuilder::new()
        .extension(config.ext_string())
        .async_connect(url, AsyncDeflateCodec::check_fn)
        .await
    {
        Ok(client) => client.split(),
        Err(e) => {
            tracing::error!("{e}");
            return Ok(());
        }
    };
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
