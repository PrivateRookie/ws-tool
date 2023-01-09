use std::mem::transmute;

pub const EXT_ID: &str = "permessage-deflate";
pub const SERVER_NO_CONTEXT_TAKEOVER: &str = "server_no_context_takeover";
pub const CLIENT_NO_CONTEXT_TAKEOVER: &str = "client_no_context_takeover";
pub const SERVER_MAX_WINDOW_BITS: &str = "server_max_window_bits";
pub const CLIENT_MAX_WINDOW_BITS: &str = "client_max_window_bits";

/// permessage-deflate window bit
#[derive(Debug, Clone, Copy)]
#[repr(u8)]
#[allow(missing_docs)]
pub enum WindowBit {
    Eight = 8,
    Nine = 9,
    Ten = 10,
    Eleven = 11,
    Twelve = 12,
    Thirteen = 13,
    Fourteen = 14,
    Fifteen = 15,
}

impl TryFrom<u8> for WindowBit {
    type Error = u8;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        if matches!(value, 8 | 9 | 10 | 11 | 12 | 13 | 14 | 15) {
            let value = unsafe { transmute(value) };
            Ok(value)
        } else {
            Err(value)
        }
    }
}

/// permessage-deflate
#[allow(missing_docs)]
#[derive(Debug, Clone)]
pub struct PMGConfig {
    pub server_no_context_takeover: bool,
    pub client_no_context_takeover: bool,
    pub server_max_window_bits: WindowBit,
    pub client_max_window_bits: WindowBit,
}

impl Default for PMGConfig {
    fn default() -> Self {
        Self {
            server_no_context_takeover: true,
            client_no_context_takeover: true,
            server_max_window_bits: WindowBit::Fifteen,
            client_max_window_bits: WindowBit::Fifteen,
        }
    }
}

impl PMGConfig {
    /// get extension string
    pub fn ext_string(&self) -> String {
        let mut s = format!("{EXT_ID};");
        if self.client_no_context_takeover {
            s.push_str(CLIENT_NO_CONTEXT_TAKEOVER);
            s.push(';');
            s.push(' ');
        }
        if self.server_no_context_takeover {
            s.push_str(SERVER_NO_CONTEXT_TAKEOVER);
            s.push(';');
            s.push(' ');
        }
        s.push_str(&format!(
            "{CLIENT_MAX_WINDOW_BITS}={};",
            self.client_max_window_bits as u8
        ));
        s.push_str(&format!(
            "{SERVER_MAX_WINDOW_BITS}={}",
            self.server_max_window_bits as u8
        ));
        s
    }

    /// helper function to build multi permessage deflate config header
    pub fn multi_ext_string(configs: &[PMGConfig]) -> String {
        configs
            .iter()
            .map(|conf| conf.ext_string())
            .collect::<Vec<String>>()
            .join(", ")
    }
}
