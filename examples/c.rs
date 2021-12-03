use std::io::Read;

use flate2::{
    read::{DeflateDecoder, DeflateEncoder},
    Compression,
};
use libflate::deflate::Decoder;

fn main() {
    let s = "{\n   \"AutobahnPy".as_bytes();
    let mut out = vec![];
    let mut en = DeflateEncoder::new(s, Compression::fast());
    dbg!(en.read_to_end(&mut out).unwrap());
    println!("{:b}", out[0]);
    out.extend([0x00, 0x00, 0xff, 0xff]);
    // let out = [
    //     170, 230, 82, 80, 80, 80, 114, 44, 45, 201, 79, 74, 204, 200, 11, 168, 4, 0, 0, 0, 255, 255,
    // ];
    let mut de = DeflateDecoder::new(&out[..]);
    let mut de_out = vec![];
    de.read_to_end(&mut de_out).unwrap();
    println!("{:?}", String::from_utf8(de_out));
    // let mut decoder = Decoder::new(&out[..]);
    // std::io::copy(&mut decoder, &mut std::io::stdout()).unwrap();
}
