//! `TRANSFORM` (§3.15) vocabulary: the field operation a host installs, and the decoded
//! `RESP(TRANSFORMS)` readback.

use crate::protocol::opcode::{TF_F_FULL, TF_OP_COUNT, TF_REMAP, TF_SWAP, TRANSFORM_MAX_ENTRIES};
use crate::types::{Class, LockTarget};

/// The operation a [`Transform`] performs on its fields (§3.15).
///
/// Both MOVE a value. Weighing one, in either direction, is [`scale`](crate::Device::scale)'s: its
/// percent is signed, so `-100` there is the inversion and `0` the block.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TransformOp {
    /// Move a source field's value into a destination, clearing the source.
    Remap = TF_REMAP,
    /// Exchange two axes: read both, then write both, so it is not two remaps.
    Swap = TF_SWAP,
}

// The op space is dense and `Swap` is the top of it, so the count has to be one past it. A box op
// with no variant here decodes as `None`, and that readback row is dropped with nothing marking it.
const _: () = assert!(TF_SWAP + 1 == TF_OP_COUNT);

impl TransformOp {
    /// The wire `op` byte.
    pub const fn as_u8(self) -> u8 {
        self as u8
    }

    /// Map a wire `op` byte to a [`TransformOp`], or `None` for an unknown value.
    pub fn from_u8(v: u8) -> Option<TransformOp> {
        Some(match v {
            TF_REMAP => TransformOp::Remap,
            TF_SWAP => TransformOp::Swap,
            _ => return None,
        })
    }

    /// Whether this op admits the given `source`/`dest` pair, mirroring the box's own check. Neither op
    /// takes a field onto itself: both move a value, and there is nowhere to move it to. Whether the
    /// fields are declared is the box's to answer.
    pub fn admits(self, source: LockTarget, dest: LockTarget) -> bool {
        use LockTarget::{Axis, Usage};
        if source == dest {
            return false;
        }
        match self {
            TransformOp::Swap => matches!((source, dest), (Axis(_), Axis(_))),
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
/// common are the same entry: setting the second overwrites the first's op.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TransformKey {
    /// The field the transform reads.
    pub source: LockTarget,
    /// The field the transform writes.
    pub dest: LockTarget,
}

/// One field transform: an [operation](TransformOp), the [`source`](Transform::source) field it reads
/// and the [`dest`](Transform::dest) field it writes.
///
/// A transform is purely structural. It says where a field's value lands, never how much of it
/// survives: that is [`scale`](crate::Device::scale)'s, which runs first and whose percent is signed.
///
/// ```
/// # use medius::{Axis, Transform, TransformOp};
/// let swap = Transform::swap(Axis::X, Axis::Y);
/// assert_eq!(swap.op, TransformOp::Swap);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Transform {
    /// What the transform does to its fields.
    pub op: TransformOp,
    /// The field the transform reads.
    pub source: LockTarget,
    /// The field the transform writes.
    pub dest: LockTarget,
}

impl Transform {
    /// A transform from an explicit op, source and destination.
    pub fn new(
        op: TransformOp,
        source: impl Into<LockTarget>,
        dest: impl Into<LockTarget>,
    ) -> Transform {
        Transform {
            op,
            source: source.into(),
            dest: dest.into(),
        }
    }

    /// Exchange two axes atomically: read both, then write both.
    pub fn swap(a: crate::types::Axis, b: crate::types::Axis) -> Transform {
        Transform {
            op: TransformOp::Swap,
            source: LockTarget::Axis(a),
            dest: LockTarget::Axis(b),
        }
    }

    /// Move a source field's value into a destination, clearing the source. Same-report for axis to
    /// axis and button to button; a button to key or media remap holds the destination through its own
    /// interface for as long as the button is down.
    pub fn remap(source: impl Into<LockTarget>, dest: impl Into<LockTarget>) -> Transform {
        Transform {
            op: TransformOp::Remap,
            source: source.into(),
            dest: dest.into(),
        }
    }

    /// The `(source, dest)` key this entry is filed under.
    pub fn key(&self) -> TransformKey {
        TransformKey {
            source: self.source,
            dest: self.dest,
        }
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
    /// `[op u8][sclass u8][sid u16][dclass u8][did u16]`, with no `state` byte per entry.
    pub(crate) fn from_payload(p: &[u8]) -> Option<Transforms> {
        if p.len() < 3 {
            return None;
        }
        let table_full = p[1] & TF_F_FULL != 0;
        let n = p[2] as usize;
        let mut entries = Vec::with_capacity(n);
        for i in 0..n {
            let o = 3 + 7 * i;
            let row = p.get(o..o + 7)?;
            // A byte the crate has no enum for (an op or class a newer box added) skips this entry
            // rather than aborting the whole decode: the rest of the table still reads.
            let (Some(op), Some(source), Some(dest)) = (
                TransformOp::from_u8(row[0]),
                LockTarget::from_class_id(row[1], u16::from_le_bytes([row[2], row[3]])),
                LockTarget::from_class_id(row[4], u16::from_le_bytes([row[5], row[6]])),
            ) else {
                continue;
            };
            entries.push(Transform { op, source, dest });
        }
        Some(Transforms {
            table_full,
            entries,
        })
    }
}
