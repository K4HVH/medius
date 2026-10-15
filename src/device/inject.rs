use crate::error::Result;
use crate::protocol::command::inject_payload;
use crate::protocol::{FrameType, RST_F_NVS};
use crate::types::{Action, Usage};

use super::Device;

impl Device {
    /// `INJECT`: override a momentary usage of any class (button, key, media).
    pub fn inject(&self, usage: impl Into<Usage>, action: Action) -> Result<()> {
        let u = usage.into();
        // Serialised against recovery re-sends, or a stale press can land after a release.
        let _serial = self.link.reassert_guard();
        self.link.desired().lock().apply(u, action);
        let (class, id) = u.class_id();
        self.link.send(
            FrameType::Inject,
            &inject_payload(class, id, action.as_u8()),
        )
    }

    /// Press (force down) a button, key or media usage.
    pub fn press(&self, usage: impl Into<Usage>) -> Result<()> {
        self.inject(usage, Action::Press)
    }

    /// Soft-release: clear the injected press, leaving a hold on the real device intact.
    pub fn release(&self, usage: impl Into<Usage>) -> Result<()> {
        self.inject(usage, Action::SoftRelease)
    }

    /// Force-release: force the usage inactive, masking a hold on the real device too.
    pub fn force_release(&self, usage: impl Into<Usage>) -> Result<()> {
        self.inject(usage, Action::ForceRelease)
    }

    /// `RESET`: return to passthrough, clearing injection and ending any open catch stream.
    pub fn reset(&self) -> Result<()> {
        self.reset_frame(&[0])
    }

    /// `RESET` with its NVS flag: [`reset`](Self::reset), then the box erases its persistent store
    /// and reboots with defaults and its MAC-derived name. Erases the name, every option and
    /// everything learned about devices seen, descriptor patch sets included. The control port
    /// stays enumerated during the reboot, so queries time out with the link up: poll one until it
    /// replies.
    pub fn factory_reset(&self) -> Result<()> {
        self.reset_frame(&[RST_F_NVS])
    }

    // Both resets clear the same session state here; the NVS flag changes only what the box does.
    // The payload byte is always sent: the box requires a payload on every command.
    fn reset_frame(&self, payload: &[u8]) -> Result<()> {
        let _serial = self.link.reassert_guard();
        self.link.desired().lock().clear();
        self.link.catch_disconnect_all_locked();
        self.link.send(FrameType::Reset, payload)
    }
}
