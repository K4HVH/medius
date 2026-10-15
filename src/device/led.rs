use crate::error::Result;
use crate::protocol::FrameType;
use crate::protocol::command::led_payload;
use crate::types::{LedMode, LedTarget};

use super::Device;

impl Device {
    /// `LED`: override a status LED (off, solid or blink at `level` brightness).
    ///
    /// [`LedMode::Auto`] returns the LED to the box's status display. Fire-and-forget.
    pub fn led(&self, target: LedTarget, mode: LedMode, level: u8) -> Result<()> {
        self.link.send(
            FrameType::Led,
            &led_payload(target.as_u8(), mode.as_u8(), level),
        )
    }
}
