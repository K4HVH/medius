//! The protocol's direction byte, shared by `LOCK`, `CLIP` and `CATCH`.

use crate::protocol::opcode::{LOCK_DIR_BOTH, LOCK_DIR_NEG, LOCK_DIR_POS};

/// Which way, on the one byte `LOCK`, `CLIP` and `CATCH` all carry.
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
    /// Both edges, both signs, or both flows.
    #[default]
    Both = LOCK_DIR_BOTH,
    /// The press edge, the positive sign, or the IN flow.
    Positive = LOCK_DIR_POS,
    /// The release edge, the negative sign, or the OUT flow.
    Negative = LOCK_DIR_NEG,
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
            _ => return None,
        })
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
