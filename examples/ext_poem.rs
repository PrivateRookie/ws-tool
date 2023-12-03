use poem::{get, handler, listener::TcpListener, web::Path, IntoResponse, Route, Server};
use tokio::main;
use tracing::{info, Level};
use tracing_subscriber::util::SubscriberInitExt;
use ws_tool::codec::{default_handshake_handler, AsyncStringCodec};

#[handler]
async fn test(Path(prefix): Path<String>, req: &poem::Request) -> impl IntoResponse {
    ws_tool::extension::poem_ext::adapt(
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

#[main]
pub async fn main() -> Result<(), std::io::Error> {
    tracing_subscriber::fmt::fmt()
        .with_max_level(Level::INFO)
        .finish()
        .try_init()
        .expect("failed to init log");

    let app = Route::new().at("/demo/:prefix", get(test));
    Server::new(TcpListener::bind("127.0.0.1:3000"))
        .run(app)
        .await
}
