use crate::error::Result;
use crate::protocol::FrameType;
use crate::protocol::command::led_payload;
use crate::types::{LedMode, LedTarget};

use super::Device;

impl Device {
    /// `LED` — override a status LED, or hand it back to the box's status display with
    /// [`LedMode::Auto`]. `level` is brightness 0..=255 (used for solid/blink). Fire-and-forget; an
    /// override reverts to status on control-PC silence, `RESET`, or inter-chip link loss.
    pub fn led(&self, target: LedTarget, mode: LedMode, level: u8) -> Result<()> {
        self.link.send(
            FrameType::Led,
            &led_payload(target.as_u8(), mode.as_u8(), level),
        )
    }
}
