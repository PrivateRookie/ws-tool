use thiserror::Error;

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
    #[error("protocol error `{0}`")]
    ProtocolError(String),
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
    #[error("insufficient data, expect {0}, got {1}")]
    UnMatchDataLen(usize, usize),
}
