use crate::{codec::apply_mask_fast32, errors::ProtocolError};
use bytes::{Bytes, BytesMut};
use std::fmt::Debug;

/// Defines the interpretation of the "Payload data".  If an unknown
/// opcode is received, the receiving endpoint MUST _Fail the
/// WebSocket Connection_.  The following values are defined.
/// - x0 denotes a continuation frame
/// - x1 denotes a text frame
/// - x2 denotes a binary frame
/// - x3-7 are reserved for further non-control frames
/// - x8 denotes a connection close
/// - x9 denotes a ping
/// - xA denotes a pong
/// - xB-F are reserved for further control frames
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpCode {
    Continue,
    Text,
    Binary,
    ReservedNonControl,
    Close,
    Ping,
    Pong,
    ReservedControl,
}

impl Default for OpCode {
    fn default() -> Self {
        Self::Text
    }
}

impl OpCode {
    pub fn as_u8(&self) -> u8 {
        match self {
            OpCode::Continue => 0,
            OpCode::Text => 1,
            OpCode::Binary => 2,
            OpCode::ReservedNonControl => 3,
            OpCode::Close => 8,
            OpCode::Ping => 9,
            OpCode::Pong => 10,
            OpCode::ReservedControl => 11,
        }
    }
}

#[inline]
pub(crate) fn parse_opcode(val: u8) -> Result<OpCode, u8> {
    let val = val << 4;
    match val {
        0 => Ok(OpCode::Continue),
        16 => Ok(OpCode::Text),
        32 => Ok(OpCode::Binary),
        48 | 64 | 80 | 96 | 112 => Ok(OpCode::ReservedNonControl),
        128 => Ok(OpCode::Close),
        144 => Ok(OpCode::Ping),
        160 => Ok(OpCode::Pong),
        176 | 192 | 208 | 224 | 240 => Ok(OpCode::ReservedControl),
        _ => Err(val >> 4),
    }
}

#[inline]
pub(crate) fn get_bit(source: &[u8], byte_idx: usize, bit_idx: usize) -> bool {
    let b: u8 = source[byte_idx];
    1 & (b >> (7 - bit_idx)) != 0
}

#[inline]
pub(crate) fn set_bit(source: &mut [u8], byte_idx: usize, bit_idx: usize, val: bool) {
    let b = source[byte_idx];
    if val {
        source[byte_idx] = b | 1 << (7 - bit_idx)
    } else {
        source[byte_idx] = b & !(1 << (7 - bit_idx))
    }
}

pub(crate) fn parse_payload_len(source: &[u8]) -> Result<(usize, usize), ProtocolError> {
    let mut len = source[1];
    len = (len << 1) >> 1;
    match len {
        0..=125 => Ok((1, len as usize)),
        126 => {
            if source.len() < 4 {
                return Err(ProtocolError::InsufficientLen(source.len()));
            }
            let mut arr = [0u8; 2];
            arr[0] = source[2];
            arr[1] = source[3];
            Ok((1 + 2, u16::from_be_bytes(arr) as usize))
        }
        127 => {
            if source.len() < 10 {
                return Err(ProtocolError::InsufficientLen(source.len()));
            }
            let mut arr = [0u8; 8];
            arr[..8].copy_from_slice(&source[2..(8 + 2)]);
            Ok((1 + 8, usize::from_be_bytes(arr)))
        }
        _ => Err(ProtocolError::InvalidLeadingLen(len)),
    }
}

/// mutable frame header
#[derive(Debug)]
pub struct FrameHeaderMut<'a>(pub(crate) &'a mut BytesMut);

impl<'a> FrameHeaderMut<'a> {
    #[inline]
    fn get_bit(&self, byte_idx: usize, bit_idx: usize) -> bool {
        get_bit(&self.0, byte_idx, bit_idx)
    }

    #[inline]
    fn set_bit(&mut self, byte_idx: usize, bit_idx: usize, val: bool) {
        set_bit(&mut self.0, byte_idx, bit_idx, val)
    }

    #[inline]
    pub fn fin(&self) -> bool {
        self.get_bit(0, 0)
    }

    #[inline]
    pub fn set_fin(&mut self, val: bool) {
        self.set_bit(0, 0, val)
    }

    #[inline]
    pub fn rsv1(&self) -> bool {
        self.get_bit(0, 1)
    }

    #[inline]
    pub fn set_rsv1(&mut self, val: bool) {
        self.set_bit(0, 1, val)
    }

    #[inline]
    pub fn rsv2(&self) -> bool {
        self.get_bit(0, 2)
    }

    #[inline]
    pub fn set_rsv2(&mut self, val: bool) {
        self.set_bit(0, 2, val)
    }

    #[inline]
    pub fn rsv3(&self) -> bool {
        self.get_bit(0, 3)
    }

    #[inline]
    pub fn set_rsv3(&mut self, val: bool) {
        self.set_bit(0, 3, val)
    }

    /// return frame opcode
    #[inline]
    pub fn opcode(&self) -> OpCode {
        parse_opcode(self.0[0])
            .map_err(|code| format!("unexpected opcode {}", code))
            .unwrap()
    }

    /// set opcode
    #[inline]
    pub fn set_opcode(&mut self, code: OpCode) {
        let header = &mut self.0;
        let leading_bits = (header[0] >> 4) << 4;
        header[0] = leading_bits | code.as_u8()
    }

    #[inline]
    pub fn mask(&self) -> bool {
        self.get_bit(1, 0)
    }

    /// **NOTE** if change mask bit after setting payload
    /// you need to set payload again to adjust data frame
    #[inline]
    pub fn set_mask(&mut self, mask: bool) {
        self.set_bit(1, 0, mask);
    }

    #[inline]
    fn frame_sep(&self) -> (usize, u64) {
        let header = &self.0;
        let mut len = header[1];
        len = (len << 1) >> 1;
        match len {
            0..=125 => (1, len as u64),
            126 => {
                let mut arr = [0u8; 2];
                arr[0] = header[2];
                arr[1] = header[3];
                (1 + 2, u16::from_be_bytes(arr) as u64)
            }
            127 => {
                let mut arr = [0u8; 8];
                arr[..8].copy_from_slice(&header[2..(8 + 2)]);
                (1 + 8, u64::from_be_bytes(arr))
            }
            _ => unreachable!(),
        }
    }

    #[inline]
    pub fn payload_len(&self) -> u64 {
        self.frame_sep().1
    }

    #[inline]
    pub fn set_payload_len(&mut self, len: u64) -> usize {
        let header = &mut self.0;
        let mut leading_byte = header[1];
        match len {
            0..=125 => {
                leading_byte &= 128;
                header[1] = leading_byte | (len as u8);
                1
            }
            126..=65535 => {
                leading_byte &= 128;
                header[1] = leading_byte | 126;
                let len_arr = (len as u16).to_be_bytes();
                header[2] = len_arr[0];
                header[3] = len_arr[1];
                3
            }
            _ => {
                leading_byte &= 128;
                header[1] = leading_byte | 127;
                let len_arr = (len as u64).to_be_bytes();
                header[2..10].copy_from_slice(&len_arr[..8]);
                9
            }
        }
    }

    #[inline]
    pub fn masking_key(&self) -> Option<[u8; 4]> {
        if self.mask() {
            let len_occupied = self.frame_sep().0;
            let mut arr = [0u8; 4];
            arr[..4].copy_from_slice(&self.0[(1 + len_occupied)..(4 + 1 + len_occupied)]);
            Some(arr)
        } else {
            None
        }
    }

    #[inline]
    pub fn set_masking_key(&mut self) -> Option<[u8; 4]> {
        if self.mask() {
            let masking_key: [u8; 4] = rand::random();
            let (len_occupied, _) = self.frame_sep();
            self.0[(1 + len_occupied)..(5 + len_occupied)].copy_from_slice(&masking_key);
            Some(masking_key)
        } else {
            None
        }
    }

    /// return (header_len, frame_len)
    #[inline]
    pub fn payload_idx(&self) -> (usize, usize) {
        let mut start_idx = 1;
        let (len_occupied, len) = self.frame_sep();
        start_idx += len_occupied;
        if self.mask() {
            start_idx += 4;
        }
        (start_idx, start_idx + (len as usize))
    }
}

/// frame header
#[derive(Debug)]
pub struct FrameHeader<'a>(pub(crate) &'a BytesMut);

impl<'a> FrameHeader<'a> {
    #[inline]
    fn get_bit(&self, byte_idx: usize, bit_idx: usize) -> bool {
        get_bit(&self.0, byte_idx, bit_idx)
    }

    #[inline]
    pub fn fin(&self) -> bool {
        self.get_bit(0, 0)
    }

    #[inline]
    pub fn rsv1(&self) -> bool {
        self.get_bit(0, 1)
    }

    #[inline]
    pub fn rsv2(&self) -> bool {
        self.get_bit(0, 2)
    }

    #[inline]
    pub fn rsv3(&self) -> bool {
        self.get_bit(0, 3)
    }

    /// return frame opcode
    #[inline]
    pub fn opcode(&self) -> OpCode {
        parse_opcode(self.0[0])
            .map_err(|code| format!("unexpected opcode {}", code))
            .unwrap()
    }

    #[inline]
    pub fn mask(&self) -> bool {
        self.get_bit(1, 0)
    }

    #[inline]
    fn frame_sep(&self) -> (usize, u64) {
        let header = &self.0;
        let mut len = header[1];
        len = (len << 1) >> 1;
        match len {
            0..=125 => (1, len as u64),
            126 => {
                let mut arr = [0u8; 2];
                arr[0] = header[2];
                arr[1] = header[3];
                (1 + 2, u16::from_be_bytes(arr) as u64)
            }
            127 => {
                let mut arr = [0u8; 8];
                arr[..8].copy_from_slice(&header[2..(8 + 2)]);
                (1 + 8, u64::from_be_bytes(arr))
            }
            _ => unreachable!(),
        }
    }

    #[inline]
    pub fn payload_len(&self) -> u64 {
        self.frame_sep().1
    }

    #[inline]
    pub fn masking_key(&self) -> Option<[u8; 4]> {
        if self.mask() {
            let len_occupied = self.frame_sep().0;
            let mut arr = [0u8; 4];
            arr[..4].copy_from_slice(&self.0[(1 + len_occupied)..(4 + 1 + len_occupied)]);
            Some(arr)
        } else {
            None
        }
    }

    #[inline]
    pub fn payload_idx(&self) -> (usize, usize) {
        let mut start_idx = 1;
        let (len_occupied, len) = self.frame_sep();
        start_idx += len_occupied;
        if self.mask() {
            start_idx += 4;
        }
        (start_idx, start_idx + (len as usize))
    }
}

#[derive(Debug, Clone)]
pub struct OwnedFrame(pub(crate) BytesMut);

impl OwnedFrame {
    pub fn new<'a, P: Into<Payload<'a>>>(
        fin: bool,
        rsv1: bool,
        rsv2: bool,
        rsv3: bool,
        opcode: OpCode,
        mask: bool,
        payload: P,
    ) -> Self {
        let payload: Payload = payload.into();
        let payload_len = payload.len();
        let mut header_len = 1;
        if mask {
            header_len += 4;
        }
        if payload_len <= 125 {
            header_len += 1;
        } else if payload_len <= 65535 {
            header_len += 3;
        } else {
            header_len += 9;
        }
        let mut buf = BytesMut::new();
        buf.resize(header_len + payload_len, 0);
        let mut frame = OwnedFrame(buf);
        let mut header = frame.header_mut();
        header.set_fin(fin);
        header.set_rsv1(rsv1);
        header.set_rsv2(rsv2);
        header.set_rsv3(rsv3);
        header.set_opcode(opcode);
        header.set_payload_len(payload_len as u64);
        let start = header_len;
        let end = header_len + payload_len;
        if mask {
            header.set_mask(true);
            let key = header.set_masking_key().unwrap();
            payload.copy_with_key(&mut frame.0[start..end], key)
        } else {
            header.set_mask(false);
            payload.copy_to(&mut frame.0[start..end])
        }
        frame
    }

    pub fn empty() -> Self {
        Self::new(true, false, false, false, OpCode::Binary, false, vec![])
    }

    pub fn replace(&mut self, other: OwnedFrame) -> OwnedFrame {
        let frame_len = self.header().payload_idx().1;
        let ret = OwnedFrame(self.0.split_to(frame_len));
        self.0.extend_from_slice(&other.0);
        ret
    }

    pub fn header(&self) -> FrameHeader {
        FrameHeader(&self.0)
    }

    pub fn header_mut(&mut self) -> FrameHeaderMut {
        FrameHeaderMut(&mut self.0)
    }

    pub fn payload(&self) -> &[u8] {
        let (start, end) = self.header().payload_idx();
        &self.0[start..end]
    }

    pub fn payload_mut(&mut self) -> &mut [u8] {
        let (start, end) = self.header().payload_idx();
        &mut self.0[start..end]
    }
}

/// borrowed data contains payload
#[derive(Debug, Clone)]
pub struct BorrowedFrame<'a> {
    pub(crate) header: BytesMut,
    pub payload: Payload<'a>,
}

impl<'a> BorrowedFrame<'a> {
    pub fn header(&self) -> FrameHeader {
        FrameHeader(&self.header)
    }

    pub fn header_mut(&mut self) -> FrameHeaderMut {
        FrameHeaderMut(&mut self.header)
    }

    pub fn payload(&self) -> Payload {
        self.payload.clone()
    }
}

#[derive(Clone, Debug)]
pub struct Payload<'a>(pub(crate) Vec<&'a [u8]>);

impl<'a> From<&'a [u8]> for Payload<'a> {
    fn from(data: &'a [u8]) -> Self {
        Self(vec![data])
    }
}
impl<'a> From<Vec<&'a [u8]>> for Payload<'a> {
    fn from(data: Vec<&'a [u8]>) -> Self {
        Self(data)
    }
}

impl<'a> From<&'a [&'a [u8]]> for Payload<'a> {
    fn from(data: &'a [&'a [u8]]) -> Self {
        Self(data.to_vec())
    }
}

impl<'a> From<&'a BytesMut> for Payload<'a> {
    fn from(src: &'a BytesMut) -> Self {
        Self(vec![src])
    }
}

impl<'a> From<&'a Bytes> for Payload<'a> {
    fn from(src: &'a Bytes) -> Self {
        Self(vec![src])
    }
}

impl<'a> Payload<'a> {
    pub fn len(&self) -> usize {
        self.0.iter().map(|i| i.len()).sum()
    }

    pub fn iter(&self) -> PayloadIterator<'_> {
        PayloadIterator {
            payload: self,
            array_idx: 0,
            offset: 0,
            idx: 0,
        }
    }

    pub fn split_with(&self, size: usize) -> Vec<Payload> {
        let mut ret = vec![];
        let last = self.0.iter().fold(vec![], |mut acc: Vec<&'a [u8]>, &arr| {
            let pre_len: usize = acc.iter().map(|x| x.len()).sum();
            let arr_len = arr.len();
            if pre_len + arr_len <= size {
                acc.push(&arr);
                acc
            } else {
                let stop = arr_len + pre_len - size;
                acc.push(&arr[..stop]);
                ret.push(acc.into());
                vec![&arr[stop..]]
            }
        });
        ret.push(last.into());
        ret
    }

    pub fn copy_to(&self, dest: &mut [u8]) {
        let mut offset = 0;
        self.0.iter().for_each(|arr| {
            dest[offset..(offset + arr.len())].copy_from_slice(arr);
            offset += arr.len()
        })
    }

    pub fn copy_with_key(&self, dest: &mut [u8], key: [u8; 4]) {
        let mut offset = 0;
        self.0.iter().for_each(|arr| {
            let buf: Vec<u8> = arr
                .iter()
                .enumerate()
                .map(|(i, v)| v ^ key[(i + offset) % 4])
                .collect();
            dest[offset..(offset + arr.len())].copy_from_slice(&buf);
            offset += arr.len()
        })
    }
}

pub struct PayloadIterator<'a> {
    payload: &'a Payload<'a>,
    array_idx: usize,
    offset: usize,
    idx: usize,
}

impl<'a> Iterator for PayloadIterator<'a> {
    type Item = &'a u8;

    fn next(&mut self) -> Option<Self::Item> {
        self.payload.0.get(self.array_idx).and_then(|arr| {
            // advance
            self.idx += 1;
            if self.idx >= arr.len() + self.offset {
                self.array_idx += 1;
                self.offset += arr.len();
            }
            arr.get(self.idx - self.offset)
        })
    }
}

#[derive(Debug)]
pub struct PayloadMut<'a>(pub(crate) Vec<&'a mut [u8]>);

impl<'a> From<&'a mut [u8]> for PayloadMut<'a> {
    fn from(data: &'a mut [u8]) -> Self {
        Self(vec![data])
    }
}
impl<'a> From<Vec<&'a mut [u8]>> for PayloadMut<'a> {
    fn from(data: Vec<&'a mut [u8]>) -> Self {
        Self(data)
    }
}

impl<'a> From<&'a mut BytesMut> for PayloadMut<'a> {
    fn from(src: &'a mut BytesMut) -> Self {
        Self(vec![src])
    }
}

impl<'a> PayloadMut<'a> {
    pub fn len(&self) -> usize {
        self.0.iter().map(|i| i.len()).sum()
    }

    // pub fn iter(&self) -> PayloadIterator<'_> {
    //     PayloadIterator {
    //         payload: self,
    //         array_idx: 0,
    //         offset: 0,
    //         idx: 0,
    //     }
    // }

    pub fn split_with(self, size: usize) -> Vec<PayloadMut<'a>> {
        let mut ret = vec![];
        let last = self
            .0
            .into_iter()
            .fold(vec![], |mut acc: Vec<&'a mut [u8]>, arr| {
                let pre_len: usize = acc.iter().map(|x| x.len()).sum();
                let arr_len = arr.len();
                if pre_len + arr_len <= size {
                    acc.push(arr);
                    acc
                } else {
                    let stop = arr_len + pre_len - size;
                    let (pre, last) = arr.split_at_mut(stop);
                    acc.push(pre);
                    ret.push(acc.into());
                    vec![last]
                }
            });
        ret.push(last.into());
        ret
    }

    pub fn copy_to(&self, dest: &mut [u8]) {
        let mut offset = 0;
        self.0.iter().for_each(|arr| {
            dest[offset..(offset + arr.len())].copy_from_slice(arr);
            offset += arr.len()
        })
    }

    pub fn apply_mask(&mut self, mask: [u8; 4]) {
        let mut offset = 0;
        for part in self.0.iter_mut() {
            let mut part_mask = mask.clone();
            part_mask[0] = mask[(0 + offset) % 4];
            part_mask[1] = mask[(1 + offset) % 4];
            part_mask[2] = mask[(2 + offset) % 4];
            part_mask[3] = mask[(3 + offset) % 4];
            apply_mask_fast32(part, mask);
            offset += part.len()
        }
    }

    pub fn copy_with_key(&self, dest: &mut [u8], key: [u8; 4]) {
        let mut offset = 0;
        self.0.iter().for_each(|arr| {
            let buf: Vec<u8> = arr
                .iter()
                .enumerate()
                .map(|(i, v)| v ^ key[(i + offset) % 4])
                .collect();
            dest[offset..(offset + arr.len())].copy_from_slice(&buf);
            offset += arr.len()
        })
    }
}
