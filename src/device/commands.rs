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

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::protocol::{DecodedFrame, FrameDecoder, FrameType};
    use crate::transport::mock::MockTransport;

    use super::*;

    fn device_with_mock() -> (Device, Arc<MockTransport>) {
        let mock = Arc::new(MockTransport::new());
        let device = Device::from_transport(mock.clone());
        (device, mock)
    }

    fn written_frames(mock: &MockTransport) -> Vec<DecodedFrame> {
        FrameDecoder::new().feed_collect(&mock.written())
    }

    #[test]
    fn move_rel_emits_exact_frame() {
        let (device, mock) = device_with_mock();
        device.move_rel(-1, 256).unwrap();
        let frames = written_frames(&mock);
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].ty, FrameType::Move);
        assert_eq!(frames[0].payload, vec![0xFF, 0xFF, 0x00, 0x01]);
    }

    #[test]
    fn wheel_emits_exact_frame() {
        let (device, mock) = device_with_mock();
        device.wheel(-3).unwrap();
        let frames = written_frames(&mock);
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].ty, FrameType::Wheel);
        assert_eq!(frames[0].payload, vec![0xFD, 0xFF]);
    }

    #[test]
    fn press_emits_button_press_payload() {
        let (device, mock) = device_with_mock();
        device.press(Button::Left).unwrap();
        let frames = written_frames(&mock);
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].ty, FrameType::Button);
        // [id=Left(0)][action=press(1)]
        assert_eq!(frames[0].payload, vec![0, 1]);
    }

    #[test]
    fn soft_release_is_soft_release() {
        let (device, mock) = device_with_mock();
        device.soft_release(Button::Right).unwrap();
        let frames = written_frames(&mock);
        assert_eq!(frames[0].ty, FrameType::Button);
        // [id=Right(1)][action=soft-release(0)]
        assert_eq!(frames[0].payload, vec![1, 0]);
    }

    #[test]
    fn force_release_emits_force_action() {
        let (device, mock) = device_with_mock();
        device.force_release(Button::Side2).unwrap();
        let frames = written_frames(&mock);
        assert_eq!(frames[0].ty, FrameType::Button);
        // [id=Side2(4)][action=force-release(2)]
        assert_eq!(frames[0].payload, vec![4, 2]);
    }

    #[test]
    fn reset_emits_empty_payload_and_clears_desired() {
        let (device, mock) = device_with_mock();
        device.press(Button::Left).unwrap();
        assert!(!device.desired().lock().is_idle());

        device.reset().unwrap();
        assert!(device.desired().lock().is_idle());

        let frames = written_frames(&mock);
        let reset = frames.iter().find(|f| f.ty == FrameType::Reset).unwrap();
        assert!(reset.payload.is_empty());
    }

    #[test]
    fn press_updates_desired_state() {
        let (device, _mock) = device_with_mock();
        device.press(Button::Middle).unwrap();
        let held: Vec<_> = device.desired().lock().held().collect();
        assert_eq!(held, vec![(Button::Middle, ButtonAction::Press)]);
    }

    #[test]
    fn soft_release_clears_desired_override() {
        let (device, _mock) = device_with_mock();
        device.press(Button::Middle).unwrap();
        device.soft_release(Button::Middle).unwrap();
        assert!(device.desired().lock().is_idle());
    }

    #[test]
    fn each_command_uses_a_fresh_rolling_seq() {
        let (device, mock) = device_with_mock();
        device.move_rel(1, 0).unwrap();
        device.move_rel(2, 0).unwrap();
        device.move_rel(3, 0).unwrap();
        let frames = written_frames(&mock);
        let seqs: Vec<u8> = frames.iter().map(|f| f.seq).collect();
        assert_eq!(seqs, vec![0, 1, 2]);
    }
}
