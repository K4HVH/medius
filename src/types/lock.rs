//! `LOCK` vocabulary (§3.8): lock targets, edges, blanket groups, decoded locks.

use crate::protocol::opcode::{LOCK_CLS_AXIS, LOCK_SCALE_BLOCK, LOCK_SCALE_PASS};
use crate::types::{Axis, Class, Direction, Usage};

/// Whole-group blanket: the cursor axes (X+Y), the wheel, every mouse button, key or media usage.
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
    /// Every group, for a clip auto-lock over all physical input.
    pub const ALL: &'static [Blanket] = &[
        Blanket::Aim,
        Blanket::Wheel,
        Blanket::Buttons,
        Blanket::Keys,
        Blanket::Media,
    ];

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

pub(crate) fn blanket_scope(scope: &[Blanket]) -> u8 {
    scope.iter().fold(0, |m, b| m | b.clip_lock_bit())
}

pub(crate) fn blanket_from_scope(scope: u8) -> Vec<Blanket> {
    Blanket::ALL
        .iter()
        .copied()
        .filter(|b| scope & b.clip_lock_bit() != 0)
        .collect()
}

/// Addressable input field: a relative axis or a momentary usage (button/key/media).
///
/// The box addresses a field the same way everywhere: a [`lock`](crate::Device::lock) weighs it and
/// a [`Transform`](crate::Transform) reads and writes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LockTarget {
    /// A relative axis (X/Y/wheel/pan), locked by sign.
    Axis(Axis),
    /// A momentary usage (button/key/media), locked by press/release edge.
    Usage(Usage),
}

impl LockTarget {
    /// Wire `(class, id)`.
    pub fn class_id(self) -> (u8, u16) {
        match self {
            LockTarget::Axis(a) => (LOCK_CLS_AXIS, a.as_u16()),
            LockTarget::Usage(u) => u.class_id(),
        }
    }

    /// Decodes a wire `(class, id)`; `None` for a class no field names or an axis id past the
    /// declared axes.
    pub fn from_class_id(class: u8, id: u16) -> Option<LockTarget> {
        if class == LOCK_CLS_AXIS {
            Some(LockTarget::Axis(Axis::from_u16(id)?))
        } else {
            Some(LockTarget::Usage(Usage::new(Class::from_u8(class)?, id)))
        }
    }

    /// The axis, or `None` for a momentary usage.
    pub fn as_axis(self) -> Option<Axis> {
        match self {
            LockTarget::Axis(a) => Some(a),
            LockTarget::Usage(_) => None,
        }
    }
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

/// What a `RESP(LOCKS)` entry addresses: a [`LockTarget`], or a whole-[`Class`] blanket.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LockScope {
    /// One axis or usage.
    Target(LockTarget),
    /// Every button, key or media usage of the class.
    Blanket(Class),
}

/// Decoded `RESP(LOCKS)` entry (§4.8): what is weighed, in which direction, by how much.
///
/// Mirrors the `LOCK` frame field for field, so it replays as sent. Only directions off
/// [`LOCK_SCALE_PASS`] are reported; a target absent from the reply passes untouched.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LockEntry {
    /// What this entry weighs.
    pub scope: LockScope,
    /// Which direction of it.
    pub direction: Direction,
    /// Percent of the physical value kept: 0 blocks, 100 passes, above 100 amplifies, and a negative
    /// one reverses what it keeps. A momentary usage carries one bit, so the box stores the block or
    /// pass it amounts to and never reports a value between.
    ///
    /// The figure the box applies, which can differ from the one sent: in
    /// [`BearingMode::Vector`](crate::BearingMode) one relative scale, the lower of X's and Y's,
    /// governs both axes, and both relative entries carry it.
    pub scale: i16,
}

impl LockEntry {
    /// Whether this entry blocks its direction outright.
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
    /// `[what][n]` then `n × [class][id u16 LE][dir][scale i16 LE]`; unknown entries skip.
    pub(crate) fn from_payload(p: &[u8]) -> Option<Locks> {
        let n = *p.get(1)? as usize;
        let mut entries = Vec::with_capacity(n);
        for i in 0..n {
            let off = 2 + 6 * i;
            let cls = *p.get(off)?;
            let id = u16::from_le_bytes([*p.get(off + 1)?, *p.get(off + 2)?]);
            let dir = *p.get(off + 3)?;
            let scale = i16::from_le_bytes([*p.get(off + 4)?, *p.get(off + 5)?]);
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

    /// [`Locks`] from decoded entries, for tests and `MockBox`.
    pub fn from_entries(entries: Vec<LockEntry>) -> Locks {
        Locks { entries }
    }

    /// Every active lock entry.
    pub fn entries(&self) -> &[LockEntry] {
        &self.entries
    }

    /// Whether `target` is blocked outright on `dir`, by its own entry or a covering blanket. A
    /// direction weighed between block and pass is not locked.
    ///
    /// [`Direction::Both`] asks about the two absolute signs. Ask for a relative direction by name: a
    /// target can be blocked against the bearing while both its fixed signs pass.
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

    /// Scale in effect on one target and direction: percent of the physical value kept;
    /// [`LOCK_SCALE_PASS`] when nothing weighs it.
    ///
    /// [`Direction::Both`] reports the least that survives across any direction, ranked by magnitude
    /// so a block outranks a reversal of any size: `Positive` blocked with `Against` at `-50` reports
    /// `0`. A delta meets something else: one fixed-direction scale times one bearing-relative one,
    /// so `Negative` 50 with `Against` 40 lands at 20% while this returns 40. For that figure, ask by
    /// direction and multiply.
    ///
    /// A covering blanket counts; where several entries cover one direction, the least that survives
    /// wins.
    pub fn scale_of(&self, target: impl Into<LockTarget>, dir: Direction) -> i16 {
        let target = target.into();
        let covers = |e: &LockEntry| match e.scope {
            LockScope::Target(t) => t == target,
            LockScope::Blanket(class) => matches!(target, LockTarget::Usage(u) if u.class == class),
        };
        self.entries
            .iter()
            .filter(|e| covers(e) && (dir == Direction::Both || e.direction.admits(dir)))
            .map(|e| e.scale)
            // By magnitude: a signed minimum ranks -50 below 0 and reports a reversal over the block
            // the delta meets.
            .min_by_key(|s| (s.unsigned_abs(), *s))
            .unwrap_or(LOCK_SCALE_PASS)
    }
}

fn decode_scope(cls: u8, id: u16) -> Option<LockScope> {
    if cls == LOCK_CLS_AXIS {
        Some(LockScope::Target(LockTarget::Axis(Axis::from_u16(id)?)))
    } else if id == crate::protocol::opcode::LOCK_ID_ALL {
        Some(LockScope::Blanket(Class::from_u8(cls)?))
    } else {
        let class = Class::from_u8(cls)?;
        Some(LockScope::Target(LockTarget::Usage(Usage::new(class, id))))
    }
}
