use std::{collections::HashMap, net::TcpStream, path::PathBuf, str::FromStr};

use pyo3::{exceptions::PyValueError, prelude::*};
use ws_tool::{
    codec::{DeflateCodec, PMDConfig, WindowBit},
    connector::{get_host, get_scheme, tcp_connect, wrap_rustls},
    errors::WsError,
    frame::{OpCode, SimplifiedHeader},
    protocol::Mode,
    stream::BufStream,
    ClientBuilder,
};

/// websocket client
#[pyclass]
struct Client {
    client: InnerClient,
}

/// websocket message type
#[pyclass]
#[derive(Debug, Clone)]
pub enum MessageCode {
    Text,
    Binary,
    Ping,
    Pong,
    Close,
}

#[pymethods]
impl MessageCode {
    pub fn is_text(&self) -> bool {
        matches!(self, MessageCode::Text)
    }

    pub fn is_binary(&self) -> bool {
        matches!(self, MessageCode::Binary)
    }

    pub fn is_close(&self) -> bool {
        matches!(self, MessageCode::Close)
    }

    pub fn is_data(&self) -> bool {
        matches!(self, MessageCode::Text | MessageCode::Binary)
    }
}

#[pymethods]
impl Client {
    /// send raw message
    fn _send(&mut self, code: MessageCode, payload: &[u8]) -> PyResult<()> {
        let code = match code {
            MessageCode::Text => OpCode::Text,
            MessageCode::Binary => OpCode::Binary,
            MessageCode::Ping => OpCode::Ping,
            MessageCode::Pong => OpCode::Pong,
            MessageCode::Close => OpCode::Close,
        };
        self.client.send(code, payload).map_err(WrapError)?;
        Ok(())
    }

    /// send text message
    fn text(&mut self, payload: &str) -> PyResult<()> {
        self._send(MessageCode::Text, payload.as_bytes())
    }

    /// send binary message
    fn binary(&mut self, payload: &[u8]) -> PyResult<()> {
        self._send(MessageCode::Binary, payload)
    }

    /// send ping message
    fn ping(&mut self, payload: &str) -> PyResult<()> {
        self._send(MessageCode::Ping, payload.as_bytes())
    }

    /// send pong message
    fn pong(&mut self, payload: &str) -> PyResult<()> {
        self._send(MessageCode::Pong, payload.as_bytes())
    }

    /// send close message, if code is not set, use default 1000
    fn close(&mut self, close_code: Option<u16>, payload: Option<&str>) -> PyResult<()> {
        let mut data = close_code.unwrap_or(1000).to_be_bytes().to_vec();
        data.extend_from_slice(payload.unwrap_or_default().as_bytes());
        self._send(MessageCode::Close, &data)
    }

    /// flush underlying stream
    fn flush(&mut self) -> PyResult<()> {
        self.client.flush().map_err(WrapError)?;
        Ok(())
    }

    fn recv(&mut self) -> PyResult<(MessageCode, &[u8])> {
        let (header, data) = self.client.recv().map_err(WrapError)?;
        let code = match header.code {
            OpCode::Text => MessageCode::Text,
            OpCode::Binary => MessageCode::Binary,
            OpCode::Ping => MessageCode::Ping,
            OpCode::Pong => MessageCode::Pong,
            OpCode::Close => MessageCode::Close,
            _ => unreachable!(),
        };
        Ok((code, data))
    }
}

enum InnerClient {
    Raw(DeflateCodec<BufStream<TcpStream>>),
    Ssl(DeflateCodec<BufStream<rustls_connector::TlsStream<TcpStream>>>),
}

impl InnerClient {
    fn send(&mut self, code: OpCode, payload: &[u8]) -> Result<(), WsError> {
        match self {
            InnerClient::Raw(c) => c.send(code, payload),
            InnerClient::Ssl(c) => c.send(code, payload),
        }
    }

    fn recv(&mut self) -> Result<(SimplifiedHeader, &[u8]), WsError> {
        match self {
            InnerClient::Raw(c) => c.receive(),
            InnerClient::Ssl(c) => c.receive(),
        }
    }

    fn flush(&mut self) -> Result<(), WsError> {
        match self {
            InnerClient::Raw(c) => c.flush(),
            InnerClient::Ssl(c) => c.flush(),
        }
    }
}

#[derive(Debug)]
struct WrapError(WsError);

impl From<WrapError> for PyErr {
    fn from(value: WrapError) -> Self {
        PyValueError::new_err(format!("{}", value.0))
    }
}

impl From<WsError> for WrapError {
    fn from(value: WsError) -> Self {
        Self(value)
    }
}

#[pyclass]
#[derive(Debug, Clone)]
struct ClientConfig {
    buf: usize,
    window: Option<u8>,
    context_take_over: bool,
    certs: Vec<PathBuf>,
    extra_headers: HashMap<String, String>,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            buf: 8192,
            window: Default::default(),
            context_take_over: Default::default(),
            certs: Default::default(),
            extra_headers: Default::default(),
        }
    }
}

#[pymethods]
impl ClientConfig {
    #[new]
    fn new(
        buf: Option<usize>,
        window: Option<u8>,
        context_take_over: Option<bool>,
        certs: Option<Vec<PathBuf>>,
        extra_headers: Option<HashMap<String, String>>,
    ) -> Self {
        Self {
            buf: buf.unwrap_or(8192),
            window,
            context_take_over: context_take_over.unwrap_or_default(),
            certs: certs.unwrap_or_default(),
            extra_headers: extra_headers.unwrap_or_default(),
        }
    }
}

#[pyfunction]
fn connect(url: &str, config: Option<ClientConfig>) -> PyResult<Client> {
    let config = config.unwrap_or_default();
    let uri =
        http::Uri::from_str(url).map_err(|_| PyValueError::new_err(format!("invalid url")))?;
    let mode = get_scheme(&uri).map_err(WrapError)?;
    let pmd_config = config.window.map(|w| PMDConfig {
        server_no_context_takeover: config.context_take_over,
        client_no_context_takeover: config.context_take_over,
        server_max_window_bits: WindowBit::try_from(w).unwrap_or(WindowBit::Fifteen),
        client_max_window_bits: WindowBit::try_from(w).unwrap_or(WindowBit::Fifteen),
    });
    let mut builder = ClientBuilder::new();
    if let Some(conf) = pmd_config {
        builder = builder.extension(conf.ext_string())
    }
    let inner = match mode {
        Mode::WS => {
            let c = builder
                .headers(config.extra_headers)
                .connect(uri, |key, resp, stream| {
                    let stream = BufStream::with_capacity(config.buf, config.buf, stream);
                    DeflateCodec::check_fn(key, resp, stream)
                })
                .map_err(WrapError)?;
            InnerClient::Raw(c)
        }
        Mode::WSS => {
            let host = get_host(&uri).map_err(|_| PyValueError::new_err("invalid host"))?;
            let stream = tcp_connect(&uri).map_err(WrapError)?;
            let stream = wrap_rustls(stream, host, config.certs).map_err(WrapError)?;
            let stream = BufStream::with_capacity(config.buf, config.buf, stream);
            let c = builder
                .headers(config.extra_headers)
                .with_stream(uri, stream, DeflateCodec::check_fn)
                .map_err(WrapError)?;
            InnerClient::Ssl(c)
        }
    };
    Ok(Client { client: inner })
}

/// A Python module implemented in Rust.
#[pymodule]
fn py_ws(_py: Python, m: &PyModule) -> PyResult<()> {
    pyo3_log::init();
    m.add_class::<MessageCode>()?;
    m.add_class::<ClientConfig>()?;
    m.add_function(wrap_pyfunction!(connect, m)?)?;
    Ok(())
}

#[pyclass]
/// client connection config
pub struct NClientConfig {
    /// read buffer size
    pub read_buf: usize,
    /// write buffer size
    pub write_buf: usize,
    /// custom certification path
    pub certs: Vec<PathBuf>,
    /// deflate window size, if none, deflate will be disabled
    pub window: Option<WindowBit>,
    /// enable/disable deflate context taker over parameter
    pub context_take_over: bool,
    /// extra header when perform websocket protocol handshake
    pub extra_headers: HashMap<String, String>,
    /// modified socket option after create tcp socket, this function will be applied
    /// before start tls session
    pub set_socket_fn: Box<dyn FnMut(&std::net::TcpStream) -> Result<(), WsError> + Send>,
}
