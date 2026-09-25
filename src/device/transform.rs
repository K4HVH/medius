use crate::error::{Error, Result};
use crate::link::reconcile::StoredTransform;
use crate::protocol::command::transform_payload;
use crate::protocol::opcode::{Q_TRANSFORMS, TRANSFORM_MAX_ENTRIES};
use crate::protocol::{FrameType, Resp, parse_resp};
use crate::types::{Axis, LockTarget, Transform, Transforms};

use super::Device;

const SET: u8 = 1;
const REMOVE: u8 = 0;

impl Device {
    /// `TRANSFORM` (§3.15): install (add or overwrite) one field transform.
    ///
    /// A transform moves a field the clone's descriptor already declares into another one, by swapping
    /// two axes or remapping a source onto a destination. To weigh a field, or reverse it, use
    /// [`scale`](Device::scale), which runs first and hands the transform what it kept. Every emitted
    /// report stays one the real device could produce, so a transform needs no
    /// [`allow_imperfect_clones`](Device::allow_imperfect_clones).
    ///
    /// An entry is keyed by its `(source, dest)`; setting one whose key exists overwrites its op in
    /// place, keeping its position. Position is the state: entries apply in installation order, and two
    /// that write the same field do not commute.
    ///
    /// Transforms are session state, re-asserted after a reconnect, a device-chip restart or any release
    /// of the session the box counts, and held alive by the keepalive exactly like a
    /// [`lock`](Device::lock). The box clears them on control-PC silence, [`reset`](Device::reset), a
    /// device detach, a link drop or a re-clone.
    ///
    /// Delivery is fire-and-forget; [`query_transforms`](Device::query_transforms) confirms what the
    /// box holds, and an entry naming a field the clone does not declare is absent from it.
    ///
    /// ```no_run
    /// # use medius::{Axis, Device, Result, Transform};
    /// # fn main() -> Result<()> {
    /// let device = Device::find()?;
    /// device.transform(&Transform::swap(Axis::X, Axis::Y))?;   // the mouse's two axes, exchanged
    /// device.transform(&Transform::remap(Axis::Wheel, Axis::Y))?; // the wheel drives vertical motion
    /// # Ok(()) }
    /// ```
    pub fn transform(&self, t: &Transform) -> Result<()> {
        validate_transform(t)?;
        // One guard for the capacity read and the send: taking it twice would deadlock, and releasing
        // it between them would let a concurrent set take the last slot.
        let _serial = self.link.reassert_guard();
        {
            let d = self.link.desired().lock();
            if !d.holds_transform(to_stored(t).key())
                && d.transform_count() >= TRANSFORM_MAX_ENTRIES
            {
                return Err(Error::TransformTableFull {
                    limit: TRANSFORM_MAX_ENTRIES,
                });
            }
        }
        self.transform_send_locked(t, SET)
    }

    /// The `TRANSFORM` set/remove send, so the ergonomic helpers and the async wrapper share one core.
    pub(crate) fn transform_send(&self, t: &Transform, state: u8) -> Result<()> {
        // Serialise the DesiredState write and its send against the keepalive/reconnect re-assert so a
        // concurrent remove/clear can't interleave.
        let _serial = self.link.reassert_guard();
        self.transform_send_locked(t, state)
    }

    /// Records the entry (or its removal) for reconnect-replay before the write, then rolls back if the
    /// frame never went out. The caller holds the re-assert guard.
    fn transform_send_locked(&self, t: &Transform, state: u8) -> Result<()> {
        let stored = to_stored(t);
        let undo = if state == SET {
            self.link.desired().lock().apply_transform(stored)
        } else {
            self.link.desired().lock().remove_transform(stored.key())
        };
        let (sclass, sid) = t.source.class_id();
        let (dclass, did) = t.dest.class_id();
        let sent = self.link.send(
            FrameType::Transform,
            &transform_payload(t.op.as_u8(), sclass, sid, dclass, did, state),
        );
        if sent.is_err() {
            self.link.desired().lock().restore_transform(undo);
        }
        sent
    }

    /// `TRANSFORM` remove (§3.15): drop the transform keyed by this entry's `(source, dest)`. The op is
    /// ignored. A no-op on the box if no such entry is held.
    pub fn untransform(&self, t: &Transform) -> Result<()> {
        self.transform_send(t, REMOVE)
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
            &transform_payload(0, 0xFF, 0xFFFF, 0xFF, 0xFFFF, REMOVE),
        );
        if sent.is_err() {
            let mut d = self.link.desired().lock();
            for t in held {
                d.apply_transform(t);
            }
        }
        sent
    }

    /// Exchange two axes on the wire: [`transform`](Device::transform) of [`Transform::swap`].
    pub fn transform_swap(&self, a: Axis, b: Axis) -> Result<()> {
        self.transform(&Transform::swap(a, b))
    }

    /// Move a source field into a destination: [`transform`](Device::transform) of
    /// [`Transform::remap`].
    pub fn transform_remap(
        &self,
        source: impl Into<LockTarget>,
        dest: impl Into<LockTarget>,
    ) -> Result<()> {
        self.transform(&Transform::remap(source, dest))
    }

    /// `QUERY(TRANSFORMS)` → [`Transforms`] (§4.18): the whole transform table, read back as the
    /// entries that rebuild it, in the order the box applies them, plus the table-full flag.
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
    }
}

/// The one refusal the crate can make before the wire, mirroring the box's own `transform_tab_set`: an
/// op a class pair cannot take, one field named as both ends included. Which fields the clone declares
/// stays the box's to answer.
pub(crate) fn validate_transform(t: &Transform) -> Result<()> {
    if !t.op.admits(t.source, t.dest) {
        return Err(Error::TransformOpFields {
            op: t.op,
            src: t.source,
            dst: t.dest,
        });
    }
    Ok(())
}
