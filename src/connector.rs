use http::Uri;

use crate::{errors::WsError, protocol::Mode};

/// get websocket scheme
pub fn get_scheme(uri: &http::Uri) -> Result<Mode, WsError> {
    match uri.scheme_str().unwrap_or("ws").to_lowercase().as_str() {
        "ws" => Ok(Mode::WS),
        "wss" => Ok(Mode::WSS),
        s => Err(WsError::InvalidUri(format!("unknown scheme {s}"))),
    }
}

/// get host from uri
pub fn get_host(uri: &Uri) -> Result<&str, WsError> {
    uri.host()
        .ok_or_else(|| WsError::InvalidUri(format!("can not find host {}", uri)))
}

#[cfg(feature = "sync")]
mod blocking {
    use std::net::TcpStream;

    use crate::errors::WsError;

    use super::{get_host, get_scheme};

    #[cfg(feature = "sync_tls_rustls")]
    pub type TlsStream = rustls_connector::TlsStream<TcpStream>;

    /// performance tcp connection
    pub fn tcp_connect(uri: &http::Uri) -> Result<TcpStream, WsError> {
        let mode = get_scheme(uri)?;
        let host = get_host(uri)?;
        let port = uri.port_u16().unwrap_or_else(|| mode.default_port());
        let stream = TcpStream::connect((host, port)).map_err(|e| {
            WsError::ConnectionFailed(format!("failed to create tcp connection {e}"))
        })?;
        Ok(stream)
    }

    #[cfg(feature = "sync_tls_rustls")]
    /// start tls session
    pub fn wrap_tls(
        stream: TcpStream,
        host: &str,
        certs: Vec<std::path::PathBuf>,
    ) -> Result<TlsStream, WsError> {
        let mut config = rustls_connector::RustlsConnectorConfig::new_with_webpki_roots_certs();
        let mut cert_data = vec![];
        for cert_path in certs.iter() {
            let mut pem = std::fs::File::open(cert_path).map_err(|_| {
                WsError::CertFileNotFound(cert_path.to_str().unwrap_or_default().to_string())
            })?;
            let mut data = vec![];
            if let Err(e) = std::io::Read::read_to_end(&mut pem, &mut data) {
                tracing::error!(
                    "failed to read cert file {} {}",
                    cert_path.display(),
                    e.to_string()
                );
                continue;
            }
            cert_data.push(data);
        }
        config.add_parsable_certificates(&cert_data);
        let connector = config.connector_with_no_client_auth();
        let tls_stream = connector
            .connect(host, stream)
            .map_err(|e| WsError::ConnectionFailed(e.to_string()))?;
        tracing::debug!("tls connection established");
        Ok(tls_stream)
    }
}

#[cfg(feature = "sync")]
pub use blocking::*;

#[cfg(feature = "async")]
mod non_blocking {
    use http::Uri;
    use tokio::net::TcpStream;

    #[cfg(feature = "async_tls_rustls")]
    pub type TlsStream = tokio_rustls::client::TlsStream<TcpStream>;

    use crate::errors::WsError;

    use super::{get_host, get_scheme};

    /// performance tcp connection
    pub async fn async_tcp_connect(uri: &Uri) -> Result<TcpStream, WsError> {
        let mode = get_scheme(uri)?;
        let host = get_host(uri)?;
        let port = uri.port_u16().unwrap_or_else(|| mode.default_port());

        TcpStream::connect((host, port))
            .await
            .map_err(|e| WsError::ConnectionFailed(format!("failed to create tcp connection {e}")))
    }

    #[cfg(feature = "async_tls_rustls")]
    /// async version of starting tls session
    pub async fn async_wrap_tls(
        stream: TcpStream,
        host: &str,
        certs: Vec<std::path::PathBuf>,
    ) -> Result<TlsStream, WsError> {
        use std::io::BufReader;

        let mut root_store = rustls_connector::rustls::RootCertStore::empty();
        root_store.add_server_trust_anchors(webpki_roots::TLS_SERVER_ROOTS.0.iter().map(|ta| {
            rustls_connector::rustls::OwnedTrustAnchor::from_subject_spki_name_constraints(
                ta.subject,
                ta.spki,
                ta.name_constraints,
            )
        }));
        let mut trust_anchors = vec![];
        for cert_path in certs.iter() {
            let mut pem = std::fs::File::open(cert_path).map_err(|_| {
                WsError::CertFileNotFound(cert_path.to_str().unwrap_or_default().to_string())
            })?;
            let mut cert = BufReader::new(&mut pem);
            let certs = rustls_pemfile::certs(&mut cert)
                .map_err(|e| WsError::LoadCertFailed(e.to_string()))?;
            for item in certs {
                let ta = webpki::TrustAnchor::try_from_cert_der(&item[..])
                    .map_err(|e| WsError::LoadCertFailed(e.to_string()))?;
                let anchor =
                    rustls_connector::rustls::OwnedTrustAnchor::from_subject_spki_name_constraints(
                        ta.subject,
                        ta.spki,
                        ta.name_constraints,
                    );
                trust_anchors.push(anchor);
            }
        }
        root_store.add_server_trust_anchors(trust_anchors.into_iter());
        let config = rustls_connector::rustls::ClientConfig::builder()
            .with_safe_defaults()
            .with_root_certificates(root_store)
            .with_no_client_auth();
        let domain = tokio_rustls::rustls::ServerName::try_from(host)
            .map_err(|e| WsError::TlsDnsFailed(e.to_string()))?;
        let connector = tokio_rustls::TlsConnector::from(std::sync::Arc::new(config));
        let tls_stream = connector
            .connect(domain, stream)
            .await
            .map_err(|e| WsError::ConnectionFailed(e.to_string()))?;
        tracing::debug!("tls connection established");
        Ok(tls_stream)
    }
}

#[cfg(feature = "async")]
pub use non_blocking::*;
