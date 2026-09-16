//! `TRANSFORM` (§3.15) vocabulary: the field operation a host installs, the fields it reads and writes,
//! the signed scale, and the decoded `RESP(TRANSFORMS)` readback.

use crate::protocol::opcode::{
    LOCK_SCALE_MAX, LOCK_SCALE_PASS, TF_F_FULL, TF_REMAP, TF_SCALE, TF_SWAP, TRANSFORM_MAX_ENTRIES,
};
use crate::types::{Axis, Class, LockTarget};

/// The operation a [`Transform`] performs on its fields (§3.15).
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TransformOp {
    /// Move a source field's contribution into a destination, clearing the source.
    Remap = TF_REMAP,
    /// Exchange two axes: read both, then write both, so it is not two remaps.
    Swap = TF_SWAP,
    /// Weigh one axis by the signed scale. `-100` is the negation, exactly.
    Scale = TF_SCALE,
}

impl TransformOp {
    /// The wire `op` byte.
    pub fn as_u8(self) -> u8 {
        self as u8
    }

    /// Map a wire `op` byte to a [`TransformOp`], or `None` for an unknown value.
    pub fn from_u8(v: u8) -> Option<TransformOp> {
        Some(match v {
            TF_REMAP => TransformOp::Remap,
            TF_SWAP => TransformOp::Swap,
            TF_SCALE => TransformOp::Scale,
            _ => return None,
        })
    }

    /// Whether this op admits the given `source`/`dest` pair, mirroring the box's own check. Whether
    /// the fields are declared is the box's to answer.
    pub fn admits(self, source: LockTarget, dest: LockTarget) -> bool {
        use LockTarget::{Axis, Usage};
        match self {
            TransformOp::Scale => matches!((source, dest), (Axis(a), Axis(b)) if a == b),
            TransformOp::Swap => matches!((source, dest), (Axis(a), Axis(b)) if a != b),
            TransformOp::Remap => match (source, dest) {
                (Axis(_), Axis(_)) => true,
                (Usage(s), Usage(d)) => {
                    s.class == Class::Button
                        && matches!(d.class, Class::Button | Class::Key | Class::Media)
                }
                _ => false,
            },
        }
    }
}

/// The `(source, dest)` key that identifies one transform-table entry. Two transforms with this key in
/// common are the same entry: setting the second overwrites the first's op and scale.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TransformKey {
    /// The field the transform reads.
    pub source: LockTarget,
    /// The field the transform writes.
    pub dest: LockTarget,
}

/// One field transform: an [operation](TransformOp), the [`source`](Transform::source) field it reads,
/// the [`dest`](Transform::dest) field it writes, and a signed scale.
///
/// The **signed scale** is a percent carrying a sign: `-100` negates, `100` is identity, `200` doubles,
/// `-50` halves and flips, `0` blocks the source. Its magnitude may not exceed
/// [`LOCK_SCALE_MAX`](crate::LOCK_SCALE_MAX), and the result is clamped to the destination field's
/// declared range. A button carries one bit, so a button source takes only `100`.
///
/// ```
/// # use medius::{Axis, Transform, TransformOp};
/// let inv = Transform::invert(Axis::Y);
/// let fast = Transform::scale_axis(Axis::Wheel, 200);
/// assert_eq!(inv.op, TransformOp::Scale);
/// assert_eq!(inv.scale, -100);
/// assert_eq!(fast.scale, 200);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Transform {
    /// What the transform does to its fields.
    pub op: TransformOp,
    /// The field the transform reads.
    pub source: LockTarget,
    /// The field the transform writes (equal to [`source`](Transform::source) for a scale).
    pub dest: LockTarget,
    /// The signed percent (see the type docs).
    pub scale: i16,
}

impl Transform {
    /// A transform from an explicit op, source, destination and signed scale.
    pub fn new(
        op: TransformOp,
        source: impl Into<LockTarget>,
        dest: impl Into<LockTarget>,
        scale: i16,
    ) -> Transform {
        Transform {
            op,
            source: source.into(),
            dest: dest.into(),
            scale,
        }
    }

    /// Negate an axis: a [`Scale`](TransformOp::Scale) of `-100`, which the box applies exactly.
    pub fn invert(axis: Axis) -> Transform {
        Transform::scale_axis(axis, -(LOCK_SCALE_PASS as i16))
    }

    /// Weigh an axis by a signed percent: `200` doubles, `-50` halves and flips, `0` blocks it.
    pub fn scale_axis(axis: Axis, percent: i16) -> Transform {
        Transform {
            op: TransformOp::Scale,
            source: LockTarget::Axis(axis),
            dest: LockTarget::Axis(axis),
            scale: percent,
        }
    }

    /// Exchange two axes atomically (read both, then write both). Add a
    /// [`with_scale`](Transform::with_scale) to weigh both directions.
    pub fn swap(a: Axis, b: Axis) -> Transform {
        Transform {
            op: TransformOp::Swap,
            source: LockTarget::Axis(a),
            dest: LockTarget::Axis(b),
            scale: LOCK_SCALE_PASS as i16,
        }
    }

    /// Move a source field's contribution into a destination, clearing the source. Same-report for
    /// axis to axis and button to button; a button to key or media remap holds the destination through
    /// its own interface for as long as the button is down.
    pub fn remap(source: impl Into<LockTarget>, dest: impl Into<LockTarget>) -> Transform {
        Transform {
            op: TransformOp::Remap,
            source: source.into(),
            dest: dest.into(),
            scale: LOCK_SCALE_PASS as i16,
        }
    }

    /// Set the signed scale, for a scaled [`swap`](Transform::swap) or axis
    /// [`remap`](Transform::remap).
    pub fn with_scale(mut self, scale: i16) -> Transform {
        self.scale = scale;
        self
    }

    /// The `(source, dest)` key this entry is filed under.
    pub fn key(&self) -> TransformKey {
        TransformKey {
            source: self.source,
            dest: self.dest,
        }
    }

    /// Whether the source names a momentary usage, which carries one bit rather than a magnitude.
    pub(crate) fn source_is_usage(&self) -> bool {
        matches!(self.source, LockTarget::Usage(_))
    }
}

/// The decoded `RESP(TRANSFORMS)` (§4.18): the whole transform table, read back as the commands that
/// rebuild it, in the order the box applies them.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Transforms {
    /// The table is full: a further entry was, or would be, refused.
    pub table_full: bool,
    /// One entry per installed transform, in installation order, which is the order the box applies
    /// them in. Two transforms that write the same field do not commute, so this order is the state.
    pub entries: Vec<Transform>,
}

impl Transforms {
    /// The most entries the box holds.
    pub const CAPACITY: usize = TRANSFORM_MAX_ENTRIES;

    /// Decode a `RESP(TRANSFORMS)` payload (§4.18): `[what][flags u8][n u8]` then `n` ×
    /// `[op u8][sclass u8][sid u16][dclass u8][did u16][scale i16]`, with no `state` byte per entry.
    pub(crate) fn from_payload(p: &[u8]) -> Option<Transforms> {
        if p.len() < 3 {
            return None;
        }
        let table_full = p[1] & TF_F_FULL != 0;
        let n = p[2] as usize;
        let mut entries = Vec::with_capacity(n);
        for i in 0..n {
            let o = 3 + 9 * i;
            let row = p.get(o..o + 9)?;
            // A byte the crate has no enum for (an op or class a newer box added) skips this entry
            // rather than aborting the whole decode: the rest of the table still reads.
            let (Some(op), Some(source), Some(dest)) = (
                TransformOp::from_u8(row[0]),
                LockTarget::from_class_id(row[1], u16::from_le_bytes([row[2], row[3]])),
                LockTarget::from_class_id(row[4], u16::from_le_bytes([row[5], row[6]])),
            ) else {
                continue;
            };
            entries.push(Transform {
                op,
                source,
                dest,
                scale: i16::from_le_bytes([row[7], row[8]]),
            });
        }
        Some(Transforms {
            table_full,
            entries,
        })
    }
}

/// The largest scale magnitude the box applies; a wider one is refused rather than saturated.
pub(crate) const TRANSFORM_SCALE_MAX: i16 = LOCK_SCALE_MAX as i16;
