use std::fs::File;
use std::io::Write;
use std::sync::Arc;
use std::{fs::create_dir_all, path::PathBuf};

use clap::Parser;
use tokio_rustls::rustls::{self, Certificate, PrivateKey};
use tokio_rustls::TlsAcceptor;
use tracing_subscriber::util::SubscriberInitExt;
use ws_tool::codec::AsyncWsStringCodec;
use ws_tool::{codec::default_handshake_handler, ServerBuilder};

/// websocket client connect to binance futures websocket
#[derive(Parser)]
struct Args {
    /// server host
    #[clap(long, default_value = "127.0.0.1")]
    host: String,
    /// server port
    #[clap(short, long, default_value = "9000")]
    port: u16,
    /// relative path from workspace dir for certs
    #[clap(short, long, default_value = "certs")]
    cert: PathBuf,

    /// enable ssl
    #[clap(short, long)]
    ssl: bool,

    /// level
    #[clap(short, long, default_value = "info")]
    level: tracing::Level,
}

#[tokio::main]
async fn main() -> Result<(), ()> {
    let args = Args::parse();
    tracing_subscriber::fmt::fmt()
        .with_max_level(args.level)
        .with_file(true)
        .with_line_number(true)
        .finish()
        .try_init()
        .expect("failed to init log");
    if args.ssl {
        let cert = rcgen::generate_simple_self_signed(vec![args.host.clone()])
            .expect("unable to generate certs");
        let mut cert_dir = std::env::current_dir().expect("failed to get current work dir");
        cert_dir.push(args.cert);
        if !cert_dir.exists() {
            create_dir_all(&cert_dir).expect("failed to create cert dir");
        }
        let mut cert_file_path = cert_dir.clone();
        cert_file_path.push("certs.pem");
        let mut cert_file = File::create(&cert_file_path).expect("failed to create cert file");
        let cert_content = cert.serialize_pem().unwrap();
        cert_file
            .write_all(cert_content.as_bytes())
            .expect("fail to write cert file");
        cert_file.sync_all().unwrap();
        tracing::info!("cert file saved at {}", cert_file_path.display());

        let mut key_file_path = cert_dir.clone();
        key_file_path.push("key.pem");
        let mut key_file = File::create(&key_file_path).expect("failed to create key file");
        let key_content = cert.serialize_private_key_pem();
        key_file
            .write_all(key_content.as_bytes())
            .expect("fail to write key file");
        key_file.sync_all().unwrap();
        tracing::info!("key file saved at {}", key_file_path.display());

        let cert_file = File::open(cert_file_path).unwrap();
        let mut reader = std::io::BufReader::new(cert_file);
        let certs = rustls_pemfile::certs(&mut reader)
            .unwrap()
            .into_iter()
            .map(Certificate)
            .collect();
        let key_file = File::open(key_file_path).unwrap();
        let mut reader = std::io::BufReader::new(key_file);
        let mut keys = rustls_pemfile::pkcs8_private_keys(&mut reader).unwrap();
        let config = rustls::ServerConfig::builder()
            .with_safe_defaults()
            .with_no_client_auth()
            .with_single_cert(
                // vec![Certificate(cert_content.into())],
                // PrivateKey(key_content.into()),
                certs,
                PrivateKey(keys.remove(0)),
            )
            .expect("failed to init ssl context");

        let accepter = TlsAcceptor::from(Arc::new(config));
        tracing::info!("binding on {}:{}", args.host, args.port);
        let listener = tokio::net::TcpListener::bind(format!("{}:{}", args.host, args.port))
            .await
            .unwrap();
        let (stream, addr) = listener.accept().await.unwrap();
        let stream = accepter.accept(stream).await.unwrap();
        tracing::info!("got connect from {:?}", addr);
        let mut server = ServerBuilder::async_accept(
            stream,
            default_handshake_handler,
            // AsyncWsStringCodec::factory,
            AsyncWsStringCodec::factory,
        )
        .await
        .unwrap();

        loop {
            if let Ok(msg) = server.receive().await {
                // if msg.code == OpCode::Close {
                //     break;
                // }
                // server.send(msg).await.unwrap();
                server.send((msg.code, msg.data)).await.unwrap();
            } else {
                break;
            }
        }
    } else {
        tracing::info!("binding on {}:{}", args.host, args.port);
        let listener = tokio::net::TcpListener::bind(format!("{}:{}", args.host, args.port))
            .await
            .unwrap();
        // loop {
        let (stream, addr) = listener.accept().await.unwrap();

        tracing::info!("got connect from {:?}", addr);
        let mut server = ServerBuilder::async_accept(
            stream,
            default_handshake_handler,
            // AsyncWsStringCodec::factory,
            AsyncWsStringCodec::factory,
        )
        .await
        .unwrap();

        loop {
            match server.receive().await {
                Ok(msg) => {
                    server.send((msg.code, msg.data)).await.unwrap()
                }
                Err(e) => {
                    dbg!(e);
                    break;
                }
            }
        }
    }

    Ok(())
}
