use crate::error::Result;
use crate::protocol::FrameType;
use crate::protocol::command::button_payload;
use crate::types::{Action, Button};

use super::Device;

impl Device {
    /// `BUTTON` — set an injection override for one button.
    pub fn button(&self, button: Button, action: Action) -> Result<()> {
        self.link.desired().lock().apply(button, action);
        self.link.send(
            FrameType::Button,
            &button_payload(button.as_id(), action.as_u8()),
        )
    }

    /// Press (hold down) a button.
    pub fn press(&self, button: Button) -> Result<()> {
        self.button(button, Action::Press)
    }

    /// Soft-release a button — clears our injected press; a physical hold is left intact.
    pub fn soft_release(&self, button: Button) -> Result<()> {
        self.button(button, Action::SoftRelease)
    }

    /// Force-release a button — forces the bit clear, masking a physical hold too.
    pub fn force_release(&self, button: Button) -> Result<()> {
        self.button(button, Action::ForceRelease)
    }

    /// `RESET` — return to pure passthrough immediately. Clears injection and ends any open catch
    /// stream (its [`EventStream`](crate::EventStream) `recv()` returns `Err`), matching the firmware,
    /// which drops every PC-owned state on the same `RESET`.
    pub fn reset(&self) -> Result<()> {
        self.link.desired().lock().clear();
        self.link.catch_disconnect_all();
        self.link.send(FrameType::Reset, &[])
    }
}
