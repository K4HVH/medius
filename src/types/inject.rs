//! Relative-axis drive for the field-generic injection verbs: `move` drives a [`Motion`], `inject` sets a momentary [`Usage`](crate::Usage).

use crate::protocol::opcode::{MV_F_DISCARD, MV_F_FLUSH, MV_F_NOW};

/// A relative axis to drive with the [`move_axis`](crate::Device::move_axis) verb.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Motion {
    Cursor { dx: i16, dy: i16 },
    Wheel(i16),
}

/// Whether a delta obeys [`set_movement_riding`](crate::Device::set_movement_riding) or bypasses it,
/// the `NOW` bit of the `MOVE` flags byte (§3.1).
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum MoveTiming {
    /// Follow the movement-riding option: with it on, wait for a native cursor-motion report to carry
    /// this delta. With it off (the box default) nothing is held, so this emits on the box's own clock.
    #[default]
    Ride = 0,
    /// Emit on the box's own clock whatever movement riding is set to.
    Now = MV_F_NOW,
}

/// What a move does to the motion the box is already holding for a ride, the `FLUSH` and `DISCARD`
/// bits of the `MOVE` flags byte (§3.1).
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
    /// The bit this timing contributes to the wire `flags` byte.
    pub fn as_u8(self) -> u8 {
        self as u8
    }

    /// Map a wire `flags` byte's `NOW` bit to a [`MoveTiming`].
    pub fn from_flags(flags: u8) -> MoveTiming {
        if flags & MV_F_NOW != 0 {
            MoveTiming::Now
        } else {
            MoveTiming::Ride
        }
    }
}

impl PendingMotion {
    /// The bits this contributes to the wire `flags` byte.
    pub fn as_u8(self) -> u8 {
        self as u8
    }

    /// Map a wire `flags` byte to a [`PendingMotion`], or `None` if it sets both `FLUSH` and
    /// `DISCARD`, which contradict each other and which the box refuses outright.
    pub fn from_flags(flags: u8) -> Option<PendingMotion> {
        Some(match flags & (MV_F_FLUSH | MV_F_DISCARD) {
            0 => PendingMotion::Keep,
            MV_F_FLUSH => PendingMotion::Flush,
            MV_F_DISCARD => PendingMotion::Discard,
            _ => return None,
        })
    }
}
