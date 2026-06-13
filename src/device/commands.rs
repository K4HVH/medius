use crate::error::Result;
use crate::protocol::FrameType;
use crate::protocol::command::{button_payload, move_payload, wheel_payload};
use crate::types::{Button, ButtonAction};

use super::Device;

impl Device {
    /// `MOVE` — relative cursor movement; full `i16`, no clamp.
    pub fn move_rel(&self, dx: i16, dy: i16) -> Result<()> {
        self.send(FrameType::Move, &move_payload(dx, dy))
    }

    /// `WHEEL` — vertical scroll; full `i16`, no clamp.
    pub fn wheel(&self, delta: i16) -> Result<()> {
        self.send(FrameType::Wheel, &wheel_payload(delta))
    }

    /// `BUTTON` — set an injection override for one button.
    pub fn button(&self, button: Button, action: ButtonAction) -> Result<()> {
        self.desired().lock().apply(button, action);
        self.send(
            FrameType::Button,
            &button_payload(button.as_id(), action.as_u8()),
        )
    }

    /// Press (hold down) a button.
    pub fn press(&self, button: Button) -> Result<()> {
        self.button(button, ButtonAction::Press)
    }

    /// Soft-release a button — clears our injected press; a physical hold is left intact.
    pub fn soft_release(&self, button: Button) -> Result<()> {
        self.button(button, ButtonAction::SoftRelease)
    }

    /// Force-release a button — forces the bit clear, masking a physical hold too.
    pub fn force_release(&self, button: Button) -> Result<()> {
        self.button(button, ButtonAction::ForceRelease)
    }

    /// `RESET` — return to pure passthrough immediately.
    pub fn reset(&self) -> Result<()> {
        self.desired().lock().clear();
        self.send(FrameType::Reset, &[])
    }
}
