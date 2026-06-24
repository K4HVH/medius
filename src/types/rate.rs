//! Decoded `RESP(RATE)` — the live native report rate (§4.5).

use crate::protocol::opcode::{RATE_CHANGE_DRIVEN, RATE_CONFIDENT};

/// The live native report rate the firmware tracks off the report stream, plus the cloned poll period.
/// Class-aware: a moving mouse has a continuous cadence; a keyboard/media collection is `change_driven`
/// (no per-report rate) so `native_period_us` is `0` and only the poll floor is meaningful. Observability,
/// not control: the firmware paces continuous injection itself. A 1 kHz mouse reads ~1000 µs; a keyboard
/// reads `change_driven` with its ~1 ms poll floor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rate {
    /// Realised native report period in µs (`0` = not learned yet, or N/A for a change-driven input).
    pub native_period_us: u16,
    /// Cloned inject-endpoint `bInterval` poll period in µs (the active input's endpoint).
    pub poll_period_us: u16,
    /// The estimator window is full, so this value is trustworthy (mirrors HEALTH `rate_confident`).
    pub confident: bool,
    /// The active input is change-driven (keyboard/media): no continuous cadence, poll floor only.
    pub change_driven: bool,
}

impl Rate {
    /// The native report rate in Hz, or `None` when there is no continuous cadence (`native_period_us ==
    /// 0`, i.e. unlearned or change-driven).
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
            change_driven: p[5] & RATE_CHANGE_DRIVEN != 0,
        })
    }
}
