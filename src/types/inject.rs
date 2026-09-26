//! Relative-axis drive for the injection verbs: `move` drives a [`Motion`], `inject` sets a
//! momentary [`Usage`](crate::Usage).

use crate::protocol::opcode::{MV_F_DISCARD, MV_F_FLUSH, MV_F_NOW};

/// Relative axis for [`move_axis`](crate::Device::move_axis).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Motion {
    /// X and Y cursor axes together.
    Cursor { dx: i16, dy: i16 },
    /// Wheel (vertical scroll).
    Wheel(i16),
    /// AC Pan (horizontal scroll), a full peer of the wheel.
    Pan(i16),
}

/// Whether a delta obeys [`set_movement_riding`](crate::Device::set_movement_riding) or bypasses
/// it: the `NOW` bit of the `MOVE` flags byte (§3.1).
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum MoveTiming {
    /// Follow the movement-riding option: with it on, wait for a native cursor-motion report to carry
    /// this delta; with it off (the box default), nothing waits.
    #[default]
    Ride = 0,
    /// Leave on the next mouse report the box sends, native or its own, whatever the riding option.
    Now = MV_F_NOW,
}

/// What a move does to motion the box already holds for a ride: the `FLUSH` and `DISCARD` bits of
/// the `MOVE` flags byte (§3.1).
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum PendingMotion {
    /// Leave it held.
    #[default]
    Keep = 0,
    /// Emit it now, ignoring the ride window.
    Flush = MV_F_FLUSH,
    /// Drop it. Motion sent with [`MoveTiming::Now`] is untouched.
    Discard = MV_F_DISCARD,
}

impl MoveTiming {
    /// Bit contributed to the wire `flags` byte.
    pub fn as_u8(self) -> u8 {
        self as u8
    }

    /// Decodes a wire `flags` byte's `NOW` bit.
    pub fn from_flags(flags: u8) -> MoveTiming {
        if flags & MV_F_NOW != 0 {
            MoveTiming::Now
        } else {
            MoveTiming::Ride
        }
    }
}

impl PendingMotion {
    /// Bits contributed to the wire `flags` byte.
    pub fn as_u8(self) -> u8 {
        self as u8
    }

    /// Decodes a wire `flags` byte; `None` if it sets both `FLUSH` and `DISCARD`, which the box
    /// refuses as contradictory.
    pub fn from_flags(flags: u8) -> Option<PendingMotion> {
        Some(match flags & (MV_F_FLUSH | MV_F_DISCARD) {
            0 => PendingMotion::Keep,
            MV_F_FLUSH => PendingMotion::Flush,
            MV_F_DISCARD => PendingMotion::Discard,
            _ => return None,
        })
    }
}
