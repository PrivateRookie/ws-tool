use axum::{extract::Path, response::Response, routing::get, Router};
use tracing::{info, Level};
use tracing_subscriber::util::SubscriberInitExt;
use ws_tool::codec::{default_handshake_handler, AsyncStringCodec};

async fn ext(Path(prefix): Path<String>, req: axum::extract::Request) -> Response {
    ws_tool::extension::axum_ext::adapt(
        req,
        default_handshake_handler,
        |req, upgraded| async move {
            let mut client = AsyncStringCodec::factory(req, upgraded).unwrap();
            loop {
                let msg = client.receive().await.unwrap();
                if msg.code.is_close() {
                    info!("peer send close: {}", msg.data);
                    break;
                }
                let echo = format!("{}: {}", prefix, msg.data);
                client.send(&echo).await.unwrap()
            }
        },
    )
    .await
}

#[tokio::main]
pub async fn main() {
    tracing_subscriber::fmt::fmt()
        .with_max_level(Level::INFO)
        .finish()
        .try_init()
        .expect("failed to init log");

    let app = Router::new().route("/demo/:prefix", get(ext));

    // run our app with hyper, listening globally on port 3000
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
