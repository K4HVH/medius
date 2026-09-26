//! Emit pacing: what paces injected motion, and the rate in effect (§4.14).

/// What paces injected motion (`OPTION(EMIT)`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum EmitPace {
    /// Pace to the learnt native report rate (the default).
    #[default]
    Learned,
    /// Pace to the cloned mouse's `bInterval` poll rate.
    Interval,
    /// Fixed rate in Hz, snapped by the 1 ms frame clock to `1000/n` Hz and capped at 1 kHz.
    Fixed(u16),
}

/// Configured [`EmitPace`], emit-rate ceiling, and wire rate in effect (§4.14).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct EmitPaceStatus {
    /// Selected mode (for [`EmitPace::Fixed`], the requested rate).
    pub mode: EmitPace,
    /// Ceiling in effect (Hz); 0 = learnt/adaptive, or no device yet in [`EmitPace::Interval`].
    /// Reads 1000 once the renderer has a profile, since a rendered stream paces itself (see
    /// [`set_render`](crate::Device::set_render)).
    pub resolved_hz: u16,
    /// Requested forced wire rate (Hz); `None` keeps the native interval.
    pub force_hz: Option<u16>,
    /// Rate the clone's input endpoints advertise now (Hz), forced or native; 0 = no clone.
    pub advertised_hz: u16,
    /// Whether the served descriptor carries a forced interval.
    pub force_active: bool,
}

impl EmitPaceStatus {
    /// Decodes a `RESP(OPTIONS, EMIT)` payload (§4.14).
    pub(crate) fn from_payload(p: &[u8]) -> Option<EmitPaceStatus> {
        if p.len() < 12 {
            return None;
        }
        let fixed_hz = u16::from_le_bytes([p[3], p[4]]);
        let resolved_hz = u16::from_le_bytes([p[5], p[6]]);
        let force_hz = u16::from_le_bytes([p[7], p[8]]);
        let advertised_hz = u16::from_le_bytes([p[9], p[10]]);
        let mode = match p[2] {
            0 => EmitPace::Learned,
            1 => EmitPace::Interval,
            2 => EmitPace::Fixed(fixed_hz),
            _ => return None,
        };
        Some(EmitPaceStatus {
            mode,
            resolved_hz,
            force_hz: (force_hz != 0).then_some(force_hz),
            advertised_hz,
            force_active: p[11] != 0,
        })
    }
}
