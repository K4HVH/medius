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
    /// key. The set belongs to the attached device, so a patch stored with none attached is dropped.
    /// Unlike a rewrite rule a patch is configuration, not session state: it survives a reconnect
    /// and clears on [`clear_patch`](Device::clear_patch) or [`factory_reset`](Device::factory_reset).
    /// Storing a patch is **not** gated on the opt-in (the box always stores it), and it reaches the
    /// game PC only when the clone is next presented under
    /// [`allow_imperfect_clones`](Device::allow_imperfect_clones): an [`apply_patch`](Device::apply_patch),
    /// the opt-in turning on, or the device attaching. Every section but
    /// [`String`](crate::PatchSection::String) keeps the descriptor's byte count. Patched descriptors
    /// that fail the box's clone or consistency checks are served unpatched, and the box logs which
    /// ([`PatchSet::refused`]). A box that could not read the device's stored set from flash drops
    /// edits until it can. Fire-and-forget; [`query_patches`](Device::query_patches) confirms.
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
    /// ignores APPLY then. The box re-presents only while the stored set differs from the one served
    /// ([`PatchSet::pending`]), so applying an emptied set serves the device unpatched, and it leaves a
    /// [refused](PatchSet::refused) set that has not changed since.
    ///
    /// Presenting the clone again releases the session like a replug of the device: the crate
    /// re-sends what it holds once the new clone is up, and
    /// [`ClipHandle::lost`](crate::ClipHandle::lost) reports a clip it dropped.
    pub fn apply_patch(&self) -> Result<()> {
        self.require_imperfect()?;
        self.apply_patch_send()
    }

    /// The `PATCH` APPLY send with no opt-in pre-check, so the async wrapper can gate on the async query path.
    pub(crate) fn apply_patch_send(&self) -> Result<()> {
        self.link.restart_watch().expect_represent();
        self.link.send(FrameType::Patch, &patch_apply_payload())
    }

    /// `PATCH` CLEAR (§3.14): erase this device's stored set (the last attached one's when unplugged).
    /// A clone serving patches re-presents unpatched (one unplug/replug to the game PC), which releases
    /// the session as [`apply_patch`](Device::apply_patch) does, and the crate re-sends it the same way.
    pub fn clear_patch(&self) -> Result<()> {
        self.link.restart_watch().expect_represent();
        self.link.send(FrameType::Patch, &patch_clear_payload())
    }

    /// `QUERY(PATCHES)` → [`PatchSet`] (§4.17): the stored patch set and its apply state (applied,
    /// pending, refused, full), a row per patch without its bytes. Read one patch in
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
