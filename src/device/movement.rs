use crate::error::Result;
use crate::protocol::FrameType;
use crate::protocol::command::{move_payload, wheel_payload};

use super::Device;

impl Device {
    /// `MOVE` — relative cursor movement; full `i16`, no clamp.
    pub fn move_rel(&self, dx: i16, dy: i16) -> Result<()> {
        self.link.send(FrameType::Move, &move_payload(dx, dy))
    }

    /// `WHEEL` — vertical scroll; full `i16`, no clamp.
    pub fn wheel(&self, delta: i16) -> Result<()> {
        self.link.send(FrameType::Wheel, &wheel_payload(delta))
    }
}
