//! `TRANSFORM` (§3.15) vocabulary: the field operation a host installs, the field it reads and the
//! field it writes, the signed scale, and the decoded `RESP(TRANSFORMS)` readback.
//!
//! A transform is a field operation on the semantic path: it negates, scales, swaps or remaps a field
//! the box's descriptor already declares, so every emitted report stays one the real device could
//! itself produce. It is descriptor-bounded on both ends — a source or destination the parsed map does
//! not declare is refused — and it is session state, re-asserted on reconnect and held alive by the
//! keepalive exactly like a [`lock`](crate::Device::lock). Unlike the rewrite/raw/patch layer it is
//! faithful and needs no [imperfect-clone opt-in](crate::Device::allow_imperfect_clones).

use crate::protocol::opcode::{
    CATCH_CLS_AXIS, LOCK_SCALE_PASS, TF_F_FULL, TF_INVERT, TF_REMAP, TF_SCALE, TF_SWAP,
};
use crate::types::{Axis, Class, Usage};

/// The operation a [`Transform`] performs on its fields (§3.15).
///
/// [`Invert`](TransformOp::Invert) and [`Scale`](TransformOp::Scale) act on one axis (source ==
/// destination); [`Swap`](TransformOp::Swap) exchanges two axes; [`Remap`](TransformOp::Remap) moves a
/// source field into a destination — axis→axis or button→button in one report, or button→key /
/// button→media across classes. [`admits`](TransformOp::admits) mirrors the box's own admissibility
/// check over a source/destination pair.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TransformOp {
    /// Move a source field's contribution into a destination, clearing the source.
    Remap = TF_REMAP,
    /// Exchange two axes: read both, then write both, so it is not two remaps.
    Swap = TF_SWAP,
    /// Negate one axis; the signed scale is ignored.
    Invert = TF_INVERT,
    /// Weigh one axis by the signed scale.
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
            TF_INVERT => TransformOp::Invert,
            TF_SCALE => TransformOp::Scale,
            _ => return None,
        })
    }

    /// Whether this op admits the given `source`/`dest` pair, mirroring the box's `transform_pair_ok`:
    /// [`Invert`](TransformOp::Invert)/[`Scale`](TransformOp::Scale) are one axis (source ==
    /// destination), [`Swap`](TransformOp::Swap) is two axes, and [`Remap`](TransformOp::Remap) is
    /// axis→axis, button→button, button→key or button→media. This is the structural check the crate can
    /// make before the wire; whether the fields are actually declared is the box's to answer.
    pub fn admits(self, source: TransformField, dest: TransformField) -> bool {
        use TransformField::Axis;
        match self {
            TransformOp::Invert | TransformOp::Scale => {
                matches!((source, dest), (Axis(a), Axis(b)) if a == b)
            }
            TransformOp::Swap => matches!((source, dest), (Axis(_), Axis(_))),
            TransformOp::Remap => match (source, dest) {
                (Axis(_), Axis(_)) => true,
                (TransformField::Usage(s), TransformField::Usage(d)) => {
                    s.class == Class::Button
                        && matches!(d.class, Class::Button | Class::Key | Class::Media)
                }
                _ => false,
            },
        }
    }
}

/// A field a [`Transform`] reads or writes: a relative [`Axis`], or a momentary [`Usage`]
/// (button/key/media).
///
/// This is the widened input core's first-class field space, addressed as the box addresses it: a
/// class plus an id. An axis carries the axis class byte and an id 0..3 (X/Y/wheel/pan); a usage
/// carries `INJECT`'s class byte (button 0 / key 1 / media 2) and the class-specific id. Any [`Axis`],
/// [`Button`](crate::Button), [`Key`](crate::Key) or [`MediaKey`](crate::MediaKey) converts in with
/// [`From`], as does a whole [`Usage`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TransformField {
    /// A relative axis (X/Y/wheel/pan).
    Axis(Axis),
    /// A momentary usage: a button, key or media usage.
    Usage(Usage),
}

impl TransformField {
    /// The wire `(class, id)` this field encodes to: an axis is the axis class byte, a usage carries
    /// its own [`Class`] byte.
    pub fn class_id(self) -> (u8, u16) {
        match self {
            TransformField::Axis(a) => (CATCH_CLS_AXIS, a.as_u16()),
            TransformField::Usage(u) => u.class_id(),
        }
    }

    /// Map a wire `(class, id)` back to a [`TransformField`], or `None` for a class no field names or
    /// an axis id past the four axes.
    pub fn from_class_id(class: u8, id: u16) -> Option<TransformField> {
        if class == CATCH_CLS_AXIS {
            Some(TransformField::Axis(Axis::from_u16(id)?))
        } else {
            Some(TransformField::Usage(Usage::new(
                Class::from_u8(class)?,
                id,
            )))
        }
    }

    /// The axis this field names, or `None` if it is a momentary usage.
    pub fn as_axis(self) -> Option<Axis> {
        match self {
            TransformField::Axis(a) => Some(a),
            TransformField::Usage(_) => None,
        }
    }
}

impl From<Axis> for TransformField {
    fn from(a: Axis) -> TransformField {
        TransformField::Axis(a)
    }
}

impl<T: Into<Usage>> From<T> for TransformField {
    fn from(u: T) -> TransformField {
        TransformField::Usage(u.into())
    }
}

/// The `(source, dest)` key that identifies one transform-table entry. Two transforms with this key in
/// common are the same entry: setting the second overwrites the first's op and scale.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TransformKey {
    /// The field the transform reads.
    pub source: TransformField,
    /// The field the transform writes.
    pub dest: TransformField,
}

/// Whether a [`transform`](crate::Device::transform) command adds/overwrites an entry or removes one;
/// the wire `state` byte.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TransformState {
    /// Remove the entry with this key.
    Remove = 0,
    /// Add or overwrite the entry with this key.
    Set = 1,
}

impl TransformState {
    /// The wire `state` byte.
    pub fn as_u8(self) -> u8 {
        self as u8
    }
}

/// One field transform: an [operation](TransformOp), the [`source`](Transform::source) field it reads,
/// the [`dest`](Transform::dest) field it writes, and a signed scale.
///
/// The **signed scale** is a percent carrying a sign: `-100` inverts, `100` is identity, `200` doubles,
/// `-50` halves and flips, and `0` blocks the source. [`Invert`](TransformOp::Invert) ignores it (the
/// negation is exact), and the box refuses an invert whose scale is `0`, so [`invert`](Transform::invert)
/// carries a non-zero placeholder. The transformed value is always clamped to the destination field's
/// declared logical range, which is what keeps a scale within-range and the whole feature faithful.
///
/// Build one with [`invert`](Transform::invert), [`scale_axis`](Transform::scale_axis),
/// [`swap`](Transform::swap) or [`remap`](Transform::remap), or [`new`](Transform::new) for the general
/// case, and adjust the scale with [`with_scale`](Transform::with_scale).
///
/// ```
/// # use medius::{Axis, Transform, TransformOp};
/// // Invert Y, and scale the wheel to double its detents.
/// let inv = Transform::invert(Axis::Y);
/// let fast = Transform::scale_axis(Axis::Wheel, 200);
/// assert_eq!(inv.op, TransformOp::Invert);
/// assert_eq!(fast.scale, 200);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Transform {
    /// What the transform does to its fields.
    pub op: TransformOp,
    /// The field the transform reads.
    pub source: TransformField,
    /// The field the transform writes (equal to [`source`](Transform::source) for invert and scale).
    pub dest: TransformField,
    /// The signed percent (see the type docs). Ignored by [`Invert`](TransformOp::Invert).
    pub scale: i16,
}

impl Transform {
    /// A transform from an explicit op, source, destination and signed scale. Prefer the named
    /// constructors where they fit; this is the general form a readback rebuilds through.
    pub fn new(
        op: TransformOp,
        source: impl Into<TransformField>,
        dest: impl Into<TransformField>,
        scale: i16,
    ) -> Transform {
        Transform {
            op,
            source: source.into(),
            dest: dest.into(),
            scale,
        }
    }

    /// Invert an axis: emit the report the device produces when moved the other way. The scale is
    /// ignored, so this carries a non-zero placeholder the box will accept.
    pub fn invert(axis: Axis) -> Transform {
        Transform {
            op: TransformOp::Invert,
            source: TransformField::Axis(axis),
            dest: TransformField::Axis(axis),
            scale: LOCK_SCALE_PASS as i16,
        }
    }

    /// Weigh an axis by a signed percent: `200` doubles, `-50` halves and flips, `0` blocks it. The
    /// result is clamped to the axis's declared range, so it can never leave the wire out of range.
    pub fn scale_axis(axis: Axis, percent: i16) -> Transform {
        Transform {
            op: TransformOp::Scale,
            source: TransformField::Axis(axis),
            dest: TransformField::Axis(axis),
            scale: percent,
        }
    }

    /// Exchange two axes atomically (read both, then write both). Defaults to an identity swap; add a
    /// [`with_scale`](Transform::with_scale) to weigh both directions.
    pub fn swap(a: Axis, b: Axis) -> Transform {
        Transform {
            op: TransformOp::Swap,
            source: TransformField::Axis(a),
            dest: TransformField::Axis(b),
            scale: LOCK_SCALE_PASS as i16,
        }
    }

    /// Move a source field's contribution into a destination, clearing the source. Same-report for
    /// axis→axis and button→button; a button→key or button→media remap emits the destination through
    /// its own interface on the button's edge. Defaults to an identity scale.
    pub fn remap(source: impl Into<TransformField>, dest: impl Into<TransformField>) -> Transform {
        Transform {
            op: TransformOp::Remap,
            source: source.into(),
            dest: dest.into(),
            scale: LOCK_SCALE_PASS as i16,
        }
    }

    /// Set the signed scale, for a scaled [`swap`](Transform::swap) or [`remap`](Transform::remap).
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
}

/// The decoded `RESP(TRANSFORMS)` (§4.18): the whole transform table, read back as the commands that
/// rebuild it.
///
/// The readback carries no generation counter (the transform table is re-asserted wholesale on
/// reconnect, matching the lock model), and each entry omits the wire `state` byte, since a readback
/// entry is always a live one. A refused entry is simply absent, with
/// [`table_full`](Transforms::table_full) flagged if the ceiling of eight was the reason.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Transforms {
    /// The table is full: a further entry was, or would be, refused.
    pub table_full: bool,
    /// One entry per installed transform, in installation order.
    pub entries: Vec<Transform>,
}

impl Transforms {
    /// Decode a `RESP(TRANSFORMS)` payload (§4.18): `[what][flags u8][n u8]` then `n` ×
    /// `[op u8][sclass u8][sid u16][dclass u8][did u16][scale i16]` — no `state` byte per entry.
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
                TransformField::from_class_id(row[1], u16::from_le_bytes([row[2], row[3]])),
                TransformField::from_class_id(row[4], u16::from_le_bytes([row[5], row[6]])),
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
