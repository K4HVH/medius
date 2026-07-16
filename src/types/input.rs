//! `CATCH` event-stream vocabulary (§3.9): the subscription mask, the catch events, and the decoded
//! `RESP(CATCH)`. Bytes are pinned to the firmware wire format in `ctrl_proto.h`.

use crate::protocol::opcode::{
    CATCH_ALL, CATCH_BUTTONS, CATCH_KEYS, CATCH_MASK, CATCH_MEDIA, CATCH_MOTION, CATCH_WHEEL,
};
use crate::types::{Class, Usage};

/// Which classes of physical input the box streams as catch events (§3.9).
///
/// `MOTION`/`WHEEL` yield a [`CatchEvent::Motion`]; `BUTTONS`/`KEYS`/`MEDIA` each yield a
/// [`CatchEvent::Usages`] snapshot. The mask only gates which report changes trigger an emission — so
/// [`CatchMask::BUTTONS`] alone stays sparse even though the mouse reports at roughly 1 kHz. Combine
/// classes with `|`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct CatchMask(u8);

impl CatchMask {
    /// Reports whose X or Y delta is non-zero.
    pub const MOTION: CatchMask = CatchMask(CATCH_MOTION);
    /// Reports whose wheel delta is non-zero.
    pub const WHEEL: CatchMask = CatchMask(CATCH_WHEEL);
    /// A mouse-button edge (press or release).
    pub const BUTTONS: CatchMask = CatchMask(CATCH_BUTTONS);
    /// A keyboard change (modifier or pressed-key set).
    pub const KEYS: CatchMask = CatchMask(CATCH_KEYS);
    /// A media (Consumer) usage change.
    pub const MEDIA: CatchMask = CatchMask(CATCH_MEDIA);

    /// The empty mask (unsubscribe).
    pub const fn empty() -> CatchMask {
        CatchMask(0)
    }

    /// Every class — the full physical-input mirror.
    pub const fn all() -> CatchMask {
        CatchMask(CATCH_ALL)
    }

    /// Build a mask from raw bits, dropping any outside the valid set.
    pub const fn from_bits_truncate(bits: u8) -> CatchMask {
        CatchMask(bits & CATCH_MASK)
    }

    /// The raw mask byte.
    pub const fn bits(self) -> u8 {
        self.0
    }

    /// Whether this mask carries no classes.
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Whether every class in `other` is set in this mask.
    pub const fn contains(self, other: CatchMask) -> bool {
        self.0 & other.0 == other.0
    }

    /// The union of two masks.
    pub const fn union(self, other: CatchMask) -> CatchMask {
        CatchMask(self.0 | other.0)
    }
}

impl core::ops::BitOr for CatchMask {
    type Output = CatchMask;
    fn bitor(self, rhs: CatchMask) -> CatchMask {
        self.union(rhs)
    }
}

impl core::ops::BitOrAssign for CatchMask {
    fn bitor_assign(&mut self, rhs: CatchMask) {
        self.0 |= rhs.0;
    }
}

/// A relative-axis catch event — a `MOTION_EVENT` frame (§4.10). The user's real motion at the merge
/// point, BEFORE any lock suppression or injection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MotionEvent {
    /// Relative X this report (right positive).
    pub dx: i16,
    /// Relative Y this report (down positive).
    pub dy: i16,
    /// Wheel delta this report (up positive).
    pub dz: i16,
}

impl MotionEvent {
    /// Decode a `MOTION_EVENT` payload (§4.10): `[dx i16 LE][dy i16 LE][dz i16 LE]`.
    pub(crate) fn from_payload(p: &[u8]) -> Option<MotionEvent> {
        if p.len() < 6 {
            return None;
        }
        Some(MotionEvent {
            dx: i16::from_le_bytes([p[0], p[1]]),
            dy: i16::from_le_bytes([p[2], p[3]]),
            dz: i16::from_le_bytes([p[4], p[5]]),
        })
    }
}

/// A held-usage snapshot catch event — a `USAGE_EVENT` frame (§4.10). Every held usage of one class
/// (button / key / media; modifiers are key usages `0xE0..=0xE7`), so a mouse-button press and a key
/// press have the same shape. Self-correcting: a dropped frame is recovered by the next one. Diff
/// successive snapshots for press/release edges, or use [`is_held`](Self::is_held) for the current state.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct UsageSnapshot {
    /// The currently-held usages (all of one class per event).
    pub usages: Vec<Usage>,
}

impl UsageSnapshot {
    /// Decode a `USAGE_EVENT` payload (§4.10): `[n u8]` then `n × [class u8][id u16 LE]`.
    pub(crate) fn from_payload(p: &[u8]) -> Option<UsageSnapshot> {
        let n = *p.first()? as usize;
        let mut usages = Vec::with_capacity(n);
        for i in 0..n {
            let off = 1 + 3 * i;
            let class = Class::from_u8(*p.get(off)?)?;
            let id = u16::from_le_bytes([*p.get(off + 1)?, *p.get(off + 2)?]);
            usages.push(Usage::new(class, id));
        }
        Some(UsageSnapshot { usages })
    }

    /// The class of this snapshot's usages (from the first entry), or `None` if empty.
    pub fn class(&self) -> Option<Class> {
        self.usages.first().map(|u| u.class)
    }

    /// Whether `usage` is held in this snapshot.
    pub fn is_held(&self, usage: impl Into<Usage>) -> bool {
        let u = usage.into();
        self.usages.contains(&u)
    }
}

/// One event from the catch stream. Match on the variant: relative motion, or a held-usage snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CatchEvent {
    /// A relative-axis event — cursor motion and/or wheel.
    Motion(MotionEvent),
    /// A held-usage snapshot for one class (buttons, keys, or media).
    Usages(UsageSnapshot),
}

/// Decoded `RESP(CATCH)` (§4.9): the active subscription mask + the firmware-side dropped-event count
/// (events the box could not queue under back-pressure; distinct from host-side
/// [`EventStream::dropped`](crate::EventStream::dropped)).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CatchState {
    /// The classes the box is currently streaming.
    pub mask: CatchMask,
    /// Events the box dropped because its outbound queue was full.
    pub dropped: u32,
}

impl CatchState {
    /// Decode a `RESP(CATCH)` payload (§4.9): `[what][mask u8][dropped u32 LE]`.
    pub(crate) fn from_payload(p: &[u8]) -> Option<CatchState> {
        if p.len() < 6 {
            return None;
        }
        Some(CatchState {
            mask: CatchMask::from_bits_truncate(p[1]),
            dropped: u32::from_le_bytes([p[2], p[3], p[4], p[5]]),
        })
    }
}
