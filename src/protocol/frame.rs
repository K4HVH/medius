//! Frame encoding and a streaming decoder — the wire packet codec.
//!
//! Wire frame (§2 of `control-protocol.md`):
//!
//! ```text
//! [SOF 0xA5][TYPE u8][SEQ u8][LEN u16 LE][PAYLOAD (LEN)][CRC16 u16 LE]
//! ```
//!
//! The CRC ([`super::crc`]) covers `TYPE | SEQ | LEN | PAYLOAD` — not SOF, not the CRC bytes.
//! [`FrameDecoder`] is a port of `medius.py`'s `_Decoder`.

use super::crc::crc16_ccitt;
use super::opcode::{FrameType, MAX_PAYLOAD, SOF};

/// Error returned by [`encode`]. A local error so `protocol/` stays free of any crate-wide `Error`
/// (the device layer wraps it). Encoding is otherwise infallible.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameError {
    /// The payload exceeds [`MAX_PAYLOAD`] and cannot be framed (§2).
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

/// A fully decoded frame. Only known opcodes reach here; unknown types are dropped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedFrame {
    pub ty: FrameType,
    pub seq: u8,
    pub payload: Vec<u8>,
}

/// Encode a frame: `[SOF][TYPE][SEQ][LEN_LO][LEN_HI][PAYLOAD..][CRC_LO][CRC_HI]`.
///
/// # Errors
/// Returns [`FrameError::PayloadTooLong`] if `payload` exceeds [`MAX_PAYLOAD`].
///
/// # Examples
/// ```ignore
/// # use medius::protocol::frame::encode;
/// # use medius::protocol::opcode::FrameType;
/// let f = encode(FrameType::Reset, 7, &[]).unwrap();
/// assert_eq!(f[0], 0xA5); // SOF
/// assert_eq!(f[1], 0x04); // TYPE = RESET
/// assert_eq!(f[2], 7); // SEQ
/// ```
pub fn encode(ty: FrameType, seq: u8, payload: &[u8]) -> Result<Vec<u8>, FrameError> {
    if payload.len() > MAX_PAYLOAD {
        return Err(FrameError::PayloadTooLong { len: payload.len() });
    }
    let len = payload.len() as u16;
    let len_lo = (len & 0xFF) as u8;
    let len_hi = (len >> 8) as u8;

    // CRC covers TYPE | SEQ | LEN | PAYLOAD (header without SOF, plus payload).
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

/// Decoder position in the wire format. `Sof` is the resync/idle state.
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

/// A streaming frame decoder. Feed it bytes ([`FrameDecoder::feed`]); it invokes a callback per
/// valid, CRC-checked, known-opcode frame. Deterministic and panic-free on any input (§2):
///
/// - **Resync:** non-frame bytes before a SOF are ignored; a stray SOF restarts framing.
/// - **CRC drop:** a CRC failure is dropped silently, counted in [`FrameDecoder::crc_error_count`].
/// - **Oversize LEN:** a `LEN` > [`MAX_PAYLOAD`] resyncs without allocating the bogus size, counted
///   in [`FrameDecoder::resync_count`].
/// - **Unknown opcode:** a CRC-valid frame with an unknown `TYPE` is consumed and ignored
///   (forward-compat), counted in [`FrameDecoder::unknown_type_count`].
#[derive(Debug)]
pub struct FrameDecoder {
    state: State,
    ty: u8,
    seq: u8,
    len: usize,
    buf: Vec<u8>,
    crc_rx: u16,
    crc_error_count: u64,
    resync_count: u64,
    unknown_type_count: u64,
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
            resync_count: 0,
            unknown_type_count: 0,
        }
    }

    /// Number of frames dropped because their CRC failed.
    pub fn crc_error_count(&self) -> u64 {
        self.crc_error_count
    }

    /// Number of times an oversize `LEN` (> [`MAX_PAYLOAD`]) forced a resync.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn resync_count(&self) -> u64 {
        self.resync_count
    }

    /// Number of CRC-valid frames dropped because their `TYPE` was an unknown opcode.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn unknown_type_count(&self) -> u64 {
        self.unknown_type_count
    }

    /// Feed `data`, invoking `on_frame` once per valid, known-opcode frame. Bytes may arrive in any
    /// chunking; framing state persists across calls.
    pub fn feed(&mut self, data: &[u8], mut on_frame: impl FnMut(DecodedFrame)) {
        for &b in data {
            self.feed_byte(b, &mut on_frame);
        }
    }

    /// Convenience: feed `data` and collect all decoded frames into a `Vec`.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn feed_collect(&mut self, data: &[u8]) -> Vec<DecodedFrame> {
        let mut out = Vec::new();
        self.feed(data, |f| out.push(f));
        out
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
                    // Resync without allocating the bogus size.
                    self.resync_count += 1;
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
            // Corrupt frame: drop silently (§2).
            self.crc_error_count += 1;
            return;
        }

        match FrameType::try_from(self.ty) {
            Ok(ty) => on_frame(DecodedFrame {
                ty,
                seq: self.seq,
                payload: core::mem::take(&mut self.buf),
            }),
            Err(_) => {
                // Unknown opcode: consume and ignore (§2 forward-compat).
                self.unknown_type_count += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A MOVE frame encodes to the exact byte layout with the CRC over TYPE|SEQ|LEN|PAYLOAD.
    #[test]
    fn encode_move_exact_bytes() {
        let payload = [0x28, 0x00, 0x00, 0x00]; // dx=40, dy=0
        let frame = encode(FrameType::Move, 0x11, &payload).unwrap();
        assert_eq!(
            &frame[..9],
            &[SOF, 0x01, 0x11, 0x04, 0x00, 0x28, 0x00, 0x00, 0x00]
        );
        let crc = crc16_ccitt(&[0x01, 0x11, 0x04, 0x00, 0x28, 0x00, 0x00, 0x00]);
        assert_eq!(frame[9], (crc & 0xFF) as u8);
        assert_eq!(frame[10], (crc >> 8) as u8);
        assert_eq!(frame.len(), 11);
    }

    /// An empty-payload frame (e.g. RESET) has LEN 0 and a 7-byte total length.
    #[test]
    fn encode_empty_payload() {
        let frame = encode(FrameType::Reset, 0, &[]).unwrap();
        assert_eq!(&frame[..5], &[SOF, 0x04, 0x00, 0x00, 0x00]);
        assert_eq!(frame.len(), 7);
    }

    /// A payload of exactly MAX_PAYLOAD is allowed; one byte more is rejected.
    #[test]
    fn encode_length_boundary() {
        let ok = vec![0u8; MAX_PAYLOAD];
        assert!(encode(FrameType::Resp, 0, &ok).is_ok());

        let too_long = vec![0u8; MAX_PAYLOAD + 1];
        assert_eq!(
            encode(FrameType::Resp, 0, &too_long),
            Err(FrameError::PayloadTooLong {
                len: MAX_PAYLOAD + 1
            })
        );
    }

    /// Round-trip: an encoded frame decodes back to the same (type, seq, payload).
    #[test]
    fn round_trip() {
        let payload = [0xDE, 0xAD, 0xBE, 0xEF];
        let frame = encode(FrameType::Button, 0x42, &payload).unwrap();
        let mut dec = FrameDecoder::new();
        let frames = dec.feed_collect(&frame);
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].ty, FrameType::Button);
        assert_eq!(frames[0].seq, 0x42);
        assert_eq!(frames[0].payload, payload);
        assert_eq!(dec.crc_error_count(), 0);
    }

    /// Round-trip every opcode and an empty payload.
    #[test]
    fn round_trip_all_opcodes() {
        for ty in [
            FrameType::Move,
            FrameType::Wheel,
            FrameType::Button,
            FrameType::Reset,
            FrameType::Query,
            FrameType::Resp,
            FrameType::RebootDl,
            FrameType::Log,
        ] {
            let frame = encode(ty, 1, &[1, 2, 3]).unwrap();
            let mut dec = FrameDecoder::new();
            let frames = dec.feed_collect(&frame);
            assert_eq!(frames.len(), 1, "ty {ty:?}");
            assert_eq!(frames[0].ty, ty);
            assert_eq!(frames[0].payload, vec![1, 2, 3]);
        }
    }

    /// Garbage before SOF is ignored; the frame still decodes (resync on SOF).
    #[test]
    fn resync_after_garbage() {
        let frame = encode(FrameType::Move, 5, &[1, 0, 2, 0]).unwrap();
        let mut stream = vec![0x00, 0xFF, b'h', b'i', 0x7E]; // arbitrary non-frame bytes
        stream.extend_from_slice(&frame);
        let mut dec = FrameDecoder::new();
        let frames = dec.feed_collect(&stream);
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].seq, 5);
        assert_eq!(frames[0].payload, vec![1, 0, 2, 0]);
    }

    /// A flipped payload byte fails the CRC: the frame is silently dropped and the counter ticks.
    #[test]
    fn crc_failure_is_dropped() {
        let mut frame = encode(FrameType::Move, 0, &[1, 2, 3, 4]).unwrap();
        frame[5] ^= 0xFF; // first payload byte, after the 5-byte header
        let mut dec = FrameDecoder::new();
        let frames = dec.feed_collect(&frame);
        assert!(frames.is_empty());
        assert_eq!(dec.crc_error_count(), 1);
    }

    /// After a dropped CRC-fail frame, a subsequent good frame still decodes.
    #[test]
    fn recovers_after_crc_failure() {
        let mut bad = encode(FrameType::Move, 1, &[9, 9, 9, 9]).unwrap();
        bad[6] ^= 0x01;
        let good = encode(FrameType::Wheel, 2, &[3, 0]).unwrap();
        let mut stream = bad;
        stream.extend_from_slice(&good);
        let mut dec = FrameDecoder::new();
        let frames = dec.feed_collect(&stream);
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].ty, FrameType::Wheel);
        assert_eq!(frames[0].seq, 2);
        assert_eq!(dec.crc_error_count(), 1);
    }

    /// A LEN claiming more than MAX_PAYLOAD is rejected without panic or huge allocation; the
    /// decoder resyncs and a following valid frame decodes.
    #[test]
    fn oversize_len_resyncs() {
        // Hand-craft a frame header with LEN = 0xFFFF (65535 > 512).
        let bogus = [SOF, FrameType::Move as u8, 0, 0xFF, 0xFF];
        let good = encode(FrameType::Reset, 7, &[]).unwrap();
        let mut stream = bogus.to_vec();
        stream.extend_from_slice(&good);
        let mut dec = FrameDecoder::new();
        let frames = dec.feed_collect(&stream);
        assert_eq!(dec.resync_count(), 1);
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].ty, FrameType::Reset);
        assert_eq!(frames[0].seq, 7);
    }

    /// A CRC-valid frame with an unknown opcode is consumed and ignored (forward-compat), and a
    /// following known frame still decodes.
    #[test]
    fn unknown_opcode_ignored() {
        // Build a valid frame with TYPE = 0x7F (unknown) by hand, computing its CRC.
        let ty = 0x7Fu8;
        let payload = [0xAB];
        let mut crc_input = vec![ty, 3, 1, 0];
        crc_input.extend_from_slice(&payload);
        let crc = crc16_ccitt(&crc_input);
        let mut frame = vec![SOF, ty, 3, 1, 0];
        frame.extend_from_slice(&payload);
        frame.push((crc & 0xFF) as u8);
        frame.push((crc >> 8) as u8);

        let good = encode(FrameType::Query, 9, &[1]).unwrap();
        frame.extend_from_slice(&good);

        let mut dec = FrameDecoder::new();
        let frames = dec.feed_collect(&frame);
        assert_eq!(dec.unknown_type_count(), 1);
        assert_eq!(dec.crc_error_count(), 0);
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].ty, FrameType::Query);
        assert_eq!(frames[0].seq, 9);
    }

    /// Delivering the frame one byte at a time still decodes it (streaming).
    #[test]
    fn split_feed_one_byte_at_a_time() {
        let payload = [1, 2, 3, 4, 5, 6];
        let frame = encode(FrameType::Resp, 0x33, &payload).unwrap();
        let mut dec = FrameDecoder::new();
        let mut frames = Vec::new();
        for &b in &frame {
            dec.feed(&[b], |f| frames.push(f));
        }
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].ty, FrameType::Resp);
        assert_eq!(frames[0].seq, 0x33);
        assert_eq!(frames[0].payload, payload);
    }

    /// Two back-to-back frames in one feed both decode, in order.
    #[test]
    fn two_frames_in_one_feed() {
        let a = encode(FrameType::Move, 1, &[1, 0, 0, 0]).unwrap();
        let b = encode(FrameType::Wheel, 2, &[255, 255]).unwrap();
        let mut stream = a;
        stream.extend_from_slice(&b);
        let mut dec = FrameDecoder::new();
        let frames = dec.feed_collect(&stream);
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0].seq, 1);
        assert_eq!(frames[1].seq, 2);
    }

    /// A stray SOF embedded in noise can mis-frame the immediately following bytes (an inherent
    /// property of SOF-resync framing, shared with `medius.py`), but it never panics, the
    /// misframing is caught by the CRC, and the decoder recovers on the next clean frame.
    #[test]
    fn stray_sof_does_not_wedge_decoder() {
        let frame = encode(FrameType::Reset, 0, &[]).unwrap();
        let mut stream = vec![SOF, 0x13, 0x37, 0x00]; // partial junk after a stray SOF
        stream.extend_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF, 0x11, 0x22, 0x33, 0x44]);
        stream.extend_from_slice(&frame);
        let mut dec = FrameDecoder::new();
        let frames = dec.feed_collect(&stream);
        assert!(
            frames
                .iter()
                .any(|f| f.ty == FrameType::Reset && f.seq == 0)
        );
    }
}
