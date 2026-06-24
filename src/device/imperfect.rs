use crate::error::Result;
use crate::protocol::FrameType;
use crate::protocol::command::imperfect_payload;

use super::Device;

impl Device {
    /// `IMPERFECT` — opt into cloning an over-capacity device (one interface left dead) or back to faithful-only; persisted in NVS, and the box reboots itself to re-apply it. Fire-and-forget.
    pub fn allow_imperfect_clones(&self, allow: bool) -> Result<()> {
        self.link
            .send(FrameType::Imperfect, &imperfect_payload(allow))
    }
}
