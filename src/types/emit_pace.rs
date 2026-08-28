//! Emit-rate pacing override: what paces injected motion, and the rate in effect (§4.14).

/// What paces injected motion (`OPTION(EMIT)`).
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

/// The configured [`EmitPace`] plus the emit-rate ceiling and the wire rate actually in effect (§4.14).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct EmitPaceStatus {
    /// The selected mode (for [`EmitPace::Fixed`], the rate the host requested).
    pub mode: EmitPace,
    /// Whether the renderer composes onto the mode, gating every 1 ms frame tick.
    pub rendered: bool,
    /// The ceiling currently in effect (Hz); 0 = learnt/adaptive, or no device yet in [`EmitPace::Interval`].
    pub resolved_hz: u16,
    /// The forced wire rate the host asked for (Hz); `None` leaves the device's own.
    pub force_hz: Option<u16>,
    /// What the clone's input endpoints advertise now (Hz), forced or native; 0 = no clone.
    pub advertised_hz: u16,
    /// Whether a forced interval is written into the descriptor being served.
    pub force_active: bool,
}

impl EmitPaceStatus {
    /// Decode a `RESP(OPTIONS, EMIT)` payload (§4.14).
    pub(crate) fn from_payload(p: &[u8]) -> Option<EmitPaceStatus> {
        if p.len() < 12 {
            return None;
        }
        let fixed_hz = u16::from_le_bytes([p[3], p[4]]);
        let resolved_hz = u16::from_le_bytes([p[5], p[6]]);
        let force_hz = u16::from_le_bytes([p[7], p[8]]);
        let advertised_hz = u16::from_le_bytes([p[9], p[10]]);
        let rendered = p[2] & 0x80 != 0;
        let mode = match p[2] & 0x7F {
            0 => EmitPace::Learned,
            1 => EmitPace::Interval,
            2 => EmitPace::Fixed(fixed_hz),
            _ => return None,
        };
        Some(EmitPaceStatus {
            mode,
            rendered,
            resolved_hz,
            force_hz: (force_hz != 0).then_some(force_hz),
            advertised_hz,
            force_active: p[11] != 0,
        })
    }
}
