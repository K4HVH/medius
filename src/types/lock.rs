//! `LOCK` control vocabulary (§3.8).

use crate::types::Button;

/// What a `LOCK` command targets; the wire `target` byte is `as_u8()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LockTarget {
    X,
    Y,
    Wheel,
    Button(Button),
}

impl LockTarget {
    /// The wire `target` byte (X=0, Y=1, Wheel=2, Button = 3 + the button id).
    pub fn as_u8(self) -> u8 {
        match self {
            LockTarget::X => 0,
            LockTarget::Y => 1,
            LockTarget::Wheel => 2,
            LockTarget::Button(b) => 3 + b.as_id(),
        }
    }

    /// Map a wire `target` byte to a [`LockTarget`], or `None` if unknown.
    pub fn from_u8(v: u8) -> Option<Self> {
        Some(match v {
            0 => LockTarget::X,
            1 => LockTarget::Y,
            2 => LockTarget::Wheel,
            3..=7 => LockTarget::Button(Button::from_id(v - 3)?),
            _ => return None,
        })
    }
}

/// Which edge a `LOCK` covers; for buttons `Positive` is the press edge and `Negative` the release.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LockDirection {
    Both = 0,
    Positive = 1,
    Negative = 2,
}

impl LockDirection {
    /// The wire `direction` byte.
    pub fn as_u8(self) -> u8 {
        self as u8
    }

    /// Map a wire `direction` byte to a [`LockDirection`], or `None` if unknown.
    pub fn from_u8(v: u8) -> Option<Self> {
        Some(match v {
            0 => LockDirection::Both,
            1 => LockDirection::Positive,
            2 => LockDirection::Negative,
            _ => return None,
        })
    }
}

/// Decoded `RESP(LOCKS)` — the lock bitmask, 2 bits per target (positive/press, negative/release).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Locks {
    mask: u16,
}

impl Locks {
    /// Decode a `RESP(LOCKS)` payload (§4.8): `[what][mask u16 LE]`.
    pub(crate) fn from_payload(p: &[u8]) -> Option<Self> {
        if p.len() < 3 {
            return None;
        }
        Some(Locks {
            mask: u16::from_le_bytes([p[1], p[2]]),
        })
    }

    /// The raw lock bitmask: bit `target*2` is positive/press, bit `target*2+1` is negative/release.
    pub fn mask(&self) -> u16 {
        self.mask
    }

    /// Whether the given target/direction is locked (`Both` requires both edges locked).
    pub fn is_locked(&self, target: LockTarget, dir: LockDirection) -> bool {
        let base = target.as_u8() * 2;
        let pos = self.mask & (1 << base) != 0;
        let neg = self.mask & (1 << (base + 1)) != 0;
        match dir {
            LockDirection::Both => pos && neg,
            LockDirection::Positive => pos,
            LockDirection::Negative => neg,
        }
    }
}
