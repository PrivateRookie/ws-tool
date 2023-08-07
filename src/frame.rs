use crate::codec::{apply_mask, apply_mask_fast32};
use bytes::{BufMut, Bytes, BytesMut};
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
#[derive(Debug, Clone, PartialEq, Eq, Copy)]
#[repr(u8)]
pub enum OpCode {
    /// - x0 denotes a continuation frame
    Continue = 0,
    /// - x1 denotes a text frame
    Text = 1,
    /// - x2 denotes a binary frame
    Binary = 2,
    /// - x3-7 are reserved for further non-control frames
    RNC3 = 3,
    /// - x3-7 are reserved for further non-control frames
    RNC4 = 4,
    /// - x3-7 are reserved for further non-control frames
    RNC5 = 5,
    /// - x3-7 are reserved for further non-control frames
    RNC6 = 6,
    /// - x3-7 are reserved for further non-control frames
    RNC7 = 7,
    /// - x8 denotes a connection close
    Close = 8,
    /// - x9 denotes a ping
    Ping = 9,
    /// - xA denotes a pong
    Pong = 10,
    /// - xB-F are reserved for further control frames
    RC11 = 11,
    /// - xB-F are reserved for further control frames
    RC12 = 12,
    /// - xB-F are reserved for further control frames
    RC13 = 13,
    /// - xB-F are reserved for further control frames
    RC14 = 14,
    /// - xB-F are reserved for further control frames
    RC15 = 15,
}

impl Default for OpCode {
    fn default() -> Self {
        Self::Text
    }
}

impl OpCode {
    /// get corresponding u8 value
    pub fn as_u8(&self) -> u8 {
        *self as u8
    }

    /// check is close type frame
    pub fn is_close(&self) -> bool {
        matches!(self, Self::Close)
    }

    /// check is text/binary ?
    pub fn is_data(&self) -> bool {
        matches!(self, Self::Text | Self::Binary | Self::Continue)
    }

    /// check is reserved
    pub fn is_reserved(&self) -> bool {
        matches!(self.as_u8(), 3..=5 | 11..=15)
    }
}

pub(crate) fn parse_opcode(val: u8) -> OpCode {
    unsafe { std::mem::transmute(val & 0b00001111) }
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

macro_rules! impl_get {
    () => {
        #[inline]
        fn get_bit(&self, byte_idx: usize, bit_idx: usize) -> bool {
            get_bit(&self.0, byte_idx, bit_idx)
        }

        /// get fin bit value
        #[inline]
        pub fn fin(&self) -> bool {
            self.get_bit(0, 0)
        }

        /// get rsv1 bit value
        #[inline]
        pub fn rsv1(&self) -> bool {
            self.get_bit(0, 1)
        }

        /// get rsv2 bit value
        #[inline]
        pub fn rsv2(&self) -> bool {
            self.get_bit(0, 2)
        }

        /// get rsv3 bit value
        #[inline]
        pub fn rsv3(&self) -> bool {
            self.get_bit(0, 3)
        }

        /// return frame opcode
        #[inline]
        pub fn opcode(&self) -> OpCode {
            parse_opcode(self.0[0])
        }

        /// get mask bit value
        #[inline]
        pub fn masked(&self) -> bool {
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

        /// return **payload** len
        #[inline]
        pub fn payload_len(&self) -> u64 {
            self.frame_sep().1
        }

        /// get frame mask key
        #[inline]
        pub fn masking_key(&self) -> Option<[u8; 4]> {
            if self.masked() {
                let len_occupied = self.frame_sep().0;
                let mut arr = [0u8; 4];
                arr[..4].copy_from_slice(&self.0[(1 + len_occupied)..(4 + 1 + len_occupied)]);
                Some(arr)
            } else {
                None
            }
        }

        /// return (headerlen, the whole frame len)
        #[inline]
        pub fn payload_idx(&self) -> (usize, usize) {
            let mut start_idx = 1;
            let (len_occupied, len) = self.frame_sep();
            start_idx += len_occupied;
            if self.masked() {
                start_idx += 4;
            }
            (start_idx, start_idx + (len as usize))
        }
    };
}

/// get expected header len
#[allow(clippy::too_many_arguments)]
pub fn header_len(mask: bool, payload_len: u64) -> usize {
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
    header_len
}

/// write header without allocation
#[allow(clippy::too_many_arguments)]
pub fn ctor_header<M: Into<Option<[u8; 4]>>>(
    buf: &mut [u8],
    fin: bool,
    rsv1: bool,
    rsv2: bool,
    rsv3: bool,
    mask_key: M,
    opcode: OpCode,
    payload_len: u64,
) -> &[u8] {
    // assert!(buf.len() >= 14);
    let mask = mask_key.into();
    let mut header_len = 1;
    if mask.is_some() {
        header_len += 4;
    }
    if payload_len <= 125 {
        buf[1] = payload_len as u8;
        header_len += 1;
    } else if payload_len <= 65535 {
        buf[1] = 126;
        buf[2..4].copy_from_slice(&(payload_len as u16).to_be_bytes());
        header_len += 3;
    } else {
        buf[1] = 127;
        buf[2..10].copy_from_slice(&payload_len.to_be_bytes());
        header_len += 9;
    }
    buf[0] = 0;
    buf[0] |= opcode as u8;
    set_bit(buf, 0, 0, fin);
    set_bit(buf, 0, 1, rsv1);
    set_bit(buf, 0, 2, rsv2);
    set_bit(buf, 0, 3, rsv3);
    if let Some(key) = mask {
        set_bit(buf, 1, 0, true);
        buf[(header_len - 4)..header_len].copy_from_slice(&key);
    } else {
        set_bit(buf, 1, 0, false);
    }
    &buf[..header_len]
}

#[test]
fn test_header() {
    fn rand_mask() -> Option<[u8; 4]> {
        fastrand::bool().then(|| fastrand::u32(0..u32::MAX).to_be_bytes())
    }

    fn rand_code() -> OpCode {
        unsafe { std::mem::transmute(fastrand::u8(0..16)) }
    }

    let mut buf = [0u8; 14];
    for _ in 0..1000 {
        let fin = fastrand::bool();
        let rsv1 = fastrand::bool();
        let rsv2 = fastrand::bool();
        let rsv3 = fastrand::bool();
        let mask_key = rand_mask();
        let opcode = rand_code();
        let payload_len = fastrand::u64(0..u64::MAX);

        let slice = ctor_header(
            &mut buf,
            fin,
            rsv1,
            rsv2,
            rsv3,
            mask_key,
            opcode,
            payload_len,
        );
        let header = Header::new(fin, rsv1, rsv2, rsv3, mask_key, opcode, payload_len);
        assert_eq!(slice, &header.0.to_vec());
    }
}

/// frame header
#[derive(Debug, Clone, Copy)]
pub struct HeaderView<'a>(pub(crate) &'a BytesMut);

impl<'a> HeaderView<'a> {
    impl_get! {}
}

/// owned header buf
#[derive(Debug, Clone)]
pub struct Header(pub(crate) BytesMut);

impl Header {
    impl_get! {}
    #[inline]
    fn set_bit(&mut self, byte_idx: usize, bit_idx: usize, val: bool) {
        set_bit(&mut self.0, byte_idx, bit_idx, val)
    }

    /// set fin bit
    #[inline]
    pub fn set_fin(&mut self, val: bool) {
        self.set_bit(0, 0, val)
    }

    /// set rsv1 bit
    #[inline]
    pub fn set_rsv1(&mut self, val: bool) {
        self.set_bit(0, 1, val)
    }

    /// set rsv2 bit
    #[inline]
    pub fn set_rsv2(&mut self, val: bool) {
        self.set_bit(0, 2, val)
    }

    /// set rsv3 bit
    #[inline]
    pub fn set_rsv3(&mut self, val: bool) {
        self.set_bit(0, 3, val)
    }

    /// set opcode
    #[inline]
    pub fn set_opcode(&mut self, code: OpCode) {
        let header = &mut self.0;
        let leading_bits = (header[0] >> 4) << 4;
        header[0] = leading_bits | code.as_u8()
    }

    /// **NOTE** if change mask bit after setting payload
    /// you need to set payload again to adjust data frame
    #[inline]
    pub fn set_mask(&mut self, mask: bool) {
        self.set_bit(1, 0, mask);
    }

    /// set header payload lens
    /// TODO do not overlay mask key
    #[inline]
    pub fn set_payload_len(&mut self, len: u64) -> usize {
        let mask = self.masking_key();
        let mask_len = mask.as_ref().map(|_| 4).unwrap_or_default();
        let header = &mut self.0;
        let mut leading_byte = header[1];
        match len {
            0..=125 => {
                leading_byte &= 128;
                header[1] = leading_byte | (len as u8);
                let idx = 1 + 1;
                header.resize(idx + mask_len, 0);
                if let Some(mask) = mask {
                    header[idx..].copy_from_slice(&mask);
                }
                1
            }
            126..=65535 => {
                leading_byte &= 128;
                header[1] = leading_byte | 126;
                let len_arr = (len as u16).to_be_bytes();
                let idx = 1 + 3;
                header.resize(idx + mask_len, 0);
                header[2] = len_arr[0];
                header[3] = len_arr[1];
                if let Some(mask) = mask {
                    header[idx..].copy_from_slice(&mask);
                }
                3
            }
            _ => {
                leading_byte &= 128;
                header[1] = leading_byte | 127;
                let len_arr = len.to_be_bytes();
                let idx = 1 + 9;
                header.resize(idx + mask_len, 0);
                header[2..10].copy_from_slice(&len_arr[..8]);
                if let Some(mask) = mask {
                    header[idx..].copy_from_slice(&mask);
                }
                9
            }
        }
    }

    pub(crate) fn raw(data: BytesMut) -> Self {
        Self(data)
    }

    /// construct new header
    pub fn new<M: Into<Option<[u8; 4]>>>(
        fin: bool,
        rsv1: bool,
        rsv2: bool,
        rsv3: bool,
        mask_key: M,
        opcode: OpCode,
        payload_len: u64,
    ) -> Self {
        let mask = mask_key.into();
        let mut buf = BytesMut::new();
        let mut header_len = 1;
        if mask.is_some() {
            header_len += 4;
        }
        if payload_len <= 125 {
            header_len += 1;
        } else if payload_len <= 65535 {
            header_len += 3;
        } else {
            header_len += 9;
        }
        buf.resize(header_len, 0);
        let mut header = Self(buf);
        header.set_fin(fin);
        header.set_rsv1(rsv1);
        header.set_rsv2(rsv2);
        header.set_rsv3(rsv3);
        header.set_opcode(opcode);
        header.set_payload_len(payload_len);
        if let Some(mask) = mask {
            header.set_mask(true);
            header.0.extend_from_slice(&mask);
        }

        header
    }
}

/// unified frame type
#[derive(Debug, Clone)]
pub enum Frame<'a> {
    /// owned frame
    Owned(OwnedFrame),
    /// borrowed payload frame
    BorrowedFrame(BorrowedFrame<'a>),
}

/// owned frame
#[derive(Debug, Clone)]
pub struct OwnedFrame {
    header: Header,
    payload: BytesMut,
}

impl OwnedFrame {
    /// construct new owned frame
    pub fn new(code: OpCode, mask: impl Into<Option<[u8; 4]>>, data: &[u8]) -> Self {
        let header = Header::new(true, false, false, false, mask, code, data.len() as u64);
        let mut payload = BytesMut::with_capacity(data.len());
        payload.extend_from_slice(data);
        if let Some(mask) = header.masking_key() {
            apply_mask(&mut payload, mask);
        }
        Self { header, payload }
    }

    /// use constructed header and payload
    ///
    /// **NOTE**: this will not check header and payload
    pub fn with_raw(header: Header, payload: BytesMut) -> Self {
        Self { header, payload }
    }

    /// helper function to construct a text frame
    pub fn text_frame(mask: impl Into<Option<[u8; 4]>>, data: &str) -> Self {
        Self::new(OpCode::Text, mask, data.as_bytes())
    }

    /// helper function to construct a binary frame
    pub fn binary_frame(mask: impl Into<Option<[u8; 4]>>, data: &[u8]) -> Self {
        Self::new(OpCode::Binary, mask, data)
    }

    /// helper function to construct a ping frame
    pub fn ping_frame(mask: impl Into<Option<[u8; 4]>>, data: &[u8]) -> Self {
        assert!(data.len() <= 125);
        Self::new(OpCode::Ping, mask, data)
    }

    /// helper function to construct a pong frame
    pub fn pong_frame(mask: impl Into<Option<[u8; 4]>>, data: &[u8]) -> Self {
        assert!(data.len() <= 125);
        Self::new(OpCode::Pong, mask, data)
    }

    /// helper function to construct a close frame
    pub fn close_frame(
        mask: impl Into<Option<[u8; 4]>>,
        code: impl Into<Option<u16>>,
        data: &[u8],
    ) -> Self {
        assert!(data.len() <= 123);
        let code = code.into();
        assert!(code.is_some() || data.is_empty());
        let mut payload = BytesMut::with_capacity(2 + data.len());
        if let Some(code) = code {
            payload.put_u16(code);
            payload.extend_from_slice(data);
        }
        Self::new(OpCode::Close, mask, &payload)
    }

    /// unmask frame if masked
    pub fn unmask(&mut self) -> Option<[u8; 4]> {
        if let Some(mask) = self.header.masking_key() {
            apply_mask_fast32(&mut self.payload, mask);
            self.header.set_mask(false);
            self.header.0.resize(self.header.0.len() - 4, 0);
            Some(mask)
        } else {
            None
        }
    }

    /// mask frame with provide mask key
    ///
    /// this will override old mask
    pub fn mask(&mut self, mask: [u8; 4]) {
        self.unmask();
        self.header.set_mask(true);
        self.header.0.extend_from_slice(&mask);
        apply_mask_fast32(&mut self.payload, mask);
    }

    /// extend frame payload
    ///
    /// **NOTE** this function will unmask first, and then extend payload, mask with old
    /// mask key finally
    pub fn extend_from_slice(&mut self, data: &[u8]) {
        if let Some(mask) = self.unmask() {
            self.payload.extend_from_slice(data);
            self.header.set_payload_len(self.payload.len() as u64);
            self.mask(mask);
        } else {
            self.payload.extend_from_slice(data);
            self.header.set_payload_len(self.payload.len() as u64);
        }
    }

    /// get frame header
    pub fn header(&self) -> &Header {
        &self.header
    }

    /// get mutable frame header
    pub fn header_mut(&mut self) -> &mut Header {
        &mut self.header
    }

    /// get payload
    pub fn payload(&self) -> &BytesMut {
        &self.payload
    }

    /// consume frame return header and payload
    pub fn parts(mut self) -> (Header, BytesMut) {
        self.unmask();
        (self.header, self.payload)
    }
}

/// borrowed frame
#[derive(Debug, Clone)]
pub struct BorrowedFrame<'a> {
    header: HeaderView<'a>,
    payload: Bytes,
}

impl<'a> BorrowedFrame<'a> {
    /// get frame header
    pub fn header(&self) -> HeaderView<'a> {
        self.header
    }

    /// get frame payload
    pub fn payload(&self) -> &Bytes {
        &self.payload
    }
}
