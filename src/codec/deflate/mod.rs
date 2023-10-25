use core::slice;
use std::{
    ffi::{c_char, c_int, c_uint},
    mem::{self, transmute, MaybeUninit},
};

/// permessage-deflate id
pub const EXT_ID: &str = "permessage-deflate";
/// server_no_context_takeover param
pub const SERVER_NO_CONTEXT_TAKEOVER: &str = "server_no_context_takeover";
/// client_no_context_takeover param
pub const CLIENT_NO_CONTEXT_TAKEOVER: &str = "client_no_context_takeover";
/// server_max_window_bits param
pub const SERVER_MAX_WINDOW_BITS: &str = "server_max_window_bits";
/// client_max_window_bits param
pub const CLIENT_MAX_WINDOW_BITS: &str = "client_max_window_bits";

/// zlib version
pub const ZLIB_VERSION: &str = "1.2.13\0";

#[cfg(feature = "sync")]
mod blocking;
#[cfg(feature = "sync")]
pub use blocking::*;
use libz_sys::{Z_BUF_ERROR, Z_NO_FLUSH, Z_OK, Z_SYNC_FLUSH};

#[cfg(feature = "async")]
mod non_blocking;
#[cfg(feature = "async")]
pub use non_blocking::*;

use crate::{errors::WsError, frame::OpCode};

use super::{
    default_handshake_handler, FrameConfig, FrameReadState, FrameWriteState, ValidateUtf8Policy,
};

/// permessage-deflate window bit
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(i8)]
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

/// permessage-deflate req handler
pub fn deflate_handshake_handler(
    req: http::Request<()>,
) -> Result<(http::Request<()>, http::Response<String>), WsError> {
    // TODO return error response if parse error
    let (req, mut resp) = default_handshake_handler(req)?;
    let mut configs: Vec<PMDConfig> = vec![];
    for (k, v) in req.headers() {
        if k.as_str().to_lowercase() == "sec-websocket-extensions" {
            if let Ok(s) = v.to_str() {
                match PMDConfig::parse_str(s) {
                    Ok(mut conf) => {
                        configs.append(&mut conf);
                    }
                    Err(e) => return Err(WsError::HandShakeFailed(e)),
                }
            }
        }
    }
    if let Some(config) = configs.pop() {
        resp.headers_mut().insert(
            "sec-websocket-extensions",
            http::HeaderValue::from_str(&config.ext_string()).unwrap(),
        );
    }
    Ok((req, resp))
}

fn gen_low_level_config(conf: &FrameConfig) -> FrameConfig {
    FrameConfig {
        mask_send_frame: conf.mask_send_frame,
        check_rsv: false,
        auto_fragment_size: conf.auto_fragment_size,
        merge_frame: false,
        validate_utf8: ValidateUtf8Policy::Off,
        ..Default::default()
    }
}

/// helper struct to handler com stream
pub struct WriteStreamHandler {
    /// permessage deflate config
    pub config: PMDConfig,
    /// compressor
    pub com: ZLibCompressStream,
}

/// helper struct to handle de stream
pub struct ReadStreamHandler {
    /// permessage deflate config
    pub config: PMDConfig,
    /// decompressor
    pub de: ZLibDeCompressStream,
}

/// permessage-deflate
#[allow(missing_docs)]
#[derive(Debug, Clone)]
pub struct PMDConfig {
    pub server_no_context_takeover: bool,
    pub client_no_context_takeover: bool,
    pub server_max_window_bits: WindowBit,
    pub client_max_window_bits: WindowBit,
}

impl Default for PMDConfig {
    fn default() -> Self {
        Self {
            server_no_context_takeover: false,
            client_no_context_takeover: false,
            server_max_window_bits: WindowBit::Fifteen,
            client_max_window_bits: WindowBit::Fifteen,
        }
    }
}

impl PMDConfig {
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
    pub fn multi_ext_string(configs: &[PMDConfig]) -> String {
        configs
            .iter()
            .map(|conf| conf.ext_string())
            .collect::<Vec<String>>()
            .join(", ")
    }
}

///
pub struct ZLibDeCompressStream {
    stream: Box<libz_sys::z_stream>,
}

unsafe impl Send for ZLibDeCompressStream {}
unsafe impl Sync for ZLibDeCompressStream {}

impl Drop for ZLibDeCompressStream {
    fn drop(&mut self) {
        match unsafe { libz_sys::inflateEnd(self.stream.as_mut()) } {
            libz_sys::Z_STREAM_ERROR => {
                tracing::trace!("decompression stream encountered bad state.")
            }
            // Ignore discarded data error because we are raw
            libz_sys::Z_OK | libz_sys::Z_DATA_ERROR => {
                tracing::trace!("deallocated compression context.")
            }
            code => tracing::trace!("bad zlib status encountered: {}", code),
        }
    }
}

impl ZLibDeCompressStream {
    /// construct new compress stream
    pub fn new(window: WindowBit) -> Self {
        let mut stream: Box<MaybeUninit<libz_sys::z_stream>> = Box::new(MaybeUninit::zeroed());
        let result = unsafe {
            libz_sys::inflateInit2_(
                stream.as_mut_ptr(),
                -(window as i8) as c_int,
                ZLIB_VERSION.as_ptr() as *const c_char,
                mem::size_of::<libz_sys::z_stream>() as c_int,
            )
        };
        assert!(result == libz_sys::Z_OK, "Failed to initialize compresser.");
        Self {
            stream: unsafe { Box::from_raw(Box::into_raw(stream) as *mut libz_sys::z_stream) },
        }
    }

    /// construct with custom stream
    pub fn with(stream: Box<libz_sys::z_stream>) -> Self {
        Self { stream }
    }

    /// decompress data
    pub fn de_compress(&mut self, inputs: &[&[u8]], output: &mut Vec<u8>) -> Result<(), c_int> {
        let total_input: usize = inputs.iter().map(|i| i.len()).sum();
        if total_input > output.capacity() * 2 + 4 {
            output.resize(total_input * 2 + 4, 0);
        }
        let mut write_idx = 0;
        let before = self.stream.total_out;
        for i in inputs {
            let mut iter_read_idx = 0;
            loop {
                unsafe {
                    self.stream.next_in = i.as_ptr().add(iter_read_idx) as *mut _;
                }
                self.stream.avail_in = (i.len() - iter_read_idx) as c_uint;
                if output.capacity() - output.len() <= 0 {
                    output.resize(output.capacity() * 2, 0);
                }
                let out_slice = unsafe {
                    slice::from_raw_parts_mut(
                        output.as_mut_ptr().add(write_idx),
                        output.capacity() - write_idx,
                    )
                };
                self.stream.next_out = out_slice.as_mut_ptr();
                self.stream.avail_out = out_slice.len() as c_uint;

                match unsafe { libz_sys::inflate(*&mut self.stream.as_mut(), Z_NO_FLUSH) } {
                    Z_OK | Z_BUF_ERROR => {}
                    code => return Err(code),
                };
                iter_read_idx = i.len() - self.stream.avail_in as usize;
                write_idx = (self.stream.total_out - before) as usize;
                if self.stream.avail_in == 0 {
                    break;
                }
            }
        }
        unsafe {
            match libz_sys::inflate(*&mut self.stream.as_mut(), Z_SYNC_FLUSH) {
                Z_OK | Z_BUF_ERROR => {}
                code => return Err(code),
            }
            output.set_len((self.stream.total_out - before) as usize);
        };
        Ok(())
    }

    /// reset stream state
    pub fn reset(&mut self) -> Result<(), c_int> {
        let code = unsafe { libz_sys::inflateReset(self.stream.as_mut()) };
        match code {
            Z_OK => Ok(()),
            code => Err(code),
        }
    }
}

/// zlib compress stream
pub struct ZLibCompressStream {
    stream: Box<libz_sys::z_stream>,
}

unsafe impl Send for ZLibCompressStream {}
unsafe impl Sync for ZLibCompressStream {}

impl Drop for ZLibCompressStream {
    fn drop(&mut self) {
        match unsafe { libz_sys::deflateEnd(self.stream.as_mut()) } {
            libz_sys::Z_STREAM_ERROR => {
                tracing::trace!("compression stream encountered bad state.")
            }
            // Ignore discarded data error because we are raw
            libz_sys::Z_OK | libz_sys::Z_DATA_ERROR => {
                tracing::trace!("deallocated compression context.")
            }
            code => tracing::trace!("bad zlib status encountered: {}", code),
        }
    }
}

impl ZLibCompressStream {
    /// construct with window bit
    pub fn new(window: WindowBit) -> Self {
        let mut stream: Box<MaybeUninit<libz_sys::z_stream>> = Box::new(MaybeUninit::zeroed());
        let result = unsafe {
            libz_sys::deflateInit2_(
                stream.as_mut_ptr(),
                9,
                libz_sys::Z_DEFLATED,
                -(window as i8) as c_int,
                9,
                libz_sys::Z_DEFAULT_STRATEGY,
                ZLIB_VERSION.as_ptr() as *const c_char,
                mem::size_of::<libz_sys::z_stream>() as c_int,
            )
        };
        assert!(result == libz_sys::Z_OK, "Failed to initialize compresser.");
        Self {
            stream: unsafe { Box::from_raw(Box::into_raw(stream) as *mut libz_sys::z_stream) },
        }
    }

    /// construct with custom stream
    pub fn with(stream: Box<libz_sys::z_stream>) -> Self {
        Self { stream }
    }

    /// compress data
    pub fn compress(&mut self, inputs: &[&[u8]], output: &mut Vec<u8>) -> Result<(), c_int> {
        let total_input: usize = inputs.iter().map(|i| i.len()).sum();
        if total_input > output.capacity() * 2 + 4 {
            output.resize(total_input * 2 + 4, 0);
        }
        let mut write_idx = 0;
        let mut total_remain = total_input;
        let before = self.stream.total_out;
        for i in inputs {
            let mut iter_read_idx = 0;
            loop {
                unsafe {
                    self.stream.next_in = i.as_ptr().add(iter_read_idx) as *mut _;
                }
                self.stream.avail_in = (i.len() - iter_read_idx) as c_uint;
                if output.capacity() - output.len() <= 0 {
                    output.resize(output.len() + total_remain * 2, 0)
                }
                let out_slice = unsafe {
                    slice::from_raw_parts_mut(
                        output.as_mut_ptr().add(write_idx),
                        output.capacity() - write_idx,
                    )
                };
                self.stream.next_out = out_slice.as_mut_ptr();
                self.stream.avail_out = out_slice.len() as c_uint;

                match unsafe { libz_sys::deflate(*&mut self.stream.as_mut(), Z_NO_FLUSH) } {
                    libz_sys::Z_OK => {}
                    code => return Err(code),
                };
                iter_read_idx = i.len() - self.stream.avail_in as usize;
                write_idx = (self.stream.total_out - before) as usize;
                if self.stream.avail_in == 0 {
                    break;
                }
            }
            total_remain -= iter_read_idx;
        }
        unsafe {
            match libz_sys::deflate(*&mut self.stream.as_mut(), Z_SYNC_FLUSH) {
                Z_OK => {}
                code => return Err(code),
            }
            output.set_len((self.stream.total_out - before) as usize);
        };
        Ok(())
    }

    /// reset stream state
    pub fn reset(&mut self) -> Result<(), c_int> {
        let code = unsafe { libz_sys::deflateReset(self.stream.as_mut()) };
        match code {
            Z_OK => Ok(()),
            code => Err(code),
        }
    }
}

#[derive(Default)]
struct PMDParamCounter {
    server_no_context_takeover: bool,
    client_no_context_takeover: bool,
    server_max_window_bits: bool,
    client_max_window_bits: bool,
}

impl PMDConfig {
    /// case-insensitive parse one line header
    pub fn parse_str(source: &str) -> Result<Vec<Self>, String> {
        let lines = source.split("\r\n").count();
        if lines > 2 {
            return Err("should not contain multi line".to_string());
        }
        let mut configs = vec![];
        for part in source.split(',') {
            if part.trim_start().to_lowercase().starts_with(EXT_ID) {
                let mut conf = Self::default();
                let mut counter = PMDParamCounter::default();
                for param in part.split(';').skip(1) {
                    let lower = param.trim().to_lowercase();
                    if lower.starts_with(SERVER_NO_CONTEXT_TAKEOVER) {
                        if counter.server_no_context_takeover {
                            return Err(format!(
                                "got multiple {SERVER_NO_CONTEXT_TAKEOVER} params"
                            ));
                        }
                        if lower.len() != SERVER_NO_CONTEXT_TAKEOVER.len() {
                            return Err(format!(
                                "{SERVER_NO_CONTEXT_TAKEOVER} does not expect param"
                            ));
                        }
                        conf.server_no_context_takeover = true;
                        counter.server_no_context_takeover = true;
                        continue;
                    }

                    if lower.starts_with(CLIENT_NO_CONTEXT_TAKEOVER) {
                        if counter.client_no_context_takeover {
                            return Err(format!(
                                "got multiple {CLIENT_NO_CONTEXT_TAKEOVER} params"
                            ));
                        }
                        if lower.len() != CLIENT_NO_CONTEXT_TAKEOVER.len() {
                            return Err(format!(
                                "{CLIENT_NO_CONTEXT_TAKEOVER} does not expect param"
                            ));
                        }
                        conf.client_no_context_takeover = true;
                        counter.client_no_context_takeover = true;
                        continue;
                    }

                    if lower.starts_with(SERVER_MAX_WINDOW_BITS) {
                        if counter.server_max_window_bits {
                            return Err(format!("got multiple {SERVER_MAX_WINDOW_BITS} params"));
                        }

                        if lower != SERVER_MAX_WINDOW_BITS {
                            let remain = lower.trim_start_matches(SERVER_MAX_WINDOW_BITS);
                            if !remain.trim_start().starts_with('=') {
                                return Err("invalid param value".to_string());
                            }
                            let remain = remain.trim_start().trim_matches('=');
                            let size = match remain.parse::<u8>() {
                                Ok(size) => WindowBit::try_from(size)
                                    .map_err(|e| format!("invalid param value {e}"))?,
                                Err(e) => return Err(format!("invalid param value {e}")),
                            };
                            conf.server_max_window_bits = size;
                        }
                        counter.server_max_window_bits = true;
                        continue;
                    }

                    if lower.starts_with(CLIENT_MAX_WINDOW_BITS) {
                        if counter.server_max_window_bits {
                            return Err(format!("got multiple {CLIENT_MAX_WINDOW_BITS} params"));
                        }

                        if lower != CLIENT_MAX_WINDOW_BITS {
                            let remain = lower.trim_start_matches(CLIENT_MAX_WINDOW_BITS);
                            if !remain.trim_start().starts_with('=') {
                                return Err("invalid param value".to_string());
                            }
                            let remain = remain.trim_start().trim_matches('=');
                            let size = match remain.parse::<u8>() {
                                Ok(size) => WindowBit::try_from(size)
                                    .map_err(|e| format!("invalid param value {e}"))?,
                                Err(e) => return Err(format!("invalid param value {e}")),
                            };
                            conf.client_max_window_bits = size;
                        }
                        counter.client_max_window_bits = true;
                        continue;
                    }
                    return Err(format!("unknown param {param}"));
                }
                configs.push(conf);
            }
        }
        Ok(configs)
    }
}

/// deflate frame write state
pub struct DeflateWriteState {
    write_state: FrameWriteState,
    com: Option<WriteStreamHandler>,
    config: FrameConfig,
    header_buf: [u8; 14],
    is_server: bool,
}

impl DeflateWriteState {
    /// construct with config
    pub fn with_config(
        frame_config: FrameConfig,
        pmd_config: Option<PMDConfig>,
        is_server: bool,
    ) -> Self {
        let low_level_config = gen_low_level_config(&frame_config);
        let write_state = FrameWriteState::with_config(low_level_config);
        let com = if let Some(config) = pmd_config {
            let com_size = if is_server {
                config.client_max_window_bits
            } else {
                config.server_max_window_bits
            };
            let com = ZLibCompressStream::new(com_size);
            Some(WriteStreamHandler { config, com })
        } else {
            None
        };
        Self {
            write_state,
            com,
            config: frame_config,
            header_buf: [0; 14],
            is_server,
        }
    }
}

/// deflate frame read state
pub struct DeflateReadState {
    read_state: FrameReadState,
    de: Option<ReadStreamHandler>,
    config: FrameConfig,
    fragmented: bool,
    fragmented_data: Vec<u8>,
    control_buf: Vec<u8>,
    fragmented_type: OpCode,
    is_server: bool,
}

impl DeflateReadState {
    /// construct with config
    pub fn with_config(
        frame_config: FrameConfig,
        pmd_config: Option<PMDConfig>,
        is_server: bool,
    ) -> Self {
        let low_level_config = gen_low_level_config(&frame_config);
        let read_state = FrameReadState::with_config(low_level_config);
        let de = if let Some(config) = pmd_config {
            let de_size = if is_server {
                config.client_max_window_bits
            } else {
                config.server_max_window_bits
            };
            let de = ZLibDeCompressStream::new(de_size);
            Some(ReadStreamHandler { config, de })
        } else {
            None
        };
        Self {
            read_state,
            de,
            config: frame_config,
            fragmented: false,
            fragmented_data: vec![],
            control_buf: vec![],
            fragmented_type: OpCode::Binary,
            is_server,
        }
    }
}
