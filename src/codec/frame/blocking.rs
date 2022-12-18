use super::{FrameConfig, FrameReadState, FrameWriteState};
use crate::{
    codec::Split,
    errors::WsError,
    frame::{Header, OpCode, Payload, PayloadMut, ReadFrame},
    protocol::standard_handshake_resp_check,
};
use std::io::{Read, Write};

type IOResult<T> = std::io::Result<T>;

impl FrameReadState {
    fn poll<S: Read>(&mut self, stream: &mut S) -> IOResult<usize> {
        self.read_data.resize(self.read_idx + 1024, 0);
        let count = stream.read(&mut self.read_data[self.read_idx..])?;
        self.read_idx += count;
        self.read_data.resize(self.read_idx, 0);
        if count == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::ConnectionAborted,
                "read eof",
            ));
        }
        Ok(count)
    }

    fn poll_one_frame<S: Read>(&mut self, stream: &mut S, size: usize) -> IOResult<usize> {
        let buf_len = self.read_data.len();
        if buf_len < size {
            self.read_data.resize(size, 0);
            stream.read_exact(&mut self.read_data[buf_len..size])?;
            Ok(size - buf_len)
        } else {
            Ok(0)
        }
    }

    fn read_one_frame<S: Read>(&mut self, stream: &mut S) -> Result<ReadFrame, WsError> {
        while !self.is_header_ok() {
            self.poll(stream)?;
        }
        let len = self.parse_frame_header()?;
        self.poll_one_frame(stream, len)?;
        Ok(self.consume_frame(len))
    }

    /// **NOTE** masked frame has already been unmasked
    pub fn receive<S: Read>(&mut self, stream: &mut S) -> Result<ReadFrame, WsError> {
        loop {
            let frame = self.read_one_frame(stream)?;
            if let Some(frame) = self.check_frame(frame)? {
                break Ok(frame);
            }
        }
    }
}

impl FrameWriteState {
    #[allow(clippy::too_many_arguments)]
    fn send_one_mut<S: Write, M: Into<Option<[u8; 4]>>>(
        &mut self,
        stream: &mut S,
        fin: bool,
        rsv1: bool,
        rsv2: bool,
        rsv3: bool,
        mask_key: M,
        opcode: OpCode,
        mut payload: PayloadMut,
        mask_payload: bool,
    ) -> IOResult<()> {
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
                payload.apply_mask(key)
            }
        }
        // let payload_size: usize = payload.0.iter().map(|part| part.len()).sum();
        // if payload_size <= THRESHOLD {
        //     let mut data = header.0;
        //     data.reserve(payload_size);
        //     for part in payload.0 {
        //         data.extend_from_slice(part);
        //     }
        //     stream.write_all(&data)?;
        // } else {
        //     stream.write_all(&header.0)?;
        //     for part in payload.0 {
        //         stream.write_all(part)?;
        //     }
        // }
        stream.write_all(&header.0)?;
        for part in payload.0 {
            stream.write_all(part)?;
        }

        Ok(())
    }

    /// send mutable payload
    ///
    /// if mask_payload is set, mask payload first
    ///
    /// will auto fragment if auto_fragment_size > 0
    pub fn send_mut<S: Write>(
        &mut self,
        stream: &mut S,
        opcode: OpCode,
        payload: PayloadMut,
        mask_payload: bool,
    ) -> IOResult<()> {
        let split_size = self.config.auto_fragment_size;
        let mask_send = self.config.mask_send_frame;
        let mask_fn = || {
            if mask_send {
                Some(rand::random())
            } else {
                None
            }
        };
        if split_size > 0 {
            let parts = payload.split_with(split_size);
            let total = parts.len();
            for (idx, part) in parts.into_iter().enumerate() {
                let fin = idx + 1 == total;
                self.send_one_mut(
                    stream,
                    fin,
                    false,
                    false,
                    false,
                    mask_fn(),
                    opcode.clone(),
                    part,
                    mask_payload,
                )?;
            }
        } else {
            self.send_one_mut(
                stream,
                true,
                false,
                false,
                false,
                mask_fn(),
                opcode,
                payload,
                mask_payload,
            )?;
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn send_one<S: Write, M: Into<Option<[u8; 4]>>>(
        &mut self,
        stream: &mut S,
        fin: bool,
        rsv1: bool,
        rsv2: bool,
        rsv3: bool,
        mask_key: M,
        opcode: OpCode,
        payload: Payload,
    ) -> IOResult<()> {
        let header = Header::new(
            fin,
            rsv1,
            rsv2,
            rsv3,
            mask_key,
            opcode,
            payload.len() as u64,
        );
        stream.write_all(&header.0)?;
        for part in payload.0 {
            stream.write_all(part)?;
        }
        Ok(())
    }

    /// send immutable payload
    ///
    /// will auto fragment if auto_fragment_size > 0
    pub fn send<S: Write>(
        &mut self,
        stream: &mut S,
        opcode: OpCode,
        payload: Payload,
    ) -> IOResult<()> {
        let split_size = self.config.auto_fragment_size;
        let mask_send = self.config.mask_send_frame;
        let mask_fn = || {
            if mask_send {
                Some(rand::random())
            } else {
                None
            }
        };
        if split_size > 0 {
            let parts = payload.split_with(split_size);
            let total = parts.len();
            for (idx, part) in parts.into_iter().enumerate() {
                let fin = idx + 1 == total;
                self.send_one(
                    stream,
                    fin,
                    false,
                    false,
                    false,
                    mask_fn(),
                    opcode.clone(),
                    part,
                )?;
            }
        } else {
            self.send_one(
                stream,
                true,
                false,
                false,
                false,
                mask_fn(),
                opcode,
                payload,
            )?;
        }
        Ok(())
    }

    fn send_read_frame<S: Write>(&mut self, stream: &mut S, frame: ReadFrame) -> IOResult<()> {
        stream.write_all(&frame.0)
    }
}

macro_rules! impl_recv {
    () => {
        /// receive a frame
        pub fn receive(&mut self) -> Result<ReadFrame, WsError> {
            self.read_state.receive(&mut self.stream)
        }
    };
}

macro_rules! impl_send {
    () => {
        /// send mutable payload
        ///
        /// if mask_payload is set, mask payload first
        ///
        /// will auto fragment if auto_fragment_size > 0
        pub fn send_mut<'a, P: Into<PayloadMut<'a>>>(
            &mut self,
            code: OpCode,
            payload: P,
            mask_payload: bool,
        ) -> Result<(), WsError> {
            self.write_state
                .send_mut(&mut self.stream, code, payload.into(), mask_payload)
                .map_err(|e| WsError::IOError(Box::new(e)))
        }

        /// send immutable payload
        ///
        /// will auto fragment if auto_fragment_size > 0
        pub fn send<'a, P: Into<Payload<'a>>>(
            &mut self,
            code: OpCode,
            payload: P,
        ) -> Result<(), WsError> {
            self.write_state
                .send(&mut self.stream, code, payload.into())
                .map_err(|e| WsError::IOError(Box::new(e)))
        }

        /// send a read frame, **this method will not check validation of frame and do not fragment**
        pub fn send_read_frame(&mut self, frame: ReadFrame) -> Result<(), WsError> {
            self.write_state
                .send_read_frame(&mut self.stream, frame)
                .map_err(|e| WsError::IOError(Box::new(e)))
        }

        /// flush stream to ensure all data are send
        pub fn flush(&mut self) -> Result<(), WsError> {
            self.stream
                .flush()
                .map_err(|e| WsError::IOError(Box::new(e)))
        }
    };
}

/// recv part of websocket stream
pub struct WsFrameRecv<S: Read> {
    stream: S,
    read_state: FrameReadState,
}

impl<S: Read> WsFrameRecv<S> {
    /// construct method
    pub fn new(stream: S, read_state: FrameReadState) -> Self {
        Self { stream, read_state }
    }

    impl_recv! {}
}

/// send part of websocket frame
pub struct WsFrameSend<S: Write> {
    stream: S,
    write_state: FrameWriteState,
}

impl<S: Write> WsFrameSend<S> {
    /// construct method
    pub fn new(stream: S, write_state: FrameWriteState) -> Self {
        Self {
            stream,
            write_state,
        }
    }

    impl_send! {}
}

/// recv/send websocket frame
pub struct WsFrameCodec<S: Read + Write> {
    /// underlying transport stream
    pub stream: S,
    /// read state
    pub read_state: FrameReadState,
    /// write state
    pub write_state: FrameWriteState,
}

impl<S: Read + Write> WsFrameCodec<S> {
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
        Ok(Self::new_with(stream, FrameConfig::default()))
    }

    impl_recv! {}

    impl_send! {}
}

impl<R, W, S> WsFrameCodec<S>
where
    R: Read,
    W: Write,
    S: Read + Write + Split<R = R, W = W>,
{
    /// split codec to recv and send parts
    pub fn split(self) -> (WsFrameRecv<R>, WsFrameSend<W>) {
        let WsFrameCodec {
            stream,
            read_state,
            write_state,
        } = self;
        let (read, write) = stream.split();
        (
            WsFrameRecv::new(read, read_state),
            WsFrameSend::new(write, write_state),
        )
    }
}
