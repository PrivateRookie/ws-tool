use bytes::BytesMut;

use super::{apply_mask_fast32, FrameConfig, FrameReadState, FrameWriteState};
use crate::{
    codec::Split,
    errors::WsError,
    frame::{Header, OpCode, OwnedFrame},
    protocol::standard_handshake_resp_check,
};
use std::io::{Read, Write};

type IOResult<T> = std::io::Result<T>;

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
        let mask_send = self.config.mask_send_frame;
        let mask_fn = || {
            if mask_send {
                Some(rand::random())
            } else {
                None
            }
        };

        if payload.is_empty() {
            let mask = mask_fn();
            let header = Header::new(true, false, false, false, mask, opcode, 0);
            stream.write_all(&header.0)?;
            return Ok(());
        }
        let chunk_size = if self.config.auto_fragment_size > 0 {
            self.config.auto_fragment_size
        } else {
            payload.len()
        };
        let parts: Vec<&[u8]> = payload.chunks(chunk_size).collect();
        let total = parts.len();
        for (idx, chunk) in parts.into_iter().enumerate() {
            let fin = idx + 1 == total;
            let mask = mask_fn();
            let header = Header::new(fin, false, false, false, mask, opcode, chunk.len() as u64);
            stream.write_all(&header.0)?;
            if let Some(mask) = mask {
                let mut data = BytesMut::from_iter(chunk);
                apply_mask_fast32(&mut data, mask);
                stream.write_all(&data)?;
            } else {
                stream.write_all(chunk)?;
            }
        }
        Ok(())
    }

    pub(crate) fn send_owned_frame<S: Write>(
        &mut self,
        stream: &mut S,
        frame: OwnedFrame,
    ) -> IOResult<()> {
        stream.write_all(&frame.header().0)?;
        stream.write_all(frame.payload())
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
            .map_err(|e| WsError::IOError(Box::new(e)))
    }

    /// send a read frame, **this method will not check validation of frame and do not fragment**
    pub fn send_owned_frame(&mut self, frame: OwnedFrame) -> Result<(), WsError> {
        self.write_state
            .send_owned_frame(&mut self.stream, frame)
            .map_err(|e| WsError::IOError(Box::new(e)))
    }

    /// flush stream to ensure all data are send
    pub fn flush(&mut self) -> Result<(), WsError> {
        self.stream
            .flush()
            .map_err(|e| WsError::IOError(Box::new(e)))
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
            .map_err(|e| WsError::IOError(Box::new(e)))
    }

    /// send a read frame, **this method will not check validation of frame and do not fragment**
    pub fn send_owned_frame(&mut self, frame: OwnedFrame) -> Result<(), WsError> {
        self.write_state
            .send_owned_frame(&mut self.stream, frame)
            .map_err(|e| WsError::IOError(Box::new(e)))
    }

    /// flush stream to ensure all data are send
    pub fn flush(&mut self) -> Result<(), WsError> {
        self.stream
            .flush()
            .map_err(|e| WsError::IOError(Box::new(e)))
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
