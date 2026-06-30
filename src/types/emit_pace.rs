//! Emit-rate pacing override — what paces injected motion, and the rate in effect (§4.14).

/// What paces injected motion (`OPTION(EMIT)`). The box raises the emit-rate ceiling to match; idle
/// stays emit-when-pending, so the override never fills holds with synthetic frames — it only stops the
/// box re-pacing a host stream that already models its own report density.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum EmitPace {
    /// Pace to the mouse's learnt native report rate (the default).
    #[default]
    Learned,
    /// Pace to the cloned mouse's `bInterval` poll rate.
    Interval,
    /// A fixed rate in Hz. The 1 ms frame clock snaps it to `1000/n` Hz and caps it at 1 kHz.
    Fixed(u16),
}

/// The configured [`EmitPace`] plus the emit-rate ceiling actually in effect (§4.14).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct EmitPaceStatus {
    /// The selected mode (for [`EmitPace::Fixed`], the rate the host requested).
    pub mode: EmitPace,
    /// The ceiling currently in effect (Hz); 0 = learnt/adaptive, or no device yet in [`EmitPace::Interval`].
    pub resolved_hz: u16,
}

impl EmitPaceStatus {
    /// Decode a `RESP(OPTIONS, EMIT)` payload (§4.14): `[what][id][mode][fixed_hz u16 LE][resolved_hz u16 LE]`.
    pub(crate) fn from_payload(p: &[u8]) -> Option<EmitPaceStatus> {
        if p.len() < 7 {
            return None;
        }
        let fixed_hz = u16::from_le_bytes([p[3], p[4]]);
        let resolved_hz = u16::from_le_bytes([p[5], p[6]]);
        let mode = match p[2] {
            0 => EmitPace::Learned,
            1 => EmitPace::Interval,
            2 => EmitPace::Fixed(fixed_hz),
            _ => return None,
        };
        Some(EmitPaceStatus { mode, resolved_hz })
    }
}
