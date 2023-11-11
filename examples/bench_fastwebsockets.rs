// Copyright 2023 Divy Srivastava <dj.srivastava23@gmail.com>
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use clap::Parser;
use fastwebsockets::upgrade;
use fastwebsockets::OpCode;
use fastwebsockets::WebSocketError;
use hyper::server::conn::Http;
use hyper::service::service_fn;
use hyper::Body;
use hyper::Request;
use hyper::Response;
use tokio::net::TcpListener;
use tracing_subscriber::util::SubscriberInitExt;

async fn handle_client(fut: upgrade::UpgradeFut) -> Result<(), WebSocketError> {
    let ws = fut.await?;
    let mut ws = fastwebsockets::FragmentCollector::new(ws);

    loop {
        let frame = ws.read_frame().await?;
        match frame.opcode {
            OpCode::Close => break,
            OpCode::Text | OpCode::Binary => {
                ws.write_frame(frame).await?;
            }
            _ => {}
        }
    }

    Ok(())
}
async fn server_upgrade(mut req: Request<Body>) -> Result<Response<Body>, WebSocketError> {
    let (response, fut) = upgrade::upgrade(&mut req)?;

    tokio::task::spawn(async move {
        if let Err(e) = tokio::task::unconstrained(handle_client(fut)).await {
            eprintln!("Error in websocket connection: {}", e);
        }
    });

    Ok(response)
}

/// websocket client connect to binance futures websocket
#[derive(Parser)]
struct Args {
    /// server host
    #[arg(long, default_value = "127.0.0.1")]
    host: String,
    /// server port
    #[arg(short, long, default_value = "9000")]
    port: u16,

    /// level
    #[arg(short, long, default_value = "info")]
    level: tracing::Level,
}

fn main() -> Result<(), WebSocketError> {
    let args = Args::parse();
    tracing_subscriber::fmt::fmt()
        .with_max_level(args.level)
        .finish()
        .try_init()
        .expect("failed to init log");

    tracing::info!("binding on {}:{}", args.host, args.port);
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .build()
        .unwrap();

    rt.block_on(async move {
        let listener = TcpListener::bind(format!("{}:{}", args.host, args.port)).await?;
        println!(
            "Server started, listening on {}",
            format!("{}:{}", args.host, args.port)
        );
        loop {
            let (stream, _) = listener.accept().await?;
            println!("Client connected");
            tokio::spawn(async move {
                let conn_fut = Http::new()
                    .serve_connection(stream, service_fn(server_upgrade))
                    .with_upgrades();
                if let Err(e) = conn_fut.await {
                    println!("An error occurred: {:?}", e);
                }
            });
        }
    })
}
