//! Frame encoding and a streaming decoder — the wire packet codec.

use super::crc::crc16_ccitt;
use super::opcode::{FrameType, MAX_PAYLOAD, SOF};

/// Error returned by [`encode`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameError {
    /// The payload exceeds [`MAX_PAYLOAD`] and cannot be framed.
    PayloadTooLong { len: usize },
}

impl core::fmt::Display for FrameError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            FrameError::PayloadTooLong { len } => {
                write!(f, "payload too long: {len} bytes (max {MAX_PAYLOAD})")
            }
        }
    }
}

impl core::error::Error for FrameError {}

/// A fully decoded frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedFrame {
    pub ty: FrameType,
    pub seq: u8,
    pub payload: Vec<u8>,
}

/// Encode a frame: `[SOF][TYPE][SEQ][LEN_LO][LEN_HI][PAYLOAD..][CRC_LO][CRC_HI]`.
pub fn encode(ty: FrameType, seq: u8, payload: &[u8]) -> Result<Vec<u8>, FrameError> {
    if payload.len() > MAX_PAYLOAD {
        return Err(FrameError::PayloadTooLong { len: payload.len() });
    }
    let len = payload.len() as u16;
    let len_lo = (len & 0xFF) as u8;
    let len_hi = (len >> 8) as u8;

    let mut crc_input = Vec::with_capacity(4 + payload.len());
    crc_input.push(ty as u8);
    crc_input.push(seq);
    crc_input.push(len_lo);
    crc_input.push(len_hi);
    crc_input.extend_from_slice(payload);
    let crc = crc16_ccitt(&crc_input);

    let mut frame = Vec::with_capacity(7 + payload.len());
    frame.push(SOF);
    frame.push(ty as u8);
    frame.push(seq);
    frame.push(len_lo);
    frame.push(len_hi);
    frame.extend_from_slice(payload);
    frame.push((crc & 0xFF) as u8);
    frame.push((crc >> 8) as u8);
    Ok(frame)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Sof,
    Type,
    Seq,
    LenLo,
    LenHi,
    Payload,
    CrcLo,
    CrcHi,
}

/// A streaming frame decoder that invokes a callback per valid, CRC-checked, known-opcode frame.
#[derive(Debug)]
pub struct FrameDecoder {
    state: State,
    ty: u8,
    seq: u8,
    len: usize,
    buf: Vec<u8>,
    crc_rx: u16,
    crc_error_count: u64,
}

impl Default for FrameDecoder {
    fn default() -> Self {
        Self::new()
    }
}

impl FrameDecoder {
    /// Create a fresh decoder in the idle (scanning-for-SOF) state.
    pub fn new() -> Self {
        FrameDecoder {
            state: State::Sof,
            ty: 0,
            seq: 0,
            len: 0,
            buf: Vec::new(),
            crc_rx: 0,
            crc_error_count: 0,
        }
    }

    /// Number of frames dropped because their CRC failed.
    pub fn crc_error_count(&self) -> u64 {
        self.crc_error_count
    }

    /// Feed `data`, invoking `on_frame` once per valid, known-opcode frame.
    pub fn feed(&mut self, data: &[u8], mut on_frame: impl FnMut(DecodedFrame)) {
        for &b in data {
            self.feed_byte(b, &mut on_frame);
        }
    }

    fn feed_byte(&mut self, b: u8, on_frame: &mut impl FnMut(DecodedFrame)) {
        match self.state {
            State::Sof => {
                if b == SOF {
                    self.state = State::Type;
                }
            }
            State::Type => {
                self.ty = b;
                self.state = State::Seq;
            }
            State::Seq => {
                self.seq = b;
                self.state = State::LenLo;
            }
            State::LenLo => {
                self.len = b as usize;
                self.state = State::LenHi;
            }
            State::LenHi => {
                self.len |= (b as usize) << 8;
                self.buf.clear();
                if self.len > MAX_PAYLOAD {
                    self.state = State::Sof;
                } else if self.len == 0 {
                    self.state = State::CrcLo;
                } else {
                    self.state = State::Payload;
                }
            }
            State::Payload => {
                self.buf.push(b);
                if self.buf.len() >= self.len {
                    self.state = State::CrcLo;
                }
            }
            State::CrcLo => {
                self.crc_rx = b as u16;
                self.state = State::CrcHi;
            }
            State::CrcHi => {
                self.crc_rx |= (b as u16) << 8;
                self.finish_frame(on_frame);
                self.state = State::Sof;
            }
        }
    }

    fn finish_frame(&mut self, on_frame: &mut impl FnMut(DecodedFrame)) {
        let len = self.buf.len() as u16;
        let mut crc_input = Vec::with_capacity(4 + self.buf.len());
        crc_input.push(self.ty);
        crc_input.push(self.seq);
        crc_input.push((len & 0xFF) as u8);
        crc_input.push((len >> 8) as u8);
        crc_input.extend_from_slice(&self.buf);

        if crc16_ccitt(&crc_input) != self.crc_rx {
            self.crc_error_count += 1;
            return;
        }

        if let Ok(ty) = FrameType::try_from(self.ty) {
            on_frame(DecodedFrame {
                ty,
                seq: self.seq,
                payload: core::mem::take(&mut self.buf),
            });
        }
    }
}
