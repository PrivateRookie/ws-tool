use crate::codec::apply_mask_fast32;
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
    /// - x0 denotes a continuation frame
    Continue,
    /// - x1 denotes a text frame
    Text,
    /// - x2 denotes a binary frame
    Binary,
    /// - x3-7 are reserved for further non-control frames
    ReservedNonControl,
    /// - x8 denotes a connection close
    Close,
    /// - x9 denotes a ping
    Ping,
    /// - xA denotes a pong
    Pong,
    /// - xB-F are reserved for further control frames
    ReservedControl,
}

impl Default for OpCode {
    fn default() -> Self {
        Self::Text
    }
}

impl OpCode {
    /// get corresponding u8 value
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

    /// check is close type frame
    pub fn is_close(&self) -> bool {
        matches!(self, Self::Close)
    }

    /// check is text/binary ?
    pub fn is_data(&self) -> bool {
        matches!(self, Self::Text | Self::Binary | Self::Continue)
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
                .map_err(|code| format!("unexpected opcode {}", code))
                .unwrap()
        }

        /// get mask bit value
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

        /// return **payload** len
        #[inline]
        pub fn payload_len(&self) -> u64 {
            self.frame_sep().1
        }

        /// get frame mask key
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

        /// return (headerlen, the whole frame len)
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
    };
}

macro_rules! impl_set {
    () => {
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

        /// key header mask key
        #[inline]
        pub fn set_masking_key(&mut self, key: [u8; 4]) {
            let (len_occupied, _) = self.frame_sep();
            self.0[(1 + len_occupied)..(5 + len_occupied)].copy_from_slice(&key);
        }

        /// auto generate mask key and apply it
        #[inline]
        pub fn auto_mask(&mut self) -> Option<[u8; 4]> {
            if self.mask() {
                let masking_key: [u8; 4] = rand::random();
                let (len_occupied, _) = self.frame_sep();
                self.0[(1 + len_occupied)..(5 + len_occupied)].copy_from_slice(&masking_key);
                Some(masking_key)
            } else {
                None
            }
        }
    };
}

macro_rules! impl_config {
    () => {
        /// config mut buf as a valid header
        ///
        /// **NOTE** this operation will override buf content, and try to extend
        /// if there is no enough len
        #[allow(clippy::too_many_arguments)]
        pub fn config(
            fin: bool,
            rsv1: bool,
            rsv2: bool,
            rsv3: bool,
            opcode: OpCode,
            mask: bool,
            payload_len: u64,
            buf: &mut BytesMut,
        ) {
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
            if buf.len() < header_len {
                buf.resize(header_len, 0);
            }
            let mut header = HeaderViewMut(buf);
            header.set_fin(fin);
            header.set_rsv1(rsv1);
            header.set_rsv2(rsv2);
            header.set_rsv3(rsv3);
            header.set_opcode(opcode);
            if mask {
                header.set_mask(true);
                header.auto_mask();
            }
            header.set_payload_len(payload_len);
        }
    };
}

/// frame header
#[derive(Debug)]
pub struct HeaderView<'a>(pub(crate) &'a BytesMut);

impl<'a> HeaderView<'a> {
    impl_get! {}
}

/// mutable frame header
#[derive(Debug)]
pub struct HeaderViewMut<'a>(pub(crate) &'a mut BytesMut);

impl<'a> HeaderViewMut<'a> {
    impl_get! {}
    impl_set! {}

    /// return bytes contains expected frame header
    pub fn empty_payload(
        fin: bool,
        rsv1: bool,
        rsv2: bool,
        rsv3: bool,
        opcode: OpCode,
        mask: bool,
        payload_len: usize,
    ) -> BytesMut {
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
        buf.resize(header_len, 0);
        let mut header = HeaderViewMut(&mut buf);
        header.set_fin(fin);
        header.set_rsv1(rsv1);
        header.set_rsv2(rsv2);
        header.set_rsv3(rsv3);
        header.set_opcode(opcode);
        if mask {
            header.set_mask(true);
            header.auto_mask();
        }
        header.set_payload_len(payload_len as u64);
        buf
    }

    impl_config! {}
}

/// owned header buf
#[derive(Debug, Clone)]
pub struct Header(pub(crate) BytesMut);

impl Header {
    impl_get! {}
    impl_set! {}
    impl_config! {}

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
        let mut buf = BytesMut::new();
        let mask_key = mask_key.into();
        Header::config(
            fin,
            rsv1,
            rsv2,
            rsv3,
            opcode,
            mask_key.is_some(),
            payload_len,
            &mut buf,
        );
        let mut header = Header(buf);
        if let Some(key) = mask_key {
            header.set_masking_key(key);
        }
        header
    }

    /// return read only header view
    pub fn view(&self) -> HeaderView {
        HeaderView(&self.0)
    }

    /// return mutable header view
    pub fn view_mut(&mut self) -> HeaderViewMut {
        HeaderViewMut(&mut self.0)
    }
}

/// unified frame type
#[allow(missing_docs)]
pub enum Frame<'a> {
    Read(ReadFrame),
    Owned(OwnedFrame),
    BorrowedFrame {
        header: HeaderView<'a>,
        payload: Payload<'a>,
    },
}

/// owned read framed, usually from read frame from stream
#[derive(Debug, Clone)]
pub struct ReadFrame(pub(crate) BytesMut);

impl ReadFrame {
    /// construct read frame
    pub fn new<'a, P: Into<PayloadMut<'a>>>(
        fin: bool,
        rsv1: bool,
        rsv2: bool,
        rsv3: bool,
        opcode: OpCode,
        mask: bool,
        payload: P,
    ) -> Self {
        let mut payload: PayloadMut = payload.into();
        let payload_len = payload.len();
        let mut buf =
            HeaderViewMut::empty_payload(fin, rsv1, rsv2, rsv3, opcode, mask, payload_len);
        let start = buf.len();
        let end = start + payload_len;
        buf.resize(end, 0);
        let mut frame = ReadFrame(buf);
        let header = frame.header_mut();
        if mask {
            let key = header.masking_key().unwrap();
            payload.apply_mask(key);
        }
        payload.copy_to(&mut frame.0[start..end]);
        frame
    }

    /// construct empty payload frame
    pub fn empty(opcdoe: OpCode) -> Self {
        Self::new(true, false, false, false, opcdoe, true, vec![])
    }

    /// replace old frame with other  buf and return it
    pub fn replace(&mut self, other: ReadFrame) -> ReadFrame {
        let frame_len = self.header().payload_idx().1;
        let ret = ReadFrame(self.0.split_to(frame_len));
        self.0.extend_from_slice(&other.0);
        ret
    }

    /// readonly header view of frame
    pub fn header(&self) -> HeaderView {
        HeaderView(&self.0)
    }

    /// mutable header view of frame
    pub fn header_mut(&mut self) -> HeaderViewMut {
        HeaderViewMut(&mut self.0)
    }

    /// immutable frame payload
    pub fn payload(&self) -> &[u8] {
        let (start, end) = self.header().payload_idx();
        &self.0[start..end]
    }

    /// consume frame and return (header,  payload)
    pub fn split(mut self) -> (Header, BytesMut) {
        let stop = self.header().payload_idx().0;
        let header = Header(self.0.split_to(stop));
        (header, self.0)
    }

    /// mutable reference of payload
    pub fn payload_mut(&mut self) -> &mut [u8] {
        let (start, end) = self.header().payload_idx();
        &mut self.0[start..end]
    }
}

/// borrowed data contains payload
#[derive(Debug, Clone)]
pub struct BorrowedFrame<'a> {
    pub(crate) header: Header,
    /// borrowed payload
    pub payload: Payload<'a>,
}

impl<'a> BorrowedFrame<'a> {
    /// immutable header view
    pub fn header(&self) -> HeaderView {
        self.header.view()
    }

    /// mutable header view
    pub fn header_mut(&mut self) -> HeaderViewMut {
        self.header.view_mut()
    }

    /// payload
    pub fn payload(&self) -> Payload {
        self.payload.clone()
    }
}

/// header for user p
#[derive(Debug, Clone)]
pub struct OwnedFrame {
    header: Header,
    payload: BytesMut,
}

impl OwnedFrame {
    /// construct new owned frame
    #[allow(clippy::too_many_arguments)]
    pub fn new<M: Into<Option<[u8; 4]>>>(
        fin: bool,
        rsv1: bool,
        rsv2: bool,
        rsv3: bool,
        mask_key: M,
        opcode: OpCode,
        mut payload: BytesMut,
        mask_payload: bool,
    ) -> Self {
        let header = Header::new(
            fin,
            rsv1,
            rsv2,
            rsv3,
            mask_key,
            opcode,
            payload.len() as u64,
        );
        if mask_payload {
            if let Some(key) = header.masking_key() {
                apply_mask_fast32(&mut payload, key);
            }
        };
        Self { header, payload }
    }

    /// immutable header view
    pub fn header(&self) -> HeaderView {
        self.header.view()
    }

    /// mutable header view
    pub fn header_mut(&mut self) -> HeaderViewMut {
        self.header.view_mut()
    }

    /// immutable payload
    pub fn payload(&self) -> &[u8] {
        &self.payload
    }

    /// mutable payload
    pub fn payload_mut(&mut self) -> &mut BytesMut {
        &mut self.payload
    }

    /// replace old payload
    pub fn replace(&mut self, new: BytesMut, mask_payload: bool) -> BytesMut {
        self.header_mut().set_payload_len(new.len() as u64);
        let old = self.payload.split_to(self.payload.len());
        self.payload = new;
        if mask_payload {
            if let Some(key) = self.header.masking_key() {
                apply_mask_fast32(&mut self.payload, key);
            }
        }
        old
    }
}

/// a collection of immutable payload(vector of u8 slice)
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
    /// check payload is empty
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// return total bytes count of payload
    pub fn len(&self) -> usize {
        self.0.iter().map(|i| i.len()).sum()
    }

    /// return payload iterator
    pub fn iter(&self) -> PayloadIterator<'_> {
        PayloadIterator {
            payload: self,
            array_idx: 0,
            offset: 0,
            idx: 0,
        }
    }

    /// split payload into smaller size
    pub fn split_with(&self, size: usize) -> Vec<Payload> {
        let mut ret = vec![];
        let last = self.0.iter().fold(vec![], |mut acc: Vec<&'a [u8]>, &arr| {
            let pre_len: usize = acc.iter().map(|x| x.len()).sum();
            let arr_len = arr.len();
            if pre_len + arr_len <= size {
                acc.push(arr);
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

    /// copy all bytes in payload to dest
    pub fn copy_to(&self, dest: &mut [u8]) {
        let mut offset = 0;
        self.0.iter().for_each(|arr| {
            dest[offset..(offset + arr.len())].copy_from_slice(arr);
            offset += arr.len()
        })
    }

    /// like copy_to, but apply mask at the same time
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

/// Iterator of payload
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

/// a collection of mutable payload(vector of mutable u8 slice)
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
    /// check payload is empty
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// return total bytes count of payloads
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

    /// split payload into smaller size
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

    /// copy all bytes in payload to dest
    pub fn copy_to(&self, dest: &mut [u8]) {
        let mut offset = 0;
        self.0.iter().for_each(|arr| {
            dest[offset..(offset + arr.len())].copy_from_slice(arr);
            offset += arr.len()
        })
    }

    /// apply mask by the given mask key
    pub fn apply_mask(&mut self, mask: [u8; 4]) {
        let mut offset = 0;
        for part in self.0.iter_mut() {
            let mut part_mask = mask;
            part_mask[0] = mask[offset % 4];
            part_mask[1] = mask[(1 + offset) % 4];
            part_mask[2] = mask[(2 + offset) % 4];
            part_mask[3] = mask[(3 + offset) % 4];
            apply_mask_fast32(part, mask);
            offset += part.len()
        }
    }
}
