//! `LOCK` control vocabulary (§3.8): what a lock addresses, its edge, blanket groups, and the decoded
//! `RESP(LOCKS)` list.

use crate::protocol::opcode::{
    LOCK_AXIS_WHEEL, LOCK_AXIS_X, LOCK_AXIS_Y, LOCK_CLS_AXIS, LOCK_DIRBIT_NEG, LOCK_DIRBIT_POS,
    LOCK_DIR_BOTH, LOCK_DIR_NEG, LOCK_DIR_POS,
};
use crate::types::{Axis, Class, Usage};

/// A whole-group blanket: the cursor aim (X+Y), the wheel, every mouse button, every key, or every media
/// usage. Used by [`lock_all`](crate::Device::lock_all) and as the clip
/// [`ClipConfig::autolock`](crate::ClipConfig::autolock) scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Blanket {
    /// The X and Y cursor axes.
    Aim,
    /// The wheel.
    Wheel,
    /// Every mouse button.
    Buttons,
    /// Every keyboard key and modifier.
    Keys,
    /// Every media (Consumer) usage.
    Media,
}

impl Blanket {
    /// Every input group, for a clip auto-lock over all physical input:
    /// [`ClipConfig::new()`](crate::ClipConfig::new)`.autolock(Blanket::ALL)`.
    pub const ALL: &'static [Blanket] = &[
        Blanket::Aim,
        Blanket::Wheel,
        Blanket::Buttons,
        Blanket::Keys,
        Blanket::Media,
    ];

    /// This group's clip auto-lock scope bit (`CLIP_LOCK_*`).
    pub(crate) fn clip_lock_bit(self) -> u8 {
        use crate::protocol::opcode::*;
        match self {
            Blanket::Aim => CLIP_LOCK_AIM,
            Blanket::Wheel => CLIP_LOCK_WHEEL,
            Blanket::Buttons => CLIP_LOCK_BUTTONS,
            Blanket::Keys => CLIP_LOCK_KEYS,
            Blanket::Media => CLIP_LOCK_MEDIA,
        }
    }
}

/// The autolock scope byte for a set of [`Blanket`] groups (an empty set = no autolock).
pub(crate) fn blanket_scope(scope: &[Blanket]) -> u8 {
    scope.iter().fold(0, |m, b| m | b.clip_lock_bit())
}

/// Which edge a `LOCK` covers; for a usage `Positive` is the press edge and `Negative` the release; for an
/// axis they are the `+`/`-` sign.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LockDirection {
    Both = LOCK_DIR_BOTH,
    Positive = LOCK_DIR_POS,
    Negative = LOCK_DIR_NEG,
}

impl LockDirection {
    /// The wire `direction` byte.
    pub fn as_u8(self) -> u8 {
        self as u8
    }

    /// Map a wire `direction` byte to a [`LockDirection`], or `None` if unknown.
    pub fn from_u8(v: u8) -> Option<Self> {
        Some(match v {
            LOCK_DIR_BOTH => LockDirection::Both,
            LOCK_DIR_POS => LockDirection::Positive,
            LOCK_DIR_NEG => LockDirection::Negative,
            _ => return None,
        })
    }
}

/// What a lock addresses: a relative axis or a momentary usage (button/key/media). Both `INJECT` and `LOCK`
/// speak this one vocabulary, so a button is locked exactly like a key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LockTarget {
    /// A relative axis (X/Y/wheel), locked by sign.
    Axis(Axis),
    /// A momentary usage (button/key/media), locked by press/release edge.
    Usage(Usage),
}

impl From<Axis> for LockTarget {
    fn from(a: Axis) -> LockTarget {
        LockTarget::Axis(a)
    }
}
impl<T: Into<Usage>> From<T> for LockTarget {
    fn from(u: T) -> LockTarget {
        LockTarget::Usage(u.into())
    }
}

/// One entry in a decoded `RESP(LOCKS)` (§4.8): the locked target and which edges are locked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LockEntry {
    /// What is locked. `None` = a whole-class blanket (see [`blanket`](Self::blanket)).
    pub target: Option<LockTarget>,
    /// The blanket class, when this entry is a whole-class lock (`id == 0xFFFF` on the wire).
    pub blanket: Option<Class>,
    /// The positive/press edge is locked.
    pub positive: bool,
    /// The negative/release edge is locked.
    pub negative: bool,
}

/// Decoded `RESP(LOCKS)` (§4.8) — every active lock across every class, so keyboard and media locks are
/// reported the same as mouse ones.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Locks {
    entries: Vec<LockEntry>,
}

impl Locks {
    /// Decode a `RESP(LOCKS)` payload: `[what][n u8]` then `n × [class u8][id u16 LE][dirbits u8]`.
    pub(crate) fn from_payload(p: &[u8]) -> Option<Locks> {
        let n = *p.get(1)? as usize;
        let mut entries = Vec::with_capacity(n);
        for i in 0..n {
            let off = 2 + 4 * i;
            let cls = *p.get(off)?;
            let id = u16::from_le_bytes([*p.get(off + 1)?, *p.get(off + 2)?]);
            let db = *p.get(off + 3)?;
            let positive = db & LOCK_DIRBIT_POS != 0;
            let negative = db & LOCK_DIRBIT_NEG != 0;
            let (target, blanket) = decode_target(cls, id);
            entries.push(LockEntry {
                target,
                blanket,
                positive,
                negative,
            });
        }
        Some(Locks { entries })
    }

    /// Build a [`Locks`] from decoded entries; useful for tests and for configuring a
    /// [`MockBox`](crate::MockBox).
    pub fn from_entries(entries: Vec<LockEntry>) -> Locks {
        Locks { entries }
    }

    /// Every active lock entry.
    pub fn entries(&self) -> &[LockEntry] {
        &self.entries
    }

    /// Whether the given target is locked on the given edge.
    pub fn is_locked(&self, target: impl Into<LockTarget>, dir: LockDirection) -> bool {
        let target = target.into();
        self.entries.iter().any(|e| {
            e.target == Some(target)
                && match dir {
                    LockDirection::Both => e.positive && e.negative,
                    LockDirection::Positive => e.positive,
                    LockDirection::Negative => e.negative,
                }
        })
    }
}

fn decode_target(cls: u8, id: u16) -> (Option<LockTarget>, Option<Class>) {
    if cls == LOCK_CLS_AXIS {
        let axis = match id {
            LOCK_AXIS_X => Some(Axis::X),
            LOCK_AXIS_Y => Some(Axis::Y),
            LOCK_AXIS_WHEEL => Some(Axis::Wheel),
            _ => None,
        };
        (axis.map(LockTarget::Axis), None)
    } else if id == crate::protocol::opcode::LOCK_ID_ALL {
        (None, Class::from_u8(cls))
    } else {
        let target = Class::from_u8(cls).map(|class| LockTarget::Usage(Usage::new(class, id)));
        (target, None)
    }
}
