//! Render texture, and whether native motion is rendered (§4.14).

/// Texture the box renders motion with (`OPTION(RENDER)`'s `mode`). [`Off`](RenderMode::Off) is the
/// paced fill; the others render the learned native report texture and differ only in the path
/// smoother. Independent of [`EmitPace`](crate::EmitPace), which caps the rendered rate.
///
/// The model is [ABCurves](https://github.com/optima-manent/ABCurves) (MIT).
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum RenderMode {
    /// Paced fill, renderer off.
    Off = 0,
    /// Rendered with the bit-exact triangular smoother.
    Stock = 1,
    /// Rendered with the smoother's onset ramped, not stepped (factory default).
    #[default]
    Despiked = 2,
    /// Rendered with no smoother; the model receives raw injection.
    Unsmoothed = 3,
}

impl RenderMode {
    /// Wire `mode` byte.
    pub(crate) fn to_wire(self) -> u8 {
        self as u8
    }

    /// Decodes a wire `mode` byte; `None` if unknown.
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

/// Configured [`RenderMode`], whether native motion is rendered, and whether a profile has
/// armed (§4.14).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct RenderStatus {
    /// Render texture.
    pub mode: RenderMode,
    /// Whether the model renders native motion instead of relaying it.
    pub full: bool,
    /// Whether the box has learned a profile for the real device. Until then a box set to any mode
    /// renders nothing.
    pub ready: bool,
}

impl RenderStatus {
    /// Decodes a `RESP(OPTIONS, RENDER)` payload (§4.14).
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
