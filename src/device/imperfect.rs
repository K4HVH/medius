use crate::error::Result;
use crate::protocol::FrameType;
use crate::protocol::command::imperfect_payload;

use super::Device;

impl Device {
    /// `IMPERFECT` — opt into cloning an over-capacity device (one interface left dead), or back to
    /// faithful-only. Persisted in NVS; takes effect on the next clone (re-plug or reboot). Fire-and-forget.
    pub fn set_imperfect_allowed(&self, allow: bool) -> Result<()> {
        self.link
            .send(FrameType::Imperfect, &imperfect_payload(allow))
    }
}
