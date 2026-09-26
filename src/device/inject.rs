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

    /// Soft-release: clear the injected press, leaving a physical hold intact.
    pub fn release(&self, usage: impl Into<Usage>) -> Result<()> {
        self.inject(usage, Action::SoftRelease)
    }

    /// Force-release: force the usage inactive, masking a physical hold too.
    pub fn force_release(&self, usage: impl Into<Usage>) -> Result<()> {
        self.inject(usage, Action::ForceRelease)
    }

    /// `RESET`: return to passthrough, clearing injection and ending any open catch stream.
    pub fn reset(&self) -> Result<()> {
        self.reset_frame(&[0])
    }

    /// `RESET` with its NVS flag: [`reset`](Self::reset), then the box erases its persistent store and
    /// reboots at its defaults under its MAC-derived name. Erases the name, every option and
    /// everything learned about devices seen, including descriptor patch sets. While it reboots the
    /// control port stays enumerated, so queries time out with the link up; poll one until it replies.
    pub fn factory_reset(&self) -> Result<()> {
        self.reset_frame(&[RST_F_NVS])
    }

    // Both resets release the same session state here; the flag adds only the box's side. The payload
    // byte is always sent: the box requires it, as it does every command's payload.
    fn reset_frame(&self, payload: &[u8]) -> Result<()> {
        let _serial = self.link.reassert_guard();
        self.link.desired().lock().clear();
        self.link.catch_disconnect_all_locked();
        self.link.send(FrameType::Reset, payload)
    }
}
