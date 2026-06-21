//! Decoded `RESP(RATE)` — the live native report rate (§4.5).

use crate::protocol::opcode::RATE_CONFIDENT;

/// The live native report rate the firmware tracks off the report stream, plus the cloned poll
/// period. Observability, not control: the firmware paces injection to this rate itself. A host reads
/// it to confirm the box sees the real rate (e.g. a 1 kHz mouse reads ~1000 µs).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rate {
    /// Realised native report period in µs (`0` = not learned yet).
    pub native_period_us: u16,
    /// Cloned inject-endpoint `bInterval` poll period in µs.
    pub poll_period_us: u16,
    /// The estimator window is full, so this value is trustworthy (mirrors HEALTH `rate_confident`).
    pub confident: bool,
}

impl Rate {
    /// The native report rate in Hz, or `None` until learned (`native_period_us == 0`).
    pub fn native_hz(&self) -> Option<f32> {
        if self.native_period_us == 0 {
            None
        } else {
            Some(1_000_000.0 / f32::from(self.native_period_us))
        }
    }

    /// Decode a `RESP(RATE)` payload (§4.5):
    /// `[what][native_period_us u16][poll_period_us u16][flags u8]`.
    pub(crate) fn from_payload(p: &[u8]) -> Option<Self> {
        if p.len() < 6 {
            return None;
        }
        Some(Rate {
            native_period_us: u16::from_le_bytes([p[1], p[2]]),
            poll_period_us: u16::from_le_bytes([p[3], p[4]]),
            confident: p[5] & RATE_CONFIDENT != 0,
        })
    }
}
