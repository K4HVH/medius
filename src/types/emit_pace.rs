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

/// How injected motion is emitted. [`Off`](RenderMode::Off) is the paced fill; the others render the
/// device's report texture and differ only in the onboard path smoother. It is `OPTION(EMIT)`'s own
/// `render` field, independent of the [`EmitPace`] beside it, which caps the rendered rate.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum RenderMode {
    /// Paced fill, renderer off.
    Off = 0,
    /// Rendered with the bit-exact triangular smoother.
    Stock = 1,
    /// Rendered with the smoother's onset ramped rather than stepped (the box's factory default).
    #[default]
    Despiked = 2,
    /// Rendered with no smoother; the model renders raw injection.
    Unsmoothed = 3,
}

impl RenderMode {
    /// The wire `render` byte.
    pub(crate) fn to_wire(self) -> u8 {
        self as u8
    }

    /// Decode a wire `render` byte; `None` is a value this crate does not know.
    pub(crate) fn from_wire(render: u8) -> Option<RenderMode> {
        match render {
            0 => Some(RenderMode::Off),
            1 => Some(RenderMode::Stock),
            2 => Some(RenderMode::Despiked),
            3 => Some(RenderMode::Unsmoothed),
            _ => None,
        }
    }
}

/// The configured [`EmitPace`] plus the emit-rate ceiling and the wire rate actually in effect (§4.14).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct EmitPaceStatus {
    /// The selected mode (for [`EmitPace::Fixed`], the rate the host requested).
    pub mode: EmitPace,
    /// The render mode composing onto the pace.
    pub render: RenderMode,
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
        if p.len() < 13 {
            return None;
        }
        let fixed_hz = u16::from_le_bytes([p[3], p[4]]);
        let resolved_hz = u16::from_le_bytes([p[5], p[6]]);
        let force_hz = u16::from_le_bytes([p[7], p[8]]);
        let advertised_hz = u16::from_le_bytes([p[9], p[10]]);
        let render = RenderMode::from_wire(p[12])?;
        let mode = match p[2] {
            0 => EmitPace::Learned,
            1 => EmitPace::Interval,
            2 => EmitPace::Fixed(fixed_hz),
            _ => return None,
        };
        Some(EmitPaceStatus {
            mode,
            render,
            resolved_hz,
            force_hz: (force_hz != 0).then_some(force_hz),
            advertised_hz,
            force_active: p[11] != 0,
        })
    }
}
