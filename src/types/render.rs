//! What the box draws motion with, and whether the device's own motion goes through it (§4.14).

/// The texture the box draws motion with (`OPTION(RENDER)`'s `mode`). [`Off`](RenderMode::Off) is the
/// paced fill; the others draw the device's learned report texture and differ only in the onboard path
/// smoother. Independent of the [`EmitPace`](crate::EmitPace) beside it, which caps the rendered rate.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum RenderMode {
    /// Paced fill, renderer off.
    Off = 0,
    /// Drawn with the bit-exact triangular smoother.
    Stock = 1,
    /// Drawn with the smoother's onset ramped rather than stepped (the box's factory default).
    #[default]
    Despiked = 2,
    /// Drawn with no smoother; the model receives raw injection.
    Unsmoothed = 3,
}

impl RenderMode {
    /// The wire `mode` byte.
    pub(crate) fn to_wire(self) -> u8 {
        self as u8
    }

    /// Map a wire `mode` byte to a [`RenderMode`], or `None` for an unknown value.
    pub fn from_u8(v: u8) -> Option<RenderMode> {
        Some(match v {
            0 => RenderMode::Off,
            1 => RenderMode::Stock,
            2 => RenderMode::Despiked,
            3 => RenderMode::Unsmoothed,
            _ => return None,
        })
    }
}

/// The configured [`RenderMode`], whether the device's own motion goes through it, and whether a
/// profile has armed (§4.14).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct RenderStatus {
    /// The texture motion is drawn with.
    pub mode: RenderMode,
    /// Whether the device's own motion is drawn by the model rather than relayed.
    pub full: bool,
    /// Whether the box has learned a profile for the attached device. Nothing is drawn until it has,
    /// so this separates a box set to a mode from a box drawing with it.
    pub ready: bool,
}

impl RenderStatus {
    /// Decode a `RESP(OPTIONS, RENDER)` payload (§4.14).
    pub(crate) fn from_payload(p: &[u8]) -> Option<RenderStatus> {
        if p.len() < 5 {
            return None;
        }
        Some(RenderStatus {
            mode: RenderMode::from_u8(p[2])?,
            full: p[3] != 0,
            ready: p[4] != 0,
        })
    }
}
