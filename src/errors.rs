use thiserror::Error;

use crate::frame::OpCode;

/// errors during handshake, read/write frame
#[derive(Debug, Error)]
pub enum WsError {
    /// invalid websocket connection url
    #[error("invalid uri `{0}`")]
    InvalidUri(String),
    #[error("unsupported proxy, expect socks5 or http, got {0}")]
    /// invalid cert file path
    CertFileNotFound(String),
    #[error("load cert {0} failed")]
    /// broken certs
    LoadCertFailed(String),
    #[error("connection failed `{0}`")]
    /// failed to connect websocket server
    ConnectionFailed(String),
    #[error("tls dns lookup failed `{0}`")]
    /// tls session DNS is broken
    TlsDnsFailed(String),
    #[error("io error {0:?}")]
    /// raised by underlying stream
    IOError(Box<dyn std::error::Error + Send + Sync>),
    #[error("{0}")]
    /// invalid protocol handshake
    HandShakeFailed(String),
    /// websocket protocol handshake
    #[error("{error:?}")]
    ProtocolError {
        /// peer close code
        close_code: u16,
        /// detail error
        error: ProtocolError,
    },
    /// peer send a frame with unknown opcode
    #[error("unsupported frame {0:?}")]
    UnsupportedFrame(OpCode),
}

impl From<std::io::Error> for WsError {
    fn from(e: std::io::Error) -> Self {
        WsError::IOError(Box::new(e))
    }
}

impl From<WsError> for std::io::Error {
    fn from(e: WsError) -> Self {
        std::io::Error::new(std::io::ErrorKind::InvalidData, e)
    }
}

/// errors during decode frame from bytes
#[derive(Debug, Error)]
pub enum ProtocolError {
    /// need more data to construct valid frame
    #[error("insufficient data len {0}")]
    InsufficientLen(usize),
    #[error("invalid leading bits {0:b}")]
    /// need more data to construct valid frame header
    InvalidLeadingBits(u8),
    /// invalid frame opcode
    #[error("invalid opcode {0}")]
    InvalidOpcode(u8),
    /// invalid bit of fin, rsv1, rsv2, rsv3
    #[error("invalid leading payload len {0}")]
    InvalidLeadingLen(u8),
    /// mismatch payload len in frame header and actual payload
    #[error("mismatch data len, expect {0}, got {1}")]
    MisMatchDataLen(usize, usize),
    /// missing first frame in fragmented frame
    #[error("missing init fragmented frame")]
    MissInitialFragmentedFrame,
    /// invalid data frame after first fragmented frame
    #[error("not continue frame after init fragmented frame")]
    NotContinueFrameAfterFragmented,
    /// control framed should not be fragmented
    #[error("fragmented control frame ")]
    FragmentedControlFrame,
    /// control frame is too big
    #[error("control frame is too big {0}")]
    ControlFrameTooBig(usize),
    /// invalid payload content(need to be valid utf8) of a close frame
    #[error("invalid close frame payload len, expect 0, >= 2")]
    InvalidCloseFramePayload,
    /// invalid utf8 payload of a text frame
    #[error("invalid utf-8 text")]
    InvalidUtf8,
    /// invalid close code
    #[error("invalid close code {0}")]
    InvalidCloseCode(u16),
    /// payload exceed payload len limit
    #[error("payload too large, max payload size {0}")]
    PayloadTooLarge(usize),
}
