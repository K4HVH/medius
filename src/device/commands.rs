//! Fire-and-go command methods (§3) — the primary `&self` control surface.
//!
//! Each method encodes one payload (via [`crate::protocol::command`]) and fires it with a fresh `SEQ`
//! (§2.1: no ACK, no wait). Button commands also update the host's `DesiredState` for the
//! keepalive/reconnect reconcile.
//!
//! Lock ordering: `desired` is updated and released before the send takes the write lock (never two
//! locks at once). The benign one-frame window where `desired` leads the box self-heals via reconcile.

use crate::error::Result;
use crate::protocol::FrameType;
use crate::protocol::command::{button_payload, move_payload, wheel_payload};
use crate::types::{Button, ButtonAction};

use super::Device;

impl Device {
    /// `MOVE` — relative cursor movement (§3.1). `+dx` right, `+dy` down; full `i16`, no clamp (the
    /// firmware clamps to the clone's field width with carry).
    ///
    /// Named `move_rel` because `move` is a reserved keyword.
    pub fn move_rel(&self, dx: i16, dy: i16) -> Result<()> {
        self.send(FrameType::Move, &move_payload(dx, dy))
    }

    /// `WHEEL` — vertical scroll (§3.2). `+` up, `−` down; full `i16`, no clamp.
    pub fn wheel(&self, delta: i16) -> Result<()> {
        self.send(FrameType::Wheel, &wheel_payload(delta))
    }

    /// `BUTTON` — set an injection override for one button (§3.3). Records the intent in `DesiredState`
    /// (press/force hold; soft-release clears it), then fires the frame.
    pub fn button(&self, button: Button, action: ButtonAction) -> Result<()> {
        // Release the desired lock BEFORE sending (never hold two locks).
        self.desired().lock().apply(button, action);
        self.send(
            FrameType::Button,
            &button_payload(button.as_id(), action.as_u8()),
        )
    }

    /// Press (hold down) a button — `BUTTON(press)` (§3.3). Forces the bit set regardless of physical
    /// state until released.
    pub fn press(&self, button: Button) -> Result<()> {
        self.button(button, ButtonAction::Press)
    }

    /// Soft-release a button — `BUTTON(soft-release)` (§3.3). Clears *our* injected press; a physical
    /// hold is left intact. (For the safety-authority release that masks a physical hold too, use
    /// [`force_release`](Device::force_release).)
    pub fn soft_release(&self, button: Button) -> Result<()> {
        self.button(button, ButtonAction::SoftRelease)
    }

    /// Force-release a button — `BUTTON(force-release)` (§3.3). Forces the bit clear, masking a
    /// physical hold too (the safety-authority release).
    pub fn force_release(&self, button: Button) -> Result<()> {
        self.button(button, ButtonAction::ForceRelease)
    }

    /// `RESET` — return to pure passthrough immediately (§3.4). Clears every held override in the
    /// host's `DesiredState` to match the box.
    pub fn reset(&self) -> Result<()> {
        self.desired().lock().clear();
        self.send(FrameType::Reset, &[])
    }
}
