use crate::error::Result;
use crate::protocol::FrameType;
use crate::protocol::command::{move_cursor_payload, move_wheel_payload};

use super::Device;

impl Device {
    /// `MOVE` (cursor) — relative cursor movement; full `i16`, no clamp.
    pub fn move_rel(&self, dx: i16, dy: i16) -> Result<()> {
        self.link
            .send(FrameType::Move, &move_cursor_payload(dx, dy))
    }

    /// `MOVE` (wheel) — vertical scroll; full `i16`, no clamp.
    pub fn wheel(&self, delta: i16) -> Result<()> {
        self.link.send(FrameType::Move, &move_wheel_payload(delta))
    }
}
