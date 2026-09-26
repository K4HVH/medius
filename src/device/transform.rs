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
    /// `TRANSFORM` (§3.15): add or overwrite one field transform.
    ///
    /// Moves a field the clone's descriptor declares into another, by swapping two axes or remapping
    /// a source onto a destination. To weigh a field, or reverse it, use [`scale`](Device::scale),
    /// which runs first and passes the transform what it kept. Every emitted report stays one the real
    /// device could produce, so no [`allow_imperfect_clones`](Device::allow_imperfect_clones) needed.
    ///
    /// Keyed by `(source, dest)`; setting an existing key overwrites its op in place, keeping its
    /// position. Position is state: entries apply in installation order, and two writing one field do
    /// not commute.
    ///
    /// Transforms are session state, re-asserted after a reconnect, a device-chip restart or any
    /// session release the box counts, and kept by the keepalive like a [`lock`](Device::lock). The
    /// box clears them on control-PC silence, [`reset`](Device::reset), a device detach, a link drop
    /// or a re-clone.
    ///
    /// Fire-and-forget; [`query_transforms`](Device::query_transforms) confirms what the box holds,
    /// and omits an entry naming a field the clone does not declare.
    ///
    /// ```no_run
    /// # use medius::{Axis, Device, Result, Transform};
    /// # fn main() -> Result<()> {
    /// let device = Device::find()?;
    /// device.transform(&Transform::swap(Axis::X, Axis::Y))?;   // X and Y exchanged
    /// device.transform(&Transform::remap(Axis::Wheel, Axis::Y))?; // wheel moves Y
    /// # Ok(()) }
    /// ```
    pub fn transform(&self, t: &Transform) -> Result<()> {
        validate_transform(t)?;
        // One guard over the capacity read and the send: it is not reentrant, and releasing it
        // between them lets a concurrent set take the last slot.
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

    pub(crate) fn transform_send(&self, t: &Transform, state: u8) -> Result<()> {
        let _serial = self.link.reassert_guard();
        self.transform_send_locked(t, state)
    }

    // Records the entry (or its removal) for replay before the write; rolled back if the frame never
    // went out. The caller holds the re-assert guard.
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

    /// `TRANSFORM` remove (§3.15): drop the transform with this entry's `(source, dest)`; the op is
    /// ignored. A no-op on the box if no such entry is held.
    pub fn untransform(&self, t: &Transform) -> Result<()> {
        self.transform_send(t, REMOVE)
    }

    /// `TRANSFORM` clear (§3.15): drop the whole table (the `class 0xFF, id 0xFFFF, state 0`
    /// blanket).
    pub fn clear_transforms(&self) -> Result<()> {
        // As `clear_rewrite`: the snapshot restores the entries if the send fails.
        let _serial = self.link.reassert_guard();
        let held = {
            let mut d = self.link.desired().lock();
            let held = d.held_transforms();
            d.clear_transforms();
            held
        };
        // Clear sentinel: op ignored, both classes 0xFF, both ids 0xFFFF, state 0.
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

    /// Exchange two axes: [`transform`](Device::transform) of [`Transform::swap`].
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

    /// `QUERY(TRANSFORMS)` → [`Transforms`] (§4.18): the table as the entries that rebuild it, in
    /// application order, and the table-full flag.
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

// Mirrors the box's `transform_tab_set`: an op a class pair cannot take, including one field as both
// ends. Which fields the clone declares is for the box to check.
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
