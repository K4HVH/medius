use crate::error::Result;
use crate::types::{Action, Input, Motion};

use super::Device;

impl Device {
    /// `INJECT` — set a momentary-usage override for any input class (button, key, or media). The
    /// field-generic verb; [`press`](Device::press), [`key_down`](Device::key_down), etc. are thin
    /// wrappers over it.
    pub fn inject(&self, input: impl Into<Input>, action: Action) -> Result<()> {
        match input.into() {
            Input::Button(b) => self.button(b, action),
            Input::Key(k) => self.key(k, action),
            Input::Media(m) => self.media(m, action),
        }
    }

    /// `MOVE` — drive a relative axis (cursor or wheel). The field-generic verb;
    /// [`move_rel`](Device::move_rel) and [`wheel`](Device::wheel) are thin wrappers over it.
    pub fn move_axis(&self, motion: Motion) -> Result<()> {
        match motion {
            Motion::Cursor { dx, dy } => self.move_rel(dx, dy),
            Motion::Wheel(dz) => self.wheel(dz),
        }
    }
}
