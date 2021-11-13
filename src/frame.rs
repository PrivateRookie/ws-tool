use crate::errors::ProtocolError;
use bytes::{Buf, Bytes, BytesMut};
use std::io::{Error as IOError, ErrorKind::InvalidData};
use std::{fmt::Debug, ops::Deref};
use tokio_util::codec::{Decoder, Encoder};

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
fn parse_opcode(val: u8) -> Result<OpCode, u8> {
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
fn get_bit(source: &[u8], byte_idx: usize, bit_idx: usize) -> bool {
    let b: u8 = source[byte_idx];
    1 & (b >> (7 - bit_idx)) != 0
}

#[inline]
fn set_bit(source: &mut [u8], byte_idx: usize, bit_idx: usize, val: bool) {
    let b = source[byte_idx];
    let op = if val {
        1 << (7 - bit_idx)
    } else {
        u8::MAX - (1 << (7 - bit_idx))
    };
    source[byte_idx] = b | op
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
pub struct Frame(BytesMut);

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

/// default websocket frame encoder
#[derive(Debug, Clone)]
pub struct FrameEncoder {}

impl Default for FrameEncoder {
    fn default() -> Self {
        Self {}
    }
}

impl Encoder<Frame> for FrameEncoder {
    type Error = IOError;

    fn encode(&mut self, item: Frame, dst: &mut BytesMut) -> Result<(), Self::Error> {
        dst.extend_from_slice(&item.0);
        Ok(())
    }
}

/// default websocket frame decoder
#[derive(Debug, Clone)]
pub struct FrameDecoder {
    pub check_rsv: bool,
    pub handshake_remaining: BytesMut,
    pub fragmented: bool,
    pub fragmented_data: BytesMut,
    pub fragmented_type: OpCode,
}

impl Default for FrameDecoder {
    fn default() -> Self {
        Self {
            check_rsv: true,
            handshake_remaining: Default::default(),
            fragmented: false,
            fragmented_data: Default::default(),
            fragmented_type: OpCode::Text,
        }
    }
}

impl FrameDecoder {
    fn decode_single(&mut self, src: &mut BytesMut) -> Result<Option<Frame>, IOError> {
        if !self.handshake_remaining.is_empty() {
            let mut tmp = BytesMut::with_capacity(src.len() + self.handshake_remaining.len());
            tmp.extend_from_slice(&self.handshake_remaining);
            tmp.extend_from_slice(src);
            src.clear();
            self.handshake_remaining.clear();
            src.extend_from_slice(&tmp);
        }

        if src.len() < 2 {
            return Ok(None);
        }
        // TODO check nonzero value according to extension negotiation
        let leading_bits = src[0] >> 4;
        if self.check_rsv && !(leading_bits == 0b00001000 || leading_bits == 0b00000000) {
            return Err(IOError::new(
                InvalidData,
                ProtocolError::InvalidLeadingBits(leading_bits),
            ));
        }
        parse_opcode(src[0])
            .map_err(|e| IOError::new(InvalidData, ProtocolError::InvalidOpcode(e)))?;
        let (payload_len, len_occ_bytes) =
            parse_payload_len(src.deref()).map_err(|e| IOError::new(InvalidData, e))?;
        let mut expected_len = 1 + len_occ_bytes + payload_len;
        let mask = get_bit(&src, 1, 0);
        if mask {
            expected_len += 4;
        }
        if expected_len > src.len() {
            src.reserve(expected_len - src.len() + 1);
            Ok(None)
        } else {
            let mut data = BytesMut::with_capacity(expected_len);
            data.extend_from_slice(&src[..expected_len]);
            src.advance(expected_len);
            Ok(Some(Frame(data)))
        }
    }
}

impl Decoder for FrameDecoder {
    type Item = Frame;
    type Error = IOError;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        let maybe_frame = self.decode_single(src)?;
        if let Some(frame) = maybe_frame {
            let opcode = frame.opcode();
            match opcode {
                OpCode::Continue => {
                    if !self.fragmented {
                        let reason = ProtocolError::MissInitialFragmentedFrame;
                        // self.close(1002, reason.to_string()).await?;
                        return Err(IOError::new(InvalidData, reason));
                    }
                    self.fragmented_data
                        .extend_from_slice(&frame.payload_data_unmask());
                    if frame.fin() {
                        if String::from_utf8(self.fragmented_data.to_vec()).is_err() {
                            let reason = ProtocolError::InvalidUtf8;
                            // self.close(1007, reason.to_string()).await?;
                            return Err(IOError::new(InvalidData, reason));
                        }
                        let completed_frame = Frame::new_with_payload(
                            self.fragmented_type.clone(),
                            &self.fragmented_data,
                        );
                        return Ok(Some(completed_frame));
                    } else {
                        Ok(None)
                    }
                }
                OpCode::Text | OpCode::Binary => {
                    if self.fragmented {
                        let reason = ProtocolError::NotContinueFrameAfterFragmented;
                        // self.close(1002, reason.to_string()).await?;
                        return Err(IOError::new(InvalidData, reason));
                    }
                    if !frame.fin() {
                        self.fragmented = true;
                        self.fragmented_type = opcode.clone();
                        let payload = frame.payload_data_unmask();
                        self.fragmented_data.extend_from_slice(&payload);
                        Ok(None)
                    } else {
                        if opcode == OpCode::Text
                            && String::from_utf8(frame.payload_data_unmask().to_vec()).is_err()
                        {
                            let reason = ProtocolError::InvalidUtf8;
                            // self.close(1007, reason.to_string()).await?;
                            return Err(IOError::new(InvalidData, reason));
                        }
                        return Ok(Some(frame));
                    }
                }
                OpCode::Close | OpCode::Ping | OpCode::Pong => {
                    if !frame.fin() {
                        let reason = ProtocolError::FragmentedControlFrame;
                        // self.close(1002, reason.to_string()).await?;
                        return Err(IOError::new(InvalidData, reason));
                    }
                    let payload_len = frame.payload_len();
                    if payload_len > 125 {
                        let reason = ProtocolError::ControlFrameTooBig(payload_len as usize);
                        // self.close(1002, reason.to_string()).await?;
                        return Err(IOError::new(InvalidData, reason));
                    }
                    if opcode == OpCode::Close {
                        if payload_len == 1 {
                            let reason = ProtocolError::InvalidCloseFramePayload;
                            // self.close(1002, reason.to_string()).await?;
                            return Err(IOError::new(InvalidData, reason));
                        }
                        if payload_len >= 2 {
                            let payload = frame.payload_data();

                            // check close code
                            let mut code_byte = [0u8; 2];
                            code_byte.copy_from_slice(&payload[..2]);
                            let code = u16::from_be_bytes(code_byte);
                            if code < 1000
                                || (1004..=1006).contains(&code)
                                || (1015..=2999).contains(&code)
                                || code >= 5000
                            {
                                let reason = ProtocolError::InvalidCloseCode(code);
                                // self.close(1002, reason.to_string()).await?;
                                return Err(IOError::new(InvalidData, reason));
                            }

                            // utf-8 validation
                            if String::from_utf8(payload[2..].to_vec()).is_err() {
                                let reason = ProtocolError::InvalidUtf8;
                                // self.close(1007, reason.to_string()).await?;
                                return Err(IOError::new(InvalidData, reason));
                            }
                        }
                    }
                    if opcode == OpCode::Close || !self.fragmented {
                        return Ok(Some(frame));
                    } else {
                        log::debug!("{:?} frame between self.fragmented data", opcode);
                        // let echo =
                        //     Frame::new_with_payload(OpCode::Pong, &frame.payload_data_unmask());
                        // self.write_frame(echo).await?;
                        return Ok(Some(frame));
                    }
                }
                OpCode::ReservedNonControl | OpCode::ReservedControl => {
                    // self.close(1002, format!("can not handle {:?} frame", opcode))
                    //     .await?;
                    return Err(IOError::new(
                        InvalidData,
                        format!("unsupported frame {:?}", opcode),
                    ));
                }
            }
        } else {
            Ok(None)
        }
    }
}

#[derive(Debug, Clone)]
pub struct FrameCodec {
    pub encoder: FrameEncoder,
    pub decoder: FrameDecoder,
}

impl Default for FrameCodec {
    fn default() -> Self {
        Self {
            encoder: Default::default(),
            decoder: Default::default(),
        }
    }
}

impl Encoder<Frame> for FrameCodec {
    type Error = IOError;

    fn encode(&mut self, item: Frame, dst: &mut BytesMut) -> Result<(), Self::Error> {
        self.encoder.encode(item, dst)
    }
}

impl Decoder for FrameCodec {
    type Item = Frame;

    type Error = IOError;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        self.decoder.decode(src)
    }
}
