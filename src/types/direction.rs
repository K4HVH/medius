//! The protocol's direction byte, shared by `LOCK`, `CLIP` and `CATCH`.

use crate::protocol::opcode::{
    LOCK_DIR_AGAINST, LOCK_DIR_BOTH, LOCK_DIR_NEG, LOCK_DIR_POS, LOCK_DIR_WITH,
};

/// Which way, on the one byte `LOCK`, `CLIP` and `CATCH` all carry.
///
/// [`Positive`](Direction::Positive) and [`Negative`](Direction::Negative) name a fixed sign or edge.
/// [`With`](Direction::With) and [`Against`](Direction::Against) name a sign relative to the bearing,
/// the direction the box is currently injecting, so they follow the aim instead of the axis; see
/// [`Device::scale`](crate::Device::scale) and [`Device::set_bearing`](crate::Device::set_bearing).
///
/// The variants are named for the axis reading; the other two are the same values under names that
/// read at the call site. Which applies is decided by the class, and no class carries two.
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
    /// Every direction of the target: both edges, both signs, both flows, and both relative senses.
    ///
    /// On a `LOCK` scale it writes the scale to the two fixed signs and a full pass to the relative
    /// pair, since the two multiply and writing both would square the number; only an unlock, a full
    /// pass, reaches the relative pair with its own value.
    #[default]
    Both = LOCK_DIR_BOTH,
    /// The press edge, the positive sign, or the IN flow.
    Positive = LOCK_DIR_POS,
    /// The release edge, the negative sign, or the OUT flow.
    Negative = LOCK_DIR_NEG,
    /// An axis sign pointing the way the box is injecting. Relative to the bearing, so it follows the
    /// aim rather than the axis; inert while no bearing is live. Axes only.
    With = LOCK_DIR_WITH,
    /// An axis sign opposing the way the box is injecting. Relative to the bearing, so it follows the
    /// aim rather than the axis; inert while no bearing is live. Axes only.
    Against = LOCK_DIR_AGAINST,
}

impl Direction {
    /// A momentary usage going down.
    pub const PRESS: Direction = Direction::Positive;
    /// A momentary usage coming up.
    pub const RELEASE: Direction = Direction::Negative;
    /// Traffic from the device to the PC.
    pub const IN: Direction = Direction::Positive;
    /// Traffic from the PC to the device.
    pub const OUT: Direction = Direction::Negative;

    /// The wire `direction` byte.
    pub fn as_u8(self) -> u8 {
        self as u8
    }

    /// Map a wire `direction` byte to a [`Direction`], or `None` if unknown.
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

    /// Whether this direction is measured against the bearing rather than a fixed sign.
    ///
    /// Only the relative-axis class reads one. A catch subscription is addressed before there is any
    /// injection to be with or against, so it refuses one with
    /// [`Error::RelativeDirection`](crate::Error::RelativeDirection). A clip trigger cannot express one
    /// at all: its edge is the separate three-variant [`Edge`](crate::Edge).
    pub fn is_relative(self) -> bool {
        matches!(self, Direction::With | Direction::Against)
    }

    /// Whether an event on `other` is one this direction asked for. Either side naming `Both`
    /// matches, as the box's own resolution does.
    pub fn admits(self, other: Direction) -> bool {
        self == Direction::Both || other == Direction::Both || self == other
    }

    /// The direction a delta moved in; `Both` for zero.
    pub fn of_delta(delta: i16) -> Direction {
        match delta {
            d if d > 0 => Direction::Positive,
            d if d < 0 => Direction::Negative,
            _ => Direction::Both,
        }
    }
}
