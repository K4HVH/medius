use crate::error::{Error, Result};
use crate::link::reconcile::StoredTransform;
use crate::protocol::command::transform_payload;
use crate::protocol::opcode::Q_TRANSFORMS;
use crate::protocol::{FrameType, Resp, parse_resp};
use crate::types::{Axis, Transform, TransformField, TransformOp, TransformState, Transforms};

use super::Device;

impl Device {
    /// `TRANSFORM` (§3.15): install (add or overwrite) one field transform.
    ///
    /// A transform negates, scales, swaps or remaps a field the clone's descriptor already declares, on
    /// the semantic path where locks, riding and rendering run. Every emitted report stays one the real
    /// device could produce, so a transform is **faithful and needs no**
    /// [`allow_imperfect_clones`](Device::allow_imperfect_clones), unlike the rewrite/raw/patch layer.
    /// An entry is keyed by its `(source, dest)`; setting one whose key exists overwrites its op and
    /// scale.
    ///
    /// Transforms are session state, re-asserted on reconnect and held alive by the keepalive exactly
    /// like a [`lock`](Device::lock), and cleared on control-PC silence, [`reset`](Device::reset), a
    /// device detach, a link drop or a re-clone.
    ///
    /// The crate rejects the two refusals it can see structurally: an op a class pair cannot take
    /// ([`Error::TransformOpFields`]) and a `scale` of `0` on an
    /// [`Invert`](crate::TransformOp::Invert) ([`Error::TransformInvertZeroScale`]). The
    /// device-dependent refusals (a field the clone does not declare, a cross-class remap with no
    /// destination collection, the eight-entry table full) are the box's to make; delivery is
    /// fire-and-forget and [`query_transforms`](Device::query_transforms) confirms what it holds (a
    /// refused entry is simply absent).
    ///
    /// ```no_run
    /// # use medius::{Axis, Device, Result, Transform};
    /// # fn main() -> Result<()> {
    /// let device = Device::find()?;
    /// device.transform(&Transform::invert(Axis::Y))?;             // flip vertical motion on the wire
    /// device.transform(&Transform::scale_axis(Axis::Wheel, 200))?; // double the wheel's detents
    /// # Ok(()) }
    /// ```
    pub fn transform(&self, t: &Transform) -> Result<()> {
        validate_transform(t)?;
        self.transform_send(t, TransformState::Set)
    }

    /// The `TRANSFORM` set/remove send with no pre-validation, so the ergonomic helpers and the async
    /// wrapper share one core. Records the entry (or its removal) for reconnect-replay before the write,
    /// then rolls back if the frame never went out (the lock/rewrite pattern).
    pub(crate) fn transform_send(&self, t: &Transform, state: TransformState) -> Result<()> {
        let stored = to_stored(t);
        // Serialise the DesiredState write and its send against the keepalive/reconnect re-assert so a
        // concurrent remove/clear can't interleave.
        let _serial = self.link.reassert_guard();
        let undo = match state {
            TransformState::Set => self.link.desired().lock().apply_transform(stored),
            TransformState::Remove => self.link.desired().lock().remove_transform(stored.key()),
        };
        let (sclass, sid) = t.source.class_id();
        let (dclass, did) = t.dest.class_id();
        let sent = self.link.send(
            FrameType::Transform,
            &transform_payload(
                t.op.as_u8(),
                sclass,
                sid,
                dclass,
                did,
                t.scale,
                state.as_u8(),
            ),
        );
        if sent.is_err() {
            self.link.desired().lock().restore_transform(undo);
        }
        sent
    }

    /// `TRANSFORM` remove (§3.15): drop the transform keyed by this entry's `(source, dest)`. The op
    /// and scale are ignored. A no-op on the box if no such entry is held.
    pub fn untransform(&self, t: &Transform) -> Result<()> {
        self.transform_send(t, TransformState::Remove)
    }

    /// `TRANSFORM` clear (§3.15): drop the whole transform table (the `class 0xFF, id 0xFFFF, state 0`
    /// blanket).
    pub fn clear_transforms(&self) -> Result<()> {
        // Snapshot the held entries first so a failed send restores them (the box still holds them),
        // and serialise against the re-assert like `clear_rewrite`.
        let _serial = self.link.reassert_guard();
        let held = {
            let mut d = self.link.desired().lock();
            let held = d.held_transforms();
            d.clear_transforms();
            held
        };
        // The clear sentinel: op is ignored, both classes 0xFF, both ids 0xFFFF, state 0.
        let sent = self.link.send(
            FrameType::Transform,
            &transform_payload(0, 0xFF, 0xFFFF, 0xFF, 0xFFFF, 0, 0),
        );
        if sent.is_err() {
            let mut d = self.link.desired().lock();
            for t in held {
                d.apply_transform(t);
            }
        }
        sent
    }

    /// Invert an axis on the wire: convenience for [`transform`](Device::transform) of
    /// [`Transform::invert`].
    pub fn invert(&self, axis: Axis) -> Result<()> {
        self.transform(&Transform::invert(axis))
    }

    /// Weigh an axis by a signed percent (`200` doubles, `-50` halves and flips): convenience for
    /// [`transform`](Device::transform) of [`Transform::scale_axis`]. Named `scale_transform` because
    /// [`scale_axis`](Device::scale_axis) is the `LOCK` weigh, a different operation.
    pub fn scale_transform(&self, axis: Axis, percent: i16) -> Result<()> {
        self.transform(&Transform::scale_axis(axis, percent))
    }

    /// Exchange two axes on the wire: convenience for [`transform`](Device::transform) of
    /// [`Transform::swap`].
    pub fn swap(&self, a: Axis, b: Axis) -> Result<()> {
        self.transform(&Transform::swap(a, b))
    }

    /// Remap a source field into a destination: convenience for [`transform`](Device::transform) of
    /// [`Transform::remap`].
    pub fn remap(
        &self,
        source: impl Into<TransformField>,
        dest: impl Into<TransformField>,
    ) -> Result<()> {
        self.transform(&Transform::remap(source, dest))
    }

    /// `QUERY(TRANSFORMS)` → [`Transforms`] (§4.18): the whole transform table, read back as the
    /// entries that rebuild it, plus the table-full flag. A refused entry is absent.
    pub fn query_transforms(&self) -> Result<Transforms> {
        let payload = self.link.query(Q_TRANSFORMS)?;
        match parse_resp(&payload) {
            Some(Resp::Transforms(t)) => Ok(t),
            _ => Err(Error::NoReply),
        }
    }
}

pub(crate) fn to_stored(t: &Transform) -> StoredTransform {
    let (sclass, sid) = t.source.class_id();
    let (dclass, did) = t.dest.class_id();
    StoredTransform {
        op: t.op.as_u8(),
        sclass,
        sid,
        dclass,
        did,
        scale: t.scale,
    }
}

/// The structural refusals the crate can make before the wire, mirroring the box's `transform_tab_set`:
/// an op a class pair cannot take, and a `scale` of 0 on an [`Invert`](TransformOp::Invert) (which
/// ignores the scale, so 0 would block a field the op is not meant to). The device-dependent refusals
/// stay the box's.
pub(crate) fn validate_transform(t: &Transform) -> Result<()> {
    if !t.op.admits(t.source, t.dest) {
        return Err(Error::TransformOpFields {
            op: t.op,
            src: t.source,
            dst: t.dest,
        });
    }
    if t.op == TransformOp::Invert && t.scale == 0 {
        return Err(Error::TransformInvertZeroScale);
    }
    Ok(())
}
