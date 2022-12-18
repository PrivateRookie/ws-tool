use std::{
    ffi::{c_char, c_int, c_uint},
    mem::MaybeUninit,
};

const ZLIB_VERSION: &str = "1.2.8\0";

enum CompressError {}

fn apply_stream(
    input: &[u8],
    output: &mut Vec<u8>,
    f: impl Fn(&mut libz_sys::z_stream) -> Result<(), CompressError>,
) -> Result<(), CompressError> {
    todo!()
}

pub struct Compreesor {
    stream: Box<libz_sys::z_stream>,
}

impl Compreesor {
    #[allow(clippy::uninit_assumed_init)]
    #[allow(invalid_value)]
    pub fn init_compreesor(level: i32, max_window_bits: i32, mem_level: i32) -> Self {
        unsafe {
            let mut stream: Box<libz_sys::z_stream> = Box::new(MaybeUninit::uninit().assume_init());
            let ret = libz_sys::deflateInit2_(
                stream.as_mut(),
                level,
                libz_sys::Z_DEFLATED,
                max_window_bits,
                mem_level,
                libz_sys::Z_DEFAULT_STRATEGY,
                ZLIB_VERSION.as_ptr() as *const c_char,
                std::mem::size_of::<libz_sys::z_stream>() as c_int,
            );
            assert!(ret == libz_sys::Z_OK, "failed to init compressor");
            Self { stream }
        }
    }

    pub fn compress(&mut self, input: &[u8], output: &mut Vec<u8>) -> Result<(), String> {
        let stream = self.stream.as_mut();
        stream.next_in = input.as_ptr() as *mut _;
        stream.avail_in = input.len() as c_uint;

        loop {
            let output_size = output.len();
            if output_size == output.capacity() {
                output.reserve(input.len())
            }

            let out_slice = unsafe {
                std::slice::from_raw_parts_mut(
                    output.as_mut_ptr().offset(output_size as isize),
                    output.capacity() - output_size,
                )
            };

            stream.next_out = out_slice.as_mut_ptr();
            stream.avail_out = out_slice.len() as c_uint;

            let before = stream.total_out;
            unsafe {
                match libz_sys::deflate(stream, libz_sys::Z_SYNC_FLUSH) {
                    libz_sys::Z_OK | libz_sys::Z_BUF_ERROR => {
                        if stream.avail_in == 0 && stream.avail_out > 0 {
                            println!("OK")
                        } else {
                        }
                    }
                    code => {}
                };
                output.set_len((stream.total_out - before) as usize + output_size);
            }
        }

        todo!()
    }
}
