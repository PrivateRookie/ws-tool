use thiserror::Error;

use crate::{frame::OpCode, ConnectionState};

#[derive(Debug, Error)]
pub enum WsError {
    #[error("invalid uri `{0}`")]
    InvalidUri(String),
    #[error("unsupported proxy, expect socks5 or http, got {0}")]
    UnsupportedProxy(String),
    #[error("invalid proxy {0}")]
    InvalidProxy(String),
    #[error("cert {0} not found")]
    CertFileNotFound(String),
    #[error("load cert {0} failed")]
    LoadCertFailed(String),
    #[error("connection failed `{0}`")]
    ConnectionFailed(String),
    #[error("tls dns lookup failed `{0}`")]
    TlsDnsFailed(String),
    #[error("io error {0}")]
    IOError(String),
    #[error("{0}")]
    HandShakeFailed(String),
    #[error("{0:?}")]
    ProtocolError(ProtocolError),
    #[error("proxy error `{0}`")]
    ProxyError(String),
    #[error("io on invalid connection state {0:?}")]
    InvalidConnState(ConnectionState),
    #[error("unsupported frame {0:?}")]
    UnsupportedFrame(OpCode),
}

#[derive(Debug, Error)]
pub enum ProtocolError {
    #[error("insufficient data len {0}")]
    InsufficientLen(usize),
    #[error("invalid leading bits {0:b}")]
    InvalidLeadingBits(u8),
    #[error("invalid opcode {0}")]
    InvalidOpcode(u8),
    #[error("invalid leading payload len {0}")]
    InvalidLeadingLen(u8),
    #[error("mismatch data len, expect {0}, got {1}")]
    MisMatchDataLen(usize, usize),
    #[error("missing init fragmented frame")]
    MissInitialFragmentedFrame,
    #[error("not continue frame after init fragmented frame")]
    NotContinueFrameAfterFragmented,
    #[error("fragmented control frame ")]
    FragmentedControlFrame,
    #[error("control frame is too big {0}")]
    ControlFrameTooBig(usize),
    #[error("invalid close frame payload len, expect 0, >= 2")]
    InvalidCloseFramePayload,
    #[error("invalid utf-8 text")]
    InvalidUtf8,
    #[error("invalid close code {0}")]
    InvalidCloseCode(u16),
}
