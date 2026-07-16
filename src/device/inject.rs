use crate::error::Result;
use crate::protocol::FrameType;
use crate::protocol::command::inject_payload;
use crate::types::{Action, Motion, Usage};

use super::Device;

impl Device {
    /// `INJECT` — set a momentary-usage override for any input class (button, key, or media). One verb
    /// for every class; [`press`](Device::press) / [`release`](Device::release) /
    /// [`force_release`](Device::force_release) are thin wrappers over it.
    pub fn inject(&self, usage: impl Into<Usage>, action: Action) -> Result<()> {
        let u = usage.into();
        self.link.desired().lock().apply(u, action);
        let (class, id) = u.class_id();
        self.link.send(
            FrameType::Inject,
            &inject_payload(class, id, action.as_u8()),
        )
    }

    /// Press (force down) any usage — a button, key, or media usage.
    pub fn press(&self, usage: impl Into<Usage>) -> Result<()> {
        self.inject(usage, Action::Press)
    }

    /// Soft-release any usage — clears our injected press; a physical hold is left intact.
    pub fn release(&self, usage: impl Into<Usage>) -> Result<()> {
        self.inject(usage, Action::SoftRelease)
    }

    /// Force-release any usage — forces it inactive, masking a physical hold too.
    pub fn force_release(&self, usage: impl Into<Usage>) -> Result<()> {
        self.inject(usage, Action::ForceRelease)
    }

    /// `MOVE` — drive a relative axis (cursor or wheel). The field-generic verb;
    /// [`move_rel`](Device::move_rel) and [`wheel`](Device::wheel) are thin wrappers over it.
    pub fn move_axis(&self, motion: Motion) -> Result<()> {
        match motion {
            Motion::Cursor { dx, dy } => self.move_rel(dx, dy),
            Motion::Wheel(dz) => self.wheel(dz),
        }
    }

    /// `RESET` — return to pure passthrough immediately. Clears injection and ends any open catch stream
    /// (its [`EventStream`](crate::EventStream) `recv()` returns `Err`), matching the firmware, which drops
    /// every PC-owned state on the same `RESET`.
    pub fn reset(&self) -> Result<()> {
        self.link.desired().lock().clear();
        self.link.catch_disconnect_all();
        self.link.send(FrameType::Reset, &[])
    }
}
