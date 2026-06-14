use crate::error::Result;
use crate::protocol::FrameType;
use crate::types::RebootTarget;

use super::Device;

impl Device {
    /// Re-send every currently held override to re-assert the intended state; no-op while idle.
    pub fn reapply(&self) -> Result<()> {
        self.link.reapply()
    }

    /// Reboot a chip via `REBOOT_DL` with the [`RebootTarget`] byte (fire-and-forget).
    pub fn reboot(&self, target: RebootTarget) -> Result<()> {
        self.link.send(FrameType::RebootDl, &[target.as_u8()])
    }

    /// Best-effort reconnect: rescan by VID/PID, reopen, re-apply held state, bump the counter.
    pub fn reconnect(&self) -> Result<()> {
        self.link.reconnect()
    }
}
