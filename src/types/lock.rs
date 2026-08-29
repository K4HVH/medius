//! `LOCK` control vocabulary (§3.8): what a lock addresses, its edge, blanket groups, and decoded locks.

use crate::protocol::opcode::{
    LOCK_AXIS_WHEEL, LOCK_AXIS_X, LOCK_AXIS_Y, LOCK_CLS_AXIS, LOCK_SCALE_BLOCK, LOCK_SCALE_PASS,
};
use crate::types::{Axis, Class, Direction, Usage};

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

/// The [`Blanket`] groups a `CLIP_LOCK_*` scope byte names (the inverse of [`blanket_scope`]).
pub(crate) fn blanket_from_scope(scope: u8) -> Vec<Blanket> {
    Blanket::ALL
        .iter()
        .copied()
        .filter(|b| scope & b.clip_lock_bit() != 0)
        .collect()
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

/// One entry in a decoded `RESP(LOCKS)` (§4.8): what is weighed, in which direction, and by how much.
///
/// Entries mirror the `LOCK` frame field for field, so what comes back is what you would send to
/// reproduce it. Only directions off [`LOCK_SCALE_PASS`] are reported; a target absent from the reply
/// is passing untouched.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LockEntry {
    /// What this entry weighs.
    pub scope: LockScope,
    /// Which direction of it.
    pub direction: Direction,
    /// Percent of the physical value kept: 0 blocks, 100 passes, above 100 amplifies. A momentary
    /// usage carries one bit, so the box stores the block or pass it amounts to and one never reports a
    /// value in between.
    ///
    /// This is the figure the box applies, not the byte it was sent. In
    /// [`BearingMode::Vector`](crate::BearingMode) one relative scale governs both axes, the lower
    /// of X's and Y's, and both relative entries carry that number.
    pub scale: u8,
}

impl LockEntry {
    /// Whether this entry blocks its direction outright, rather than merely weighing it.
    pub fn is_block(&self) -> bool {
        self.scale == LOCK_SCALE_BLOCK
    }
}

/// Decoded `RESP(LOCKS)` (§4.8): every active lock across every class.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Locks {
    entries: Vec<LockEntry>,
}

impl Locks {
    /// Decode a `RESP(LOCKS)` payload: `[what][n]` then `n × [class][id u16 LE][dir][scale]`; unknown entries skip.
    pub(crate) fn from_payload(p: &[u8]) -> Option<Locks> {
        let n = *p.get(1)? as usize;
        let mut entries = Vec::with_capacity(n);
        for i in 0..n {
            let off = 2 + 5 * i;
            let cls = *p.get(off)?;
            let id = u16::from_le_bytes([*p.get(off + 1)?, *p.get(off + 2)?]);
            let dir = *p.get(off + 3)?;
            let scale = *p.get(off + 4)?;
            let (Some(scope), Some(direction)) = (decode_scope(cls, id), Direction::from_u8(dir))
            else {
                continue;
            };
            entries.push(LockEntry {
                scope,
                direction,
                scale,
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

    /// Whether the given target is blocked outright on the given direction, by a specific entry or a
    /// covering blanket. A direction merely weighed (any scale between block and pass) is not locked.
    ///
    /// [`Direction::Both`] asks about the two absolute signs, the pair it has always named. A relative
    /// direction is asked for by name, because a target can be blocked against the bearing while both
    /// of its fixed signs pass.
    pub fn is_locked(&self, target: impl Into<LockTarget>, dir: Direction) -> bool {
        let target = target.into();
        match dir {
            Direction::Both => {
                self.scale_of(target, Direction::Positive) == LOCK_SCALE_BLOCK
                    && self.scale_of(target, Direction::Negative) == LOCK_SCALE_BLOCK
            }
            d => self.scale_of(target, d) == LOCK_SCALE_BLOCK,
        }
    }

    /// The scale in effect on one target and direction: percent of the physical value kept, so
    /// [`LOCK_SCALE_PASS`] when nothing weighs it.
    ///
    /// [`Direction::Both`] reports the lowest scale across any direction. That is not what a delta
    /// meets: a delta picks up one fixed-direction scale and one bearing-relative one, and the box
    /// multiplies them, so `Negative` 50 with `Against` 40 lands at 20% while this returns 40. Ask by
    /// direction and multiply if you need the figure a delta actually sees.
    ///
    /// A covering blanket counts, and where several entries cover the same direction the lowest wins.
    pub fn scale_of(&self, target: impl Into<LockTarget>, dir: Direction) -> u8 {
        let target = target.into();
        let covers = |e: &LockEntry| match e.scope {
            LockScope::Target(t) => t == target,
            LockScope::Blanket(class) => matches!(target, LockTarget::Usage(u) if u.class == class),
        };
        self.entries
            .iter()
            .filter(|e| covers(e) && (dir == Direction::Both || e.direction.admits(dir)))
            .map(|e| e.scale)
            .min()
            .unwrap_or(LOCK_SCALE_PASS)
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
