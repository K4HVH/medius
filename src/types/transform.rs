//! `TRANSFORM` (§3.15) vocabulary: field operations and decoded `RESP(TRANSFORMS)`.

use crate::protocol::opcode::{TF_F_FULL, TF_OP_COUNT, TF_REMAP, TF_SWAP, TRANSFORM_MAX_ENTRIES};
use crate::types::{Class, LockTarget};

/// Operation a [`Transform`] performs (§3.15); both move a value between fields. To weigh a field, or
/// reverse it, use [`scale`](crate::Device::scale).
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TransformOp {
    /// Move a source field's value into a destination, clearing the source.
    Remap = TF_REMAP,
    /// Exchange two axes: read both, then write both (not two remaps).
    Swap = TF_SWAP,
}

// `Swap` tops the dense op space. A box op with no variant decodes as `None`, and its readback row
// is dropped with nothing marking it.
const _: () = assert!(TF_SWAP + 1 == TF_OP_COUNT);

impl TransformOp {
    /// Wire `op` byte.
    pub const fn as_u8(self) -> u8 {
        self as u8
    }

    /// Decodes a wire `op` byte; `None` if unknown.
    pub fn from_u8(v: u8) -> Option<TransformOp> {
        Some(match v {
            TF_REMAP => TransformOp::Remap,
            TF_SWAP => TransformOp::Swap,
            _ => return None,
        })
    }

    /// Whether this op admits `source`/`dest`, mirroring the box's check; one field as both ends is
    /// refused. Whether the fields are declared is for the box to check.
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

/// `(source, dest)` key of one transform-table entry; setting a second transform with the same key
/// overwrites the first's op.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TransformKey {
    /// Field read.
    pub source: LockTarget,
    /// Field written.
    pub dest: LockTarget,
}

/// Field transform: an [operation](TransformOp), the [`source`](Transform::source) field it reads
/// and the [`dest`](Transform::dest) field it writes.
///
/// A transform sets where a field's value lands. To weigh one, or reverse it, use
/// [`scale`](crate::Device::scale), which runs first and passes the transform what it kept.
///
/// ```
/// # use medius::{Axis, Transform, TransformOp};
/// let swap = Transform::swap(Axis::X, Axis::Y);
/// assert_eq!(swap.op, TransformOp::Swap);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Transform {
    /// Operation.
    pub op: TransformOp,
    /// Field read.
    pub source: LockTarget,
    /// Field written.
    pub dest: LockTarget,
}

impl Transform {
    /// Transform from an explicit op, source and destination.
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
    /// axis and button to button; a button to key or media remap holds the destination on its own
    /// interface while the button is down.
    pub fn remap(source: impl Into<LockTarget>, dest: impl Into<LockTarget>) -> Transform {
        Transform {
            op: TransformOp::Remap,
            source: source.into(),
            dest: dest.into(),
        }
    }

    /// The entry's `(source, dest)` key.
    pub fn key(&self) -> TransformKey {
        TransformKey {
            source: self.source,
            dest: self.dest,
        }
    }
}

/// Decoded `RESP(TRANSFORMS)` (§4.18): the transform table as the commands that rebuild it, in the
/// order the box applies them.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Transforms {
    /// The table is full: a further entry was, or would be, refused.
    pub table_full: bool,
    /// One entry per installed transform, in installation order, which the box applies them in.
    /// Transforms writing one field do not commute, so the order is part of the state.
    pub entries: Vec<Transform>,
}

impl Transforms {
    /// Max entries the box holds.
    pub const CAPACITY: usize = TRANSFORM_MAX_ENTRIES;

    /// `[what][flags u8][n u8]` then `n` × `[op u8][sclass u8][sid u16][dclass u8][did u16]`, with no
    /// `state` byte per entry.
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
            // An op or class a newer box added skips this entry; the rest of the table still reads.
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
