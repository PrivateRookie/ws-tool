use socks::Socks5Stream;
use tracing_subscriber::util::SubscriberInitExt;
use ws_tool::{
    codec::{DeflateCodec, PMDConfig, WindowBit},
    frame::OpCode,
    ClientBuilder, ClientConfig,
};

fn main() {
    tracing_subscriber::fmt::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .finish()
        .try_init()
        .expect("failed to init log");

    let url = std::env::var("URL").expect("env URL not set");
    let uri = url.parse::<http::Uri>().unwrap();
    let host = uri.host().unwrap();
    let proxy_addr = std::env::var("SOCKS5_PROXY").expect("env SOCKS5_PROXY not set");
    let mut stream =
        Socks5Stream::connect(proxy_addr.trim_start_matches("socks5://"), (host, 443)).unwrap();
    stream
        .get_mut()
        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .unwrap();
    let stream = ws_tool::connector::wrap_rustls(stream, host, Vec::new()).unwrap();

    let pmd_config = PMDConfig {
        server_no_context_takeover: ClientConfig::default().context_take_over,
        client_no_context_takeover: ClientConfig::default().context_take_over,
        server_max_window_bits: WindowBit::Fifteen,
        client_max_window_bits: WindowBit::Fifteen,
    };
    let mut stream = ClientBuilder::new()
        .extension(pmd_config.ext_string())
        .with_stream(uri, stream, DeflateCodec::check_fn)
        .unwrap();
    loop {
        let (header, data) = stream.receive().unwrap();
        match &header.code {
            OpCode::Text => {
                let data = String::from_utf8(data.to_vec()).unwrap();
                tracing::info!("receive {data}");
            }
            OpCode::Close => {
                stream.send(OpCode::Close, &[]).unwrap();
                tracing::info!("receive Close");
                break;
            }
            OpCode::Ping => {
                stream.send(OpCode::Pong, &[]).unwrap();
            }
            OpCode::Pong => {}
            _ => {
                unreachable!()
            }
        }
    }
}
