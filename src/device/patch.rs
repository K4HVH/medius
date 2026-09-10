use crate::error::{Error, Result};
use crate::protocol::command::{patch_apply_payload, patch_clear_payload, patch_payload};
use crate::protocol::opcode::{Q_PATCH_ENTRY, Q_PATCHES};
use crate::protocol::{FrameType, Resp, parse_resp};
use crate::types::patch::patch_entry_from_payload;
use crate::types::{Patch, PatchSet};

use super::Device;

impl Device {
    /// `PATCH` (§3.14): store one descriptor patch, keyed by `(section, cfg, index, offset)`.
    ///
    /// A patch overwrites bytes in what the clone presents at enumeration, persisted per device
    /// (VID:PID) in the box's NVS. A patch with empty [`bytes`](Patch::bytes) removes the patch at that
    /// key. Unlike a rewrite rule a patch is configuration, not session state: it survives a reconnect
    /// and clears only on [`clear_patch`](Device::clear_patch). Storing a patch is **not** gated on the
    /// opt-in — the box always stores it — but it takes effect only once
    /// [`apply_patch`](Device::apply_patch) re-presents the clone under
    /// [`allow_imperfect_clones`](Device::allow_imperfect_clones). A patch never changes a descriptor's
    /// byte count; the box refuses (and logs) an apply whose descriptors would then advertise one
    /// length and serve another. Fire-and-forget; [`query_patches`](Device::query_patches) confirms.
    pub fn set_patch(&self, patch: &Patch) -> Result<()> {
        self.link.send(
            FrameType::Patch,
            &patch_payload(
                patch.section.as_u8(),
                patch.cfg,
                patch.index,
                patch.offset,
                &patch.bytes,
            ),
        )
    }

    /// `PATCH` APPLY (§3.14): re-present the clone with the stored patch set (one unplug/replug to the
    /// game PC). Gated on [`allow_imperfect_clones`](Device::allow_imperfect_clones): with the opt-in
    /// off this returns [`Error::ImperfectRequired`](crate::Error::ImperfectRequired), since the box
    /// applies a set only under the opt-in and would otherwise re-present unchanged.
    pub fn apply_patch(&self) -> Result<()> {
        self.require_imperfect()?;
        self.apply_patch_send()
    }

    /// The `PATCH` APPLY send with no opt-in pre-check, so the async wrapper can gate on the async query path.
    pub(crate) fn apply_patch_send(&self) -> Result<()> {
        self.link.send(FrameType::Patch, &patch_apply_payload())
    }

    /// `PATCH` CLEAR (§3.14): drop every patch for this device and re-present the clone unpatched.
    pub fn clear_patch(&self) -> Result<()> {
        self.link.send(FrameType::Patch, &patch_clear_payload())
    }

    /// `QUERY(PATCHES)` → [`PatchSet`] (§4.17): the stored patch set and its apply state (applied,
    /// pending, last-apply-refused, store-full), a row per patch without its bytes. Read one patch in
    /// full with [`query_patch_entry`](Device::query_patch_entry).
    pub fn query_patches(&self) -> Result<PatchSet> {
        let payload = self.link.query(Q_PATCHES)?;
        match parse_resp(&payload) {
            Some(Resp::Patches(s)) => Ok(s),
            _ => Err(Error::NoReply),
        }
    }

    /// `QUERY(PATCH_ENTRY, index)` → [`Patch`] (§4.17): one patch in full, in the shape
    /// [`set_patch`](Device::set_patch) takes, so a read patch replays as a set. `index` is the row in
    /// the [`query_patches`](Device::query_patches) summary.
    pub fn query_patch_entry(&self, index: u8) -> Result<Patch> {
        let payload = self.link.query_indexed(Q_PATCH_ENTRY, index)?;
        patch_entry_from_payload(&payload).ok_or(Error::NoReply)
    }
}
