//! Direction byte shared by `LOCK`, `CLIP` and `CATCH`.

use crate::protocol::opcode::{
    LOCK_DIR_AGAINST, LOCK_DIR_BOTH, LOCK_DIR_NEG, LOCK_DIR_POS, LOCK_DIR_WITH,
};

/// Direction byte carried by `LOCK`, `CLIP` and `CATCH`.
///
/// [`Positive`](Direction::Positive) and [`Negative`](Direction::Negative) name a fixed sign or edge.
/// [`With`](Direction::With) and [`Against`](Direction::Against) name a sign relative to the bearing
/// (the direction the box is injecting), so their sign follows the injection, not the axis; see
/// [`Device::scale`](crate::Device::scale) and [`Device::set_bearing`](crate::Device::set_bearing).
///
/// Variants are named for the axis reading; the constants below alias them for the edge and flow
/// readings. The class selects the reading, and no class carries two.
///
/// | Constant | Same as | Classes |
/// |---|---|---|
/// | [`Direction::PRESS`] | `Positive` | button, key, media |
/// | [`Direction::RELEASE`] | `Negative` | button, key, media |
/// | [`Direction::IN`] | `Positive` | traffic: device to PC |
/// | [`Direction::OUT`] | `Negative` | traffic: PC to device |
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub enum Direction {
    /// Both edges, signs or flows. On a `LOCK` scale, the scale goes to the two fixed signs and a full
    /// pass to the relative pair, since the two multiply and writing both would square it; only an
    /// unlock, a full pass, reaches the relative pair with its own value.
    #[default]
    Both = LOCK_DIR_BOTH,
    /// Press edge, positive sign or IN flow.
    Positive = LOCK_DIR_POS,
    /// Release edge, negative sign or OUT flow.
    Negative = LOCK_DIR_NEG,
    /// Axis sign along the bearing (the injected direction), so it follows the bearing, not the axis;
    /// inert while no bearing is live. Axes only.
    With = LOCK_DIR_WITH,
    /// Axis sign opposing the bearing (the injected direction), so it follows the bearing, not the
    /// axis; inert while no bearing is live. Axes only.
    Against = LOCK_DIR_AGAINST,
}

impl Direction {
    /// A momentary usage going down.
    pub const PRESS: Direction = Direction::Positive;
    /// A momentary usage coming up.
    pub const RELEASE: Direction = Direction::Negative;
    /// Device-to-PC traffic.
    pub const IN: Direction = Direction::Positive;
    /// PC-to-device traffic.
    pub const OUT: Direction = Direction::Negative;

    /// Wire `direction` byte.
    pub fn as_u8(self) -> u8 {
        self as u8
    }

    /// Decodes a wire `direction` byte; `None` if unknown.
    pub fn from_u8(v: u8) -> Option<Direction> {
        Some(match v {
            LOCK_DIR_BOTH => Direction::Both,
            LOCK_DIR_POS => Direction::Positive,
            LOCK_DIR_NEG => Direction::Negative,
            LOCK_DIR_WITH => Direction::With,
            LOCK_DIR_AGAINST => Direction::Against,
            _ => return None,
        })
    }

    /// Whether this direction is relative to the bearing.
    ///
    /// Only the relative-axis class reads one. A catch subscription (addressed before any injection),
    /// a button, key or media lock (no bearing) and a clip packet trigger refuse one with
    /// [`Error::RelativeDirection`](crate::Error::RelativeDirection). A clip input trigger's edge is
    /// the three-variant [`Edge`](crate::Edge).
    pub fn is_relative(self) -> bool {
        matches!(self, Direction::With | Direction::Against)
    }

    /// Whether an event on `other` matches this direction. `Both` on either side matches, as on the
    /// box.
    pub fn admits(self, other: Direction) -> bool {
        self == Direction::Both || other == Direction::Both || self == other
    }

    /// Sign of a delta; `Both` for zero.
    pub fn of_delta(delta: i16) -> Direction {
        match delta {
            d if d > 0 => Direction::Positive,
            d if d < 0 => Direction::Negative,
            _ => Direction::Both,
        }
    }
}
