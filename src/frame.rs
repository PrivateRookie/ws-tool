use crate::errors::ProtocolError;
use bytes::{Bytes, BytesMut};
use std::fmt::Debug;

const DEFAULT_FRAME: [u8; 14] = [0b10000001, 0b10000000, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];

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

/// websocket data frame
#[derive(Clone)]
pub struct Frame(pub(crate) BytesMut);

impl Frame {
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
    pub fn opcode(&self) -> OpCode {
        parse_opcode(self.0[0])
            .map_err(|code| format!("unexpected opcode {}", code))
            .unwrap()
    }

    fn set_opcode(&mut self, code: OpCode) {
        let leading_bits = (self.0[0] >> 4) << 4;
        self.0[0] = leading_bits | code.as_u8()
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
    fn payload_len_with_occ(&self) -> (usize, u64) {
        let mut len = self.0[1];
        len = (len << 1) >> 1;
        match len {
            0..=125 => (1, len as u64),
            126 => {
                let mut arr = [0u8; 2];
                arr[0] = self.0[2];
                arr[1] = self.0[3];
                (1 + 2, u16::from_be_bytes(arr) as u64)
            }
            127 => {
                let mut arr = [0u8; 8];
                arr[..8].copy_from_slice(&self.0[2..(8 + 2)]);
                (1 + 8, u64::from_be_bytes(arr))
            }
            _ => unreachable!(),
        }
    }

    pub fn payload_len(&self) -> u64 {
        self.payload_len_with_occ().1
    }

    fn set_payload_len(&mut self, len: u64) -> usize {
        let mut leading_byte = self.0[1];
        match len {
            0..=125 => {
                leading_byte &= 128;
                self.0[1] = leading_byte | (len as u8);
                1
            }
            126..=65535 => {
                leading_byte &= 128;
                self.0[1] = leading_byte | 126;
                let len_arr = (len as u16).to_be_bytes();
                self.0[2] = len_arr[0];
                self.0[3] = len_arr[1];
                3
            }
            _ => {
                leading_byte &= 128;
                self.0[1] = leading_byte | 127;
                let len_arr = (len as u64).to_be_bytes();
                self.0[2..10].copy_from_slice(&len_arr[..8]);
                9
            }
        }
    }

    pub fn masking_key(&self) -> Option<[u8; 4]> {
        if self.mask() {
            let len_occupied = self.payload_len_with_occ().0;
            let mut arr = [0u8; 4];
            arr[..4].copy_from_slice(&self.0[(1 + len_occupied)..(4 + 1 + len_occupied)]);
            Some(arr)
        } else {
            None
        }
    }

    pub fn set_masking_key(&mut self) -> Option<[u8; 4]> {
        if self.mask() {
            let masking_key: [u8; 4] = rand::random();
            let (len_occupied, _) = self.payload_len_with_occ();
            self.0[(1 + len_occupied)..(5 + len_occupied)].copy_from_slice(&masking_key);
            Some(masking_key)
        } else {
            None
        }
    }

    /// return unmask(if masked) payload data
    pub fn payload_data_unmask(&self) -> Bytes {
        match self.masking_key() {
            Some(masking_key) => {
                let slice = self
                    .payload_data()
                    .iter()
                    .enumerate()
                    .map(|(idx, num)| num ^ masking_key[idx % 4])
                    .collect::<Vec<u8>>();
                Bytes::copy_from_slice(&slice)
            }
            None => Bytes::copy_from_slice(self.payload_data()),
        }
    }

    pub fn payload_data(&self) -> &[u8] {
        let mut start_idx = 1;
        let (len_occupied, len) = self.payload_len_with_occ();
        start_idx += len_occupied;
        if self.mask() {
            start_idx += 4;
        }
        &self.0[start_idx..start_idx + (len as usize)]
    }
}

impl Default for Frame {
    fn default() -> Self {
        // TODO may adjust to better size
        let mut raw = BytesMut::with_capacity(200);
        raw.extend_from_slice(&DEFAULT_FRAME);
        Self(raw)
    }
}

/// helper construct methods
impl Frame {
    // TODO should init with const array to avoid computing?
    pub fn new_with_opcode(opcode: OpCode) -> Self {
        let mut frame = Frame::default();
        frame.set_opcode(opcode);
        frame
    }

    pub fn new_with_payload(opcode: OpCode, payload: &[u8]) -> Self {
        let mut frame = Frame::default();
        frame.set_opcode(opcode);
        frame.set_payload(payload);
        frame
    }

    /// set frame payload
    ///
    /// **NOTE!** avoid calling this method multi times, since it need to calculate mask
    /// every time
    pub fn set_payload(&mut self, payload: &[u8]) {
        let len = payload.len();
        let offset = self.set_payload_len(len as u64);
        let mask = self.mask();
        let mut start_idx = 1 + offset;
        let mut end_idx = 1 + offset + len;
        if mask {
            let masking_key = self.set_masking_key().unwrap();
            start_idx += 4;
            end_idx += 4;
            self.0.resize(end_idx, 0x0);
            let data = payload
                .iter()
                .enumerate()
                .map(|(idx, v)| v ^ masking_key[idx % 4])
                .collect::<Vec<u8>>();
            self.0[start_idx..end_idx].copy_from_slice(&data);
        } else {
            self.0.resize(end_idx, 0x0);
            self.0[start_idx..end_idx].copy_from_slice(payload)
        }
    }

    pub fn as_bytes(&self) -> &[u8] {
        let (occ, len) = self.payload_len_with_occ();
        let mut end = 1 + occ + len as usize;
        if self.mask() {
            end += 4
        }
        &self.0[..end]
    }
}

impl Debug for Frame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(
            f,
            "<Frame {:b} {:?} {}>",
            self.0[0] >> 4,
            self.opcode(),
            self.payload_len()
        )?;
        Ok(())
    }
}
