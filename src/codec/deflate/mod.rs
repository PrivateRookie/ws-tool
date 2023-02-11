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

#[cfg(feature = "async")]
mod non_blocking;
#[cfg(feature = "async")]
pub use non_blocking::*;

use crate::errors::WsError;

use super::default_handshake_handler;

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
            EXT_ID,
            http::HeaderValue::from_str(&config.ext_string()).unwrap(),
        );
    }
    Ok((req, resp))
}

/// helper struct to handle com/de stream
pub struct StreamHandler {
    /// permessage deflate config
    pub config: PMDConfig,
    /// compressor
    pub com: Compressor,
    /// decompressor
    pub de: DeCompressor,
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

trait Context {
    fn stream(&mut self) -> &mut libz_sys::z_stream;

    fn stream_apply<F>(&mut self, input: &[u8], output: &mut Vec<u8>, each: F) -> Result<(), String>
    where
        F: Fn(&mut libz_sys::z_stream) -> Option<Result<(), String>>,
    {
        debug_assert!(output.is_empty(), "Output vector is not empty.");

        let stream = self.stream();

        stream.next_in = input.as_ptr() as *mut _;
        stream.avail_in = input.len() as c_uint;

        let mut output_size;

        loop {
            output_size = output.len();

            if output_size == output.capacity() {
                output.reserve(input.len())
            }

            let out_slice = unsafe {
                slice::from_raw_parts_mut(
                    output.as_mut_ptr().add(output_size),
                    output.capacity() - output_size,
                )
            };

            stream.next_out = out_slice.as_mut_ptr();
            stream.avail_out = out_slice.len() as c_uint;

            let before = stream.total_out;
            let cont = each(stream);

            unsafe {
                output.set_len((stream.total_out - before) as usize + output_size);
            }

            if let Some(result) = cont {
                return result;
            }
        }
    }
}

/// permessage-deflate compress
///
/// copy from ws-rs
pub struct Compressor {
    stream: Box<libz_sys::z_stream>,
}

impl Context for Compressor {
    fn stream(&mut self) -> &mut libz_sys::z_stream {
        &mut self.stream
    }
}

impl Drop for Compressor {
    fn drop(&mut self) {
        match unsafe { libz_sys::deflateEnd(self.stream.as_mut()) } {
            libz_sys::Z_STREAM_ERROR => {
                tracing::warn!("Compression stream encountered bad state.")
            }
            // Ignore discarded data error because we are raw
            libz_sys::Z_OK | libz_sys::Z_DATA_ERROR => {
                tracing::trace!("Deallocated compression context.")
            }
            code => tracing::error!("Bad zlib status encountered: {}", code),
        }
    }
}

impl Compressor {
    /// construct a compressor
    pub fn new(size: WindowBit) -> Compressor {
        unsafe {
            let mut stream: Box<MaybeUninit<libz_sys::z_stream>> = Box::new(MaybeUninit::zeroed());
            let result = libz_sys::deflateInit2_(
                stream.as_mut_ptr(),
                9,
                libz_sys::Z_DEFLATED,
                -(size as i8) as c_int,
                9,
                libz_sys::Z_DEFAULT_STRATEGY,
                ZLIB_VERSION.as_ptr() as *const c_char,
                mem::size_of::<libz_sys::z_stream>() as c_int,
            );
            assert!(result == libz_sys::Z_OK, "Failed to initialize compresser.");
            // SAFETY: This is exactly what the (currently unstable) Box::assume_init does.
            let stream = Box::from_raw(Box::into_raw(stream) as *mut libz_sys::z_stream);
            Self { stream }
        }
    }

    /// compress data
    pub fn compress(&mut self, input: &[u8], output: &mut Vec<u8>) -> Result<(), String> {
        // tracing::debug!("compress source {:?}", input);
        self.stream_apply(input, output, |stream| unsafe {
            match libz_sys::deflate(stream, libz_sys::Z_SYNC_FLUSH) {
                libz_sys::Z_OK | libz_sys::Z_BUF_ERROR => {
                    if stream.avail_in == 0 && stream.avail_out > 0 {
                        Some(Ok(()))
                    } else {
                        None
                    }
                }
                code => Some(Err(code.to_string())),
            }
        })?;
        // tracing::debug!("compress output {:?}", output);
        Ok(())
    }

    /// reset compressor state
    pub fn reset(&mut self) -> Result<(), String> {
        match unsafe { libz_sys::deflateReset(self.stream.as_mut()) } {
            libz_sys::Z_OK => Ok(()),
            code => Err(format!("Failed to reset compression context: {}", code)),
        }
    }
}

/// permessage-deflate decompress
///
/// copy from ws-rs
pub struct DeCompressor {
    stream: Box<libz_sys::z_stream>,
}

impl Context for DeCompressor {
    fn stream(&mut self) -> &mut libz_sys::z_stream {
        &mut self.stream
    }
}

impl Drop for DeCompressor {
    fn drop(&mut self) {
        match unsafe { libz_sys::deflateEnd(self.stream.as_mut()) } {
            libz_sys::Z_STREAM_ERROR => {
                tracing::warn!("Decompression stream encountered bad state.")
            }
            // Ignore discarded data error because we are raw
            libz_sys::Z_OK | libz_sys::Z_DATA_ERROR => {
                tracing::trace!("Deallocated decompression context.")
            }
            code => tracing::error!("Bad zlib status encountered: {}", code),
        }
    }
}

impl DeCompressor {
    /// construct a decompressor
    pub fn new(size: WindowBit) -> Self {
        unsafe {
            let mut stream: Box<MaybeUninit<libz_sys::z_stream>> = Box::new(MaybeUninit::zeroed());
            let result = libz_sys::inflateInit2_(
                stream.as_mut_ptr(),
                -(size as i8) as c_int,
                ZLIB_VERSION.as_ptr() as *const c_char,
                mem::size_of::<libz_sys::z_stream>() as c_int,
            );
            assert!(
                result == libz_sys::Z_OK,
                "Failed to initialize decompresser."
            );
            let stream = Box::from_raw(Box::into_raw(stream) as *mut libz_sys::z_stream);
            DeCompressor { stream }
        }
    }

    /// decompress data
    pub fn decompress(&mut self, input: &[u8], output: &mut Vec<u8>) -> Result<(), String> {
        // tracing::debug!("decompress source {:?}", input);
        self.stream_apply(input, output, |stream| unsafe {
            match libz_sys::inflate(stream, libz_sys::Z_SYNC_FLUSH) {
                libz_sys::Z_OK | libz_sys::Z_BUF_ERROR => {
                    if stream.avail_in == 0 && stream.avail_out > 0 {
                        Some(Ok(()))
                    } else {
                        None
                    }
                }
                code => Some(Err(code.to_string())),
            }
        })?;
        // tracing::debug!("decompress output {:?}", output);
        Ok(())
    }

    /// reset decompressor state
    pub fn reset(&mut self) -> Result<(), String> {
        match unsafe { libz_sys::inflateReset(self.stream.as_mut()) } {
            libz_sys::Z_OK => Ok(()),
            code => Err(format!("Failed to reset compression context: {}", code)),
        }
    }
}
