//! The bearing: what [`Direction::With`](crate::Direction) and [`Direction::Against`](crate::Direction) are measured against (§3.12).

use std::time::Duration;

use crate::protocol::opcode::{BEARING_PER_AXIS, BEARING_VECTOR};

/// How the box reads whether physical motion runs with or against its own injection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum BearingMode {
    /// Each axis compares its own sign against its own bearing, independently (the default).
    #[default]
    PerAxis,
    /// The physical delta is projected onto the injected XY vector: only the part of it lying
    /// along the injection is weighed, and the part across it passes untouched. Smoother on
    /// diagonals; X and Y stop being independent.
    ///
    /// One relative scale governs both axes, the lower of X's and Y's, so setting only X's
    /// weighs Y's motion too. `RESP(LOCKS)` reports that effective number on both relative entries.
    ///
    /// The projection moves motion between the axes, and each axis's absolute scale then applies to
    /// what the projection left rather than to the sign the report carried: an absolute scale is a
    /// statement about what reaches the PC, so a block covers motion the projection put there.
    Vector,
}

impl BearingMode {
    /// The wire `mode` byte.
    pub fn as_u8(self) -> u8 {
        match self {
            BearingMode::PerAxis => BEARING_PER_AXIS,
            BearingMode::Vector => BEARING_VECTOR,
        }
    }

    /// Map a wire `mode` byte to a [`BearingMode`], or `None` for an unknown value.
    pub fn from_u8(v: u8) -> Option<BearingMode> {
        Some(match v {
            BEARING_PER_AXIS => BearingMode::PerAxis,
            BEARING_VECTOR => BearingMode::Vector,
            _ => return None,
        })
    }
}

/// The configured bearing (`RESP(OPTIONS, BEARING)`, §4.14).
///
/// [`Default`] is what a box boots with, so a [`MockBox`](crate::MockBox) replies as real
/// hardware would: [`BEARING_WINDOW_DEFAULT`](crate::BEARING_WINDOW_DEFAULT) in
/// [`BearingMode::PerAxis`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Bearing {
    /// How long the direction of the last injected delta stays the thing
    /// [`Direction::With`](crate::Direction) and [`Direction::Against`](crate::Direction) are measured
    /// against. `None` = never, so the relative directions are inert whatever their scale. Whole
    /// milliseconds on the wire, so this reads back rounded.
    pub window: Option<Duration>,
    /// Whether the bearing is read per axis or as one vector.
    pub mode: BearingMode,
}

impl Default for Bearing {
    fn default() -> Bearing {
        Bearing {
            window: Some(crate::BEARING_WINDOW_DEFAULT),
            mode: BearingMode::PerAxis,
        }
    }
}

impl Bearing {
    /// Decode a `RESP(OPTIONS, BEARING)` payload (§4.14): `[what][id][window u16 LE][mode]`.
    pub(crate) fn from_payload(p: &[u8]) -> Option<Bearing> {
        if p.len() < 5 {
            return None;
        }
        let ms = u16::from_le_bytes([p[2], p[3]]);
        Some(Bearing {
            window: (ms != 0).then(|| Duration::from_millis(ms as u64)),
            mode: BearingMode::from_u8(p[4])?,
        })
    }

    /// Whether a bearing is held at all; the relative directions do nothing when it is not.
    pub fn is_live(&self) -> bool {
        self.window.is_some()
    }
}
