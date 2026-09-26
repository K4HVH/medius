use crate::error::Result;
use crate::protocol::FrameType;
use crate::types::RebootTarget;

use super::Device;

impl Device {
    /// Re-send every held override; no-op while idle.
    pub fn reapply(&self) -> Result<()> {
        self.link.reapply()
    }

    /// `REBOOT_DL`: reboot the [`RebootTarget`] chip. Fire-and-forget.
    pub fn reboot(&self, target: RebootTarget) -> Result<()> {
        self.link.send(FrameType::RebootDl, &[target.as_u8()])
    }

    /// Best-effort: rescan by VID/PID, reopen, re-apply held state, bump the counter. A device chip
    /// that restarted meanwhile gets the held state once its clone is up.
    /// [`Error::BadProtoVer`](crate::Error::BadProtoVer) if the box replies on another control
    /// protocol (as after a reflash to other firmware); it stays disconnected.
    pub fn reconnect(&self) -> Result<()> {
        self.link.reconnect()
    }
}
