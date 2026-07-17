//! `LOCK` control vocabulary (§3.8): what a lock addresses, its edge, blanket groups, and decoded locks.

use crate::protocol::opcode::{
    LOCK_AXIS_WHEEL, LOCK_AXIS_X, LOCK_AXIS_Y, LOCK_CLS_AXIS, LOCK_DIR_BOTH, LOCK_DIR_NEG,
    LOCK_DIR_POS, LOCK_DIRBIT_NEG, LOCK_DIRBIT_POS,
};
use crate::types::{Axis, Class, Usage};

/// A whole-group blanket: the cursor aim (X+Y), the wheel, every mouse button, every key, or every media usage.
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
    /// Every input group, for a clip auto-lock over all physical input.
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

/// Which edge a `LOCK` covers: press/release for a usage, `+`/`-` sign for an axis.
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

/// What a lock addresses: a relative axis or a momentary usage (button/key/media).
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

/// What a `RESP(LOCKS)` entry addresses: a specific [`LockTarget`], or a whole-class blanket over a [`Class`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LockScope {
    /// A specific axis or usage.
    Target(LockTarget),
    /// A whole-class blanket (every button, key, or media usage of the class).
    Blanket(Class),
}

/// One entry in a decoded `RESP(LOCKS)` (§4.8): what is locked and which edges are locked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LockEntry {
    /// What this entry locks.
    pub scope: LockScope,
    /// The positive/press edge is locked.
    pub positive: bool,
    /// The negative/release edge is locked.
    pub negative: bool,
}

/// Decoded `RESP(LOCKS)` (§4.8): every active lock across every class.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Locks {
    entries: Vec<LockEntry>,
}

impl Locks {
    /// Decode a `RESP(LOCKS)` payload: `[what][n]` then `n × [class][id u16 LE][dirbits]`; unknown entries skip.
    pub(crate) fn from_payload(p: &[u8]) -> Option<Locks> {
        let n = *p.get(1)? as usize;
        let mut entries = Vec::with_capacity(n);
        for i in 0..n {
            let off = 2 + 4 * i;
            let cls = *p.get(off)?;
            let id = u16::from_le_bytes([*p.get(off + 1)?, *p.get(off + 2)?]);
            let db = *p.get(off + 3)?;
            let Some(scope) = decode_scope(cls, id) else {
                continue;
            };
            entries.push(LockEntry {
                scope,
                positive: db & LOCK_DIRBIT_POS != 0,
                negative: db & LOCK_DIRBIT_NEG != 0,
            });
        }
        Some(Locks { entries })
    }

    /// Build a [`Locks`] from decoded entries; useful for tests and [`MockBox`](crate::MockBox).
    pub fn from_entries(entries: Vec<LockEntry>) -> Locks {
        Locks { entries }
    }

    /// Every active lock entry.
    pub fn entries(&self) -> &[LockEntry] {
        &self.entries
    }

    /// Whether the given target is locked on the given edge, by a specific entry or a covering blanket.
    pub fn is_locked(&self, target: impl Into<LockTarget>, dir: LockDirection) -> bool {
        let target = target.into();
        self.entries.iter().any(|e| {
            let covers = match e.scope {
                LockScope::Target(t) => t == target,
                LockScope::Blanket(class) => {
                    matches!(target, LockTarget::Usage(u) if u.class == class)
                }
            };
            covers
                && match dir {
                    LockDirection::Both => e.positive && e.negative,
                    LockDirection::Positive => e.positive,
                    LockDirection::Negative => e.negative,
                }
        })
    }
}

fn decode_scope(cls: u8, id: u16) -> Option<LockScope> {
    if cls == LOCK_CLS_AXIS {
        let axis = match id {
            LOCK_AXIS_X => Axis::X,
            LOCK_AXIS_Y => Axis::Y,
            LOCK_AXIS_WHEEL => Axis::Wheel,
            _ => return None,
        };
        Some(LockScope::Target(LockTarget::Axis(axis)))
    } else if id == crate::protocol::opcode::LOCK_ID_ALL {
        Some(LockScope::Blanket(Class::from_u8(cls)?))
    } else {
        let class = Class::from_u8(cls)?;
        Some(LockScope::Target(LockTarget::Usage(Usage::new(class, id))))
    }
}
