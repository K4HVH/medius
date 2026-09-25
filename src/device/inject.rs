use crate::error::Result;
use crate::protocol::command::inject_payload;
use crate::protocol::{FrameType, RST_F_NVS};
use crate::types::{Action, Usage};

use super::Device;

impl Device {
    /// `INJECT`: set a momentary-usage override for any input class (button, key, or media).
    pub fn inject(&self, usage: impl Into<Usage>, action: Action) -> Result<()> {
        let u = usage.into();
        // Serialised against a recovery's re-send, which would otherwise land a stale press after a release.
        let _serial = self.link.reassert_guard();
        self.link.desired().lock().apply(u, action);
        let (class, id) = u.class_id();
        self.link.send(
            FrameType::Inject,
            &inject_payload(class, id, action.as_u8()),
        )
    }

    /// Press (force down) any usage: a button, key, or media usage.
    pub fn press(&self, usage: impl Into<Usage>) -> Result<()> {
        self.inject(usage, Action::Press)
    }

    /// Soft-release any usage; clears our injected press, a physical hold is left intact.
    pub fn release(&self, usage: impl Into<Usage>) -> Result<()> {
        self.inject(usage, Action::SoftRelease)
    }

    /// Force-release any usage; forces it inactive, masking a physical hold too.
    pub fn force_release(&self, usage: impl Into<Usage>) -> Result<()> {
        self.inject(usage, Action::ForceRelease)
    }

    /// `RESET`: return to pure passthrough, clearing injection and ending any open catch stream.
    pub fn reset(&self) -> Result<()> {
        self.reset_frame(&[0])
    }

    /// `RESET` carrying its NVS flag: the release above, and then the
    /// box erases its persistent store and reboots, returning at its defaults under its MAC-derived
    /// name. Clears the box's name, every option, and everything the box has learned about the
    /// devices it has seen, including any descriptor patch set.
    /// The box goes quiet while it reboots: the control port stays enumerated, so
    /// queries time out rather than the link dropping. Poll one until it answers.
    pub fn factory_reset(&self) -> Result<()> {
        self.reset_frame(&[RST_F_NVS])
    }

    // Both resets release the same session state here, so the flag only ever adds the box's side.
    // The byte is always sent: the box requires it, as it does every other command's payload.
    fn reset_frame(&self, payload: &[u8]) -> Result<()> {
        let _serial = self.link.reassert_guard();
        self.link.desired().lock().clear();
        self.link.catch_disconnect_all_locked();
        self.link.send(FrameType::Reset, payload)
    }
}
