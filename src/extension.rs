/// poem websocket extension
#[cfg(feature = "poem")]
pub mod poem_ext {

    use std::future::Future;

    use poem::Body;

    use crate::errors::WsError;

    fn convert<T: Into<Body>>(resp: http::Response<T>) -> poem::Response {
        let (parts, body) = resp.into_parts();
        poem::Response::from_parts(
            poem::ResponseParts {
                status: parts.status,
                version: parts.version,
                headers: parts.headers,
                extensions: parts.extensions,
            },
            body.into(),
        )
    }

    /// accept poem raw request
    pub async fn adapt<T, F1, F2, Fut>(
        req: &poem::Request,
        mut handshake_handler: F1,
        callback: F2,
    ) -> poem::Response
    where
        F1: FnMut(
            http::Request<()>,
        )
            -> Result<(http::Request<()>, http::Response<T>), (http::Response<T>, WsError)>,
        F2: FnOnce(http::Request<()>, poem::Upgraded) -> Fut + Send + Sync + 'static,
        Fut: Future + Send + 'static,
        T: Into<Body> + std::fmt::Debug,
    {
        let on_upgrade = match req.take_upgrade() {
            Err(e) => {
                tracing::error!("http upgrade failed {e}");
                return poem::Response::builder()
                    .version(http::Version::HTTP_11)
                    .status(http::StatusCode::BAD_REQUEST)
                    .body(());
            }
            Ok(i) => i,
        };

        let mut builder = http::Request::builder().method(req.method()).uri(req.uri());
        for (k, v) in req.headers() {
            builder = builder.header(k, v)
        }
        let req = builder.body(()).unwrap();
        let (req, resp) = match handshake_handler(req) {
            Ok(i) => i,
            Err((resp, e)) => {
                tracing::error!("handshake error {e}");
                return convert(resp);
            }
        };
        tokio::spawn(async move {
            match on_upgrade.await {
                Err(e) => {
                    tracing::error!("http upgrade failed {e}");
                    return;
                }
                Ok(upgraded) => {
                    callback(req, upgraded).await;
                }
            }
        });
        convert(resp)
    }
}
