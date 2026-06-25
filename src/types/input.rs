//! `CATCH` event-stream vocabulary (§3.9): the subscription mask, the per-report snapshot, and the
//! decoded `RESP(CATCH)`. Bytes are pinned to the firmware wire format in `ctrl_proto.h`.

use crate::protocol::opcode::{
    CATCH_ALL, CATCH_BUTTONS, CATCH_KEYS, CATCH_MASK, CATCH_MOTION, CATCH_WHEEL,
};
use crate::types::{Button, KeyboardEvent, MediaEvent};

/// Which classes of physical input the box streams as `EVENT` frames (§3.9).
///
/// The event payload is always the full snapshot ([`MouseEvent`]); the mask only gates which report
/// changes trigger an emission — so [`CatchMask::BUTTONS`] alone stays sparse even though the mouse
/// reports at roughly 1 kHz. Combine classes with `|`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct CatchMask(u8);

impl CatchMask {
    /// Reports whose X or Y delta is non-zero.
    pub const MOTION: CatchMask = CatchMask(CATCH_MOTION);
    /// Reports whose wheel delta is non-zero.
    pub const WHEEL: CatchMask = CatchMask(CATCH_WHEEL);
    /// Reports with a button edge (press or release).
    pub const BUTTONS: CatchMask = CatchMask(CATCH_BUTTONS);
    /// Keyboard and media changes — yields [`CatchEvent::Keyboard`] and [`CatchEvent::Media`].
    pub const KEYS: CatchMask = CatchMask(CATCH_KEYS);

    /// The empty mask (unsubscribe).
    pub const fn empty() -> CatchMask {
        CatchMask(0)
    }

    /// Every class — the full physical-input mirror.
    pub const fn all() -> CatchMask {
        CatchMask(CATCH_ALL)
    }

    /// Build a mask from raw bits, dropping any outside the valid set (motion / wheel / buttons / keys).
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

/// One physical-input snapshot from the `CATCH` stream — an `EVENT` frame payload (§3.9).
///
/// It mirrors the user's real mouse report at the merge point, BEFORE any lock suppression or
/// injection, so a locked or injected target still reports the genuine hand input here. Each field is
/// this report's value; diff [`buttons`](Self::buttons) across two reports to detect press/release
/// edges, or use [`is_pressed`](Self::is_pressed) for the current state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MouseEvent {
    /// Pressed-button bitmask: bit `b.as_id()` set means button `b` is held (`Left`=0 .. `Side2`=4).
    pub buttons: u8,
    /// Relative X this report (right positive).
    pub dx: i16,
    /// Relative Y this report (down positive).
    pub dy: i16,
    /// Wheel delta this report (up positive).
    pub wheel: i16,
}

impl MouseEvent {
    /// Decode an `EVENT` payload (§4.10): `[buttons u8][dx i16 LE][dy i16 LE][wheel i16 LE]`.
    pub(crate) fn from_payload(p: &[u8]) -> Option<MouseEvent> {
        if p.len() < 7 {
            return None;
        }
        Some(MouseEvent {
            buttons: p[0],
            dx: i16::from_le_bytes([p[1], p[2]]),
            dy: i16::from_le_bytes([p[3], p[4]]),
            wheel: i16::from_le_bytes([p[5], p[6]]),
        })
    }

    /// Whether `button` is held in this snapshot.
    pub fn is_pressed(self, button: Button) -> bool {
        self.buttons & (1 << button.as_id()) != 0
    }
}

/// One event from the catch stream. The class is set by the subscription mask: a [`CatchMask`] that
/// covers several classes yields a single heterogeneous stream, so match on the variant. Mouse classes
/// (motion/wheel/buttons) arrive as [`CatchEvent::Mouse`]; the [`CatchMask::KEYS`] class arrives as
/// [`CatchEvent::Keyboard`] (typing) and [`CatchEvent::Media`] (media keys).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CatchEvent {
    /// A mouse report — buttons + relative motion + wheel.
    Mouse(MouseEvent),
    /// A keyboard snapshot — the modifier bitmap + currently-pressed keys.
    Keyboard(KeyboardEvent),
    /// A media snapshot — the currently-active Consumer usages.
    Media(MediaEvent),
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
