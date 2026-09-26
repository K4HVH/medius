//! The bearing [`Direction::With`](crate::Direction) and [`Direction::Against`](crate::Direction)
//! are measured against (§3.12).

use std::time::Duration;

use crate::protocol::opcode::{BEARING_PER_AXIS, BEARING_VECTOR};

/// How the box reads whether physical motion runs with or against its injection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum BearingMode {
    /// Each axis compares its sign against its own bearing (the default).
    #[default]
    PerAxis,
    /// The physical delta is projected onto the injected XY vector: only the part along the
    /// injection is weighed, and the part across it passes untouched. Smoother on diagonals; X and
    /// Y stop being independent.
    ///
    /// One relative scale, the lower of X's and Y's, governs both axes, so setting only X's weighs
    /// Y's motion too. `RESP(LOCKS)` reports it on both relative entries.
    ///
    /// The projection moves motion between the axes, and each axis's absolute scale then applies to
    /// what the projection left, not the sign the report carried: an absolute scale governs what
    /// reaches the PC, so a block covers motion the projection put there.
    Vector,
}

impl BearingMode {
    /// Wire `mode` byte.
    pub fn as_u8(self) -> u8 {
        match self {
            BearingMode::PerAxis => BEARING_PER_AXIS,
            BearingMode::Vector => BEARING_VECTOR,
        }
    }

    /// Decodes a wire `mode` byte; `None` if unknown.
    pub fn from_u8(v: u8) -> Option<BearingMode> {
        Some(match v {
            BEARING_PER_AXIS => BearingMode::PerAxis,
            BEARING_VECTOR => BearingMode::Vector,
            _ => return None,
        })
    }
}

/// Configured bearing (`RESP(OPTIONS, BEARING)`, §4.14).
///
/// [`Default`] is the boot state, so a `MockBox` replies as hardware does:
/// [`BEARING_WINDOW_DEFAULT`](crate::BEARING_WINDOW_DEFAULT) in [`BearingMode::PerAxis`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Bearing {
    /// How long the last injected delta's direction stays the reference for
    /// [`Direction::With`](crate::Direction) and [`Direction::Against`](crate::Direction). `None` =
    /// never, so the relative directions are inert whatever their scale. Whole milliseconds on the
    /// wire, so it reads back rounded.
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
    /// `[what][id][window u16 LE][mode]` (§4.14).
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

    /// Whether a bearing is held; without one the relative directions do nothing.
    pub fn is_live(&self) -> bool {
        self.window.is_some()
    }
}
