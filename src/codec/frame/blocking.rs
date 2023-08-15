use bytes::BytesMut;

use super::{FrameConfig, FrameNewReadState, FrameReadState, FrameWriteState};
use crate::{
    codec::{apply_mask, Split},
    errors::{ProtocolError, WsError},
    frame::{ctor_header, get_bit, header_len, parse_opcode, OpCode, OwnedFrame},
    protocol::standard_handshake_resp_check,
};
use std::io::{IoSlice, Read, Write};

type IOResult<T> = std::io::Result<T>;

impl FrameNewReadState {
    pub fn receive<S: Read, W: Write>(
        &mut self,
        stream: &mut S,
        buf: &mut W,
    ) -> Result<(usize, OpCode), WsError> {
        let (_, len, code) = self.read_one_frame(stream, buf)?;
        // TODO do merge
        Ok((len, code))
    }

    fn read_one_frame<S: Read, W: Write>(
        &mut self,
        stream: &mut S,
        buf: &mut W,
    ) -> Result<(bool, usize, OpCode), WsError> {
        fn read_mask<S: Read>(stream: &mut S) -> Result<[u8; 4], std::io::Error> {
            let mut mask = [0u8; 4];
            stream.read_exact(&mut mask)?;
            Ok(mask)
        }

        stream.read_exact(&mut self.header_buf[..2])?;
        let len = self.header_buf[1] & 0b01111111;
        let masked = get_bit(&self.header_buf, 1, 0);
        let fin = get_bit(&self.header_buf, 0, 0);
        let code = parse_opcode(self.header_buf[0]);
        let len = match len {
            0..=125 => {
                let len = len as u64;
                if masked {
                    let mask = read_mask(stream)?;
                    if len > self.masked_buf.len() as u64 {
                        self.masked_buf.resize(len as usize, 0);
                    }
                    let slice = &mut self.masked_buf[..len as usize];
                    stream.read_exact(slice)?;
                    apply_mask(slice, mask);
                    buf.write_all(slice)?;
                    Some(mask)
                } else {
                    let num = std::io::copy(&mut stream.take(len), buf)?;
                    if num != len {
                        return Err(WsError::IOError(std::io::Error::new(
                            std::io::ErrorKind::Other,
                            "writer cap is not enough",
                        )));
                    }
                    None
                };
                len
            }
            126 => {
                stream.read_exact(&mut self.header_buf[2..4])?;
                let mut len_raw = [0u8; 2];
                len_raw.copy_from_slice(&self.header_buf[2..4]);
                let len = u16::from_be_bytes(len_raw) as u64;
                if masked {
                    let mask = read_mask(stream)?;
                    if len > self.masked_buf.len() as u64 {
                        self.masked_buf.resize(len as usize, 0);
                    }
                    let slice = &mut self.masked_buf[..len as usize];
                    stream.read_exact(slice)?;
                    apply_mask(slice, mask);
                    buf.write_all(slice)?;
                    Some(mask)
                } else {
                    let num = std::io::copy(&mut stream.take(len), buf)?;
                    if num != len {
                        return Err(WsError::IOError(std::io::Error::new(
                            std::io::ErrorKind::Other,
                            "writer cap is not enough",
                        )));
                    }
                    None
                };
                len
            }
            127 => {
                stream.read_exact(&mut self.header_buf[2..10])?;
                let mut len_raw = [0u8; 8];
                len_raw.copy_from_slice(&self.header_buf[2..10]);
                let len = u64::from_be_bytes(len_raw);
                if masked {
                    let mask = read_mask(stream)?;
                    if len > self.masked_buf.len() as u64 {
                        self.masked_buf.resize(len as usize, 0);
                    }
                    let slice = &mut self.masked_buf[..len as usize];
                    stream.read_exact(slice)?;
                    apply_mask(slice, mask);
                    buf.write_all(slice)?;
                    Some(mask)
                } else {
                    let num = std::io::copy(&mut stream.take(len), buf)?;
                    if num != len {
                        return Err(WsError::IOError(std::io::Error::new(
                            std::io::ErrorKind::Other,
                            "writer cap is not enough",
                        )));
                    }
                    None
                };
                len
            }
            _ => {
                return Err(WsError::ProtocolError {
                    close_code: 1008,
                    error: ProtocolError::InvalidLeadingLen(len),
                })
            }
        };
        Ok((fin, len as usize, code))
    }
}

impl FrameReadState {
    fn poll<S: Read>(&mut self, stream: &mut S) -> IOResult<usize> {
        if self.read_idx + self.config.resize_thresh >= self.read_data.len() {
            self.read_data
                .resize(self.config.resize_size + self.read_data.len(), 0)
        }
        let count = stream.read(&mut self.read_data[self.read_idx..])?;
        self.read_idx += count;
        if count == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::ConnectionAborted,
                "read eof",
            ));
        }
        Ok(count)
    }

    fn poll_one_frame<S: Read>(&mut self, stream: &mut S, size: usize) -> IOResult<usize> {
        let start = self.read_idx;
        while self.read_idx < size {
            self.poll(stream)?;
        }
        let num = self.read_idx - start;
        Ok(num)
    }

    fn read_one_frame<S: Read>(&mut self, stream: &mut S) -> Result<OwnedFrame, WsError> {
        while !self.is_header_ok() {
            self.poll(stream)?;
        }
        let len = self.parse_frame_header()?;
        self.poll_one_frame(stream, len)?;
        Ok(self.consume_frame(len))
    }

    /// **NOTE** masked frame has already been unmasked
    pub fn receive<S: Read>(&mut self, stream: &mut S) -> Result<OwnedFrame, WsError> {
        loop {
            let frame = self.read_one_frame(stream)?;
            if self.config.merge_frame {
                if let Some(frame) = self
                    .check_frame(frame)
                    .and_then(|frame| self.merge_frame(frame))?
                {
                    break Ok(frame);
                }
            } else {
                break self.check_frame(frame);
            }
        }
    }
}

impl FrameWriteState {
    // DOUBT return error if payload len data >= 126 ?
    /// send immutable payload
    ///
    /// if need to mask, copy data to inner buffer and then apply mask
    ///
    /// will auto fragment if auto_fragment_size > 0
    pub fn send<S: Write>(
        &mut self,
        stream: &mut S,
        opcode: OpCode,
        payload: &[u8],
    ) -> IOResult<()> {
        if payload.is_empty() {
            let mask = if self.config.mask_send_frame {
                Some(rand::random())
            } else {
                None
            };
            let header = ctor_header(
                &mut self.header_buf,
                true,
                false,
                false,
                false,
                mask,
                opcode,
                0,
            );
            stream.write_all(header)?;
            return Ok(());
        }
        if self.config.auto_fragment_size > 0 && self.config.auto_fragment_size < payload.len() {
            let chunk_size = self.config.auto_fragment_size;
            let parts: Vec<&[u8]> = payload.chunks(chunk_size).collect();
            let total = parts.len();
            let single_header_len = header_len(self.config.mask_send_frame, chunk_size as u64);
            let mut header_buf_len = single_header_len * parts.len();
            let left = payload.len() % chunk_size;
            if left != 0 {
                header_buf_len += header_len(self.config.mask_send_frame, left as u64);
            }
            let total_bytes = payload.len() + header_buf_len;
            if self.config.mask_send_frame {
                if self.buf.len() < total_bytes {
                    self.buf.resize(total_bytes, 0);
                }
                parts.iter().enumerate().for_each(|(idx, chunk)| {
                    let fin = idx + 1 == total;
                    let s_idx = idx * single_header_len;
                    let mask = rand::random();
                    let header_len = ctor_header(
                        &mut self.buf[s_idx..],
                        fin,
                        false,
                        false,
                        false,
                        mask,
                        opcode,
                        chunk.len() as u64,
                    )
                    .len();
                    apply_mask(&mut self.buf[(s_idx + header_len)..], mask);
                });
                stream.write_all(&self.buf[..total_bytes])?;
            } else {
                if self.buf.len() < header_buf_len {
                    self.buf.resize(header_buf_len, 0);
                }
                let mut slices = Vec::with_capacity(total * 2);
                parts.iter().enumerate().for_each(|(idx, chunk)| {
                    let fin = idx + 1 == total;
                    let s_idx = idx * chunk_size;
                    ctor_header(
                        &mut self.buf[s_idx..],
                        fin,
                        false,
                        false,
                        false,
                        None,
                        opcode,
                        chunk.len() as u64,
                    );
                });
                parts.iter().enumerate().for_each(|(idx, chunk)| {
                    let fin = idx + 1 == total;
                    if fin {
                        slices.push(IoSlice::new(&self.buf[(idx * single_header_len)..]))
                    } else {
                        slices.push(IoSlice::new(
                            &self.buf[(idx * single_header_len)..(idx + 1) * single_header_len],
                        ))
                    }
                    slices.push(IoSlice::new(chunk));
                });
                let num = stream.write_vectored(&slices)?;
                let remain = total_bytes - num;
                if remain > 0 {
                    if let Some(buf) = slices.last() {
                        stream.write_all(&buf[(buf.len() - remain)..])?;
                    }
                }
            }
        } else {
            if self.config.mask_send_frame {
                let total_bytes = header_len(true, payload.len() as u64) + payload.len();
                let mask: [u8; 4] = rand::random();
                let header = ctor_header(
                    &mut self.header_buf,
                    true,
                    false,
                    false,
                    false,
                    mask,
                    opcode,
                    payload.len() as u64,
                );
                if self.buf.len() < payload.len() {
                    self.buf.resize(payload.len(), 0)
                }
                apply_mask(&mut self.buf, mask);
                let num = stream.write_vectored(&[
                    IoSlice::new(header),
                    IoSlice::new(&self.buf[..(payload.len())]),
                ])?;
                let remain = total_bytes - num;
                if remain > 0 {
                    stream.write_all(&self.buf[(payload.len() - remain)..(payload.len())])?;
                }
            } else {
                let total_bytes = header_len(false, payload.len() as u64) + payload.len();
                let header = ctor_header(
                    &mut self.header_buf,
                    true,
                    false,
                    false,
                    false,
                    None,
                    opcode,
                    payload.len() as u64,
                );
                if self.buf.len() < payload.len() {
                    self.buf.resize(payload.len(), 0)
                }
                let num = stream.write_vectored(&[
                    IoSlice::new(header),
                    IoSlice::new(&self.buf[..(payload.len())]),
                ])?;
                let remain = total_bytes - num;
                if remain > 0 {
                    stream.write_all(&self.buf[(payload.len() - remain)..(payload.len())])?;
                }
            }
        };

        if self.config.renew_buf_on_write {
            self.buf = BytesMut::new()
        }
        Ok(())
    }

    pub(crate) fn send_owned_frame<S: Write>(
        &mut self,
        stream: &mut S,
        frame: OwnedFrame,
    ) -> IOResult<()> {
        let header = IoSlice::new(&frame.header().0);
        let body = IoSlice::new(frame.payload());
        let total = header.len() + body.len();
        let num = stream.write_vectored(&[header, body])?;
        let remain = total - num;
        if remain > 0 {
            stream.write_all(&body[(body.len() - remain)..])?
        }
        Ok(())
    }
}

/// recv part of websocket stream
pub struct FrameRecv<S: Read> {
    stream: S,
    read_state: FrameReadState,
}

impl<S: Read> FrameRecv<S> {
    /// construct method
    pub fn new(stream: S, read_state: FrameReadState) -> Self {
        Self { stream, read_state }
    }

    /// receive a frame
    pub fn receive(&mut self) -> Result<OwnedFrame, WsError> {
        self.read_state.receive(&mut self.stream)
    }
}

/// send part of websocket frame
pub struct FrameSend<S: Write> {
    stream: S,
    write_state: FrameWriteState,
}

impl<S: Write> FrameSend<S> {
    /// construct method
    pub fn new(stream: S, write_state: FrameWriteState) -> Self {
        Self {
            stream,
            write_state,
        }
    }

    /// send payload
    ///
    /// will auto fragment if auto_fragment_size > 0
    pub fn send(&mut self, code: OpCode, payload: &[u8]) -> Result<(), WsError> {
        self.write_state
            .send(&mut self.stream, code, payload)
            .map_err(WsError::IOError)
    }

    /// send a read frame, **this method will not check validation of frame and do not fragment**
    pub fn send_owned_frame(&mut self, frame: OwnedFrame) -> Result<(), WsError> {
        self.write_state
            .send_owned_frame(&mut self.stream, frame)
            .map_err(WsError::IOError)
    }

    /// flush stream to ensure all data are send
    pub fn flush(&mut self) -> Result<(), WsError> {
        self.stream.flush().map_err(WsError::IOError)
    }
}

/// recv/send websocket frame
pub struct FrameNewCodec<S: Read + Write> {
    /// underlying transport stream
    pub stream: S,
    /// read state
    pub read_state: FrameNewReadState,
    /// write state
    pub write_state: FrameWriteState,
}

impl<S: Read + Write> FrameNewCodec<S> {
    /// construct method
    pub fn new(stream: S) -> Self {
        Self {
            stream,
            read_state: FrameNewReadState::default(),
            write_state: FrameWriteState::default(),
        }
    }

    /// construct with stream and config
    pub fn new_with(stream: S, config: FrameConfig) -> Self {
        Self {
            stream,
            read_state: FrameNewReadState::with_config(config.clone()),
            write_state: FrameWriteState::with_config(config),
        }
    }

    /// get mutable underlying stream
    pub fn stream_mut(&mut self) -> &mut S {
        &mut self.stream
    }

    /// used for server side to construct a new server
    pub fn factory(_req: http::Request<()>, stream: S) -> Result<Self, WsError> {
        let config = FrameConfig {
            mask_send_frame: false,
            ..Default::default()
        };
        Ok(Self::new_with(stream, config))
    }

    /// used to client side to construct a new client
    pub fn check_fn(key: String, resp: http::Response<()>, stream: S) -> Result<Self, WsError> {
        standard_handshake_resp_check(key.as_bytes(), &resp)?;
        Ok(Self::new_with(stream, Default::default()))
    }

    /// receive a frame
    pub fn receive<W: Write>(&mut self, buf: &mut W) -> Result<(usize, OpCode), WsError> {
        self.read_state.receive(&mut self.stream, buf)
    }

    /// send data, **will copy data if need mask**
    pub fn send(&mut self, code: OpCode, payload: &[u8]) -> Result<(), WsError> {
        self.write_state
            .send(&mut self.stream, code, payload)
            .map_err(WsError::IOError)
    }

    /// send a read frame, **this method will not check validation of frame and do not fragment**
    pub fn send_owned_frame(&mut self, frame: OwnedFrame) -> Result<(), WsError> {
        self.write_state
            .send_owned_frame(&mut self.stream, frame)
            .map_err(WsError::IOError)
    }

    /// flush stream to ensure all data are send
    pub fn flush(&mut self) -> Result<(), WsError> {
        self.stream.flush().map_err(WsError::IOError)
    }
}

/// recv/send websocket frame
pub struct FrameCodec<S: Read + Write> {
    /// underlying transport stream
    pub stream: S,
    /// read state
    pub read_state: FrameReadState,
    /// write state
    pub write_state: FrameWriteState,
}

impl<S: Read + Write> FrameCodec<S> {
    /// construct method
    pub fn new(stream: S) -> Self {
        Self {
            stream,
            read_state: FrameReadState::default(),
            write_state: FrameWriteState::default(),
        }
    }

    /// construct with stream and config
    pub fn new_with(stream: S, config: FrameConfig) -> Self {
        Self {
            stream,
            read_state: FrameReadState::with_config(config.clone()),
            write_state: FrameWriteState::with_config(config),
        }
    }

    /// get mutable underlying stream
    pub fn stream_mut(&mut self) -> &mut S {
        &mut self.stream
    }

    /// used for server side to construct a new server
    pub fn factory(_req: http::Request<()>, stream: S) -> Result<Self, WsError> {
        let config = FrameConfig {
            mask_send_frame: false,
            ..Default::default()
        };
        Ok(Self::new_with(stream, config))
    }

    /// used to client side to construct a new client
    pub fn check_fn(key: String, resp: http::Response<()>, stream: S) -> Result<Self, WsError> {
        standard_handshake_resp_check(key.as_bytes(), &resp)?;
        Ok(Self::new_with(stream, Default::default()))
    }

    /// receive a frame
    pub fn receive(&mut self) -> Result<OwnedFrame, WsError> {
        self.read_state.receive(&mut self.stream)
    }

    /// send data, **will copy data if need mask**
    pub fn send(&mut self, code: OpCode, payload: &[u8]) -> Result<(), WsError> {
        self.write_state
            .send(&mut self.stream, code, payload)
            .map_err(WsError::IOError)
    }

    /// send a read frame, **this method will not check validation of frame and do not fragment**
    pub fn send_owned_frame(&mut self, frame: OwnedFrame) -> Result<(), WsError> {
        self.write_state
            .send_owned_frame(&mut self.stream, frame)
            .map_err(WsError::IOError)
    }

    /// flush stream to ensure all data are send
    pub fn flush(&mut self) -> Result<(), WsError> {
        self.stream.flush().map_err(WsError::IOError)
    }
}

impl<R, W, S> FrameCodec<S>
where
    R: Read,
    W: Write,
    S: Read + Write + Split<R = R, W = W>,
{
    /// split codec to recv and send parts
    pub fn split(self) -> (FrameRecv<R>, FrameSend<W>) {
        let FrameCodec {
            stream,
            read_state,
            write_state,
        } = self;
        let (read, write) = stream.split();
        (
            FrameRecv::new(read, read_state),
            FrameSend::new(write, write_state),
        )
    }
}
