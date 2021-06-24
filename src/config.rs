use bytes::Bytes;

/// websocket client config
#[derive(Debug, Clone)]
pub struct WebsocketConfig {
    /// if true, log every byte in `trace` level
    pub log_octets: bool,
    /// if true, log every frame `debug` level
    pub log_frame: bool,
    /// check text utf-8 validation, default true
    pub validate_utf8: bool,
    /// apply frame masking, default true
    pub mask: bool,
    /// max frame payload size, default 0, unlimited
    pub max_frame_payload_size: u64,
    /// default 0, not fragmented
    pub auto_fragment_size: u64,
    /// open handshake timeout in milliseconds, default 0, not setting timeout
    pub open_handshake_timeout: u64,
    /// close handshake timeout in milliseconds, default 0, not setting timeout
    pub close_handshake_timeout: u64,
    /// if true, set NODELAY(Nagle) socket option
    pub tcp_no_delay: bool,
    /// auto ping interval, milliseconds between auto ping
    pub auto_ping_interval: u64,
    /// auto ping timeout in milliseconds
    pub auto_ping_timeout: u64,
    pub auto_ping_payload: Bytes,

    /// websocket version, default 13
    pub version: u8,
}

impl Default for WebsocketConfig {
    fn default() -> Self {
        Self {
            log_octets: false,
            log_frame: false,
            validate_utf8: true,
            mask: true,
            max_frame_payload_size: 0,
            auto_fragment_size: 0,
            open_handshake_timeout: 0,
            close_handshake_timeout: 0,
            tcp_no_delay: false,
            auto_ping_interval: 0,
            auto_ping_timeout: 0,
            auto_ping_payload: Bytes::new(),
            version: 13,
        }
    }
}
