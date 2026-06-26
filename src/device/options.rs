use std::time::Duration;

use crate::error::Result;
use crate::protocol::FrameType;
use crate::protocol::command::{imperfect_payload, move_ride_payload};

use super::Device;

impl Device {
    /// `OPTION(IMPERFECT)` — opt into cloning an over-capacity device (one interface left dead) or back to
    /// faithful-only; persisted in NVS, and the box reboots itself to re-apply it. Fire-and-forget.
    pub fn allow_imperfect_clones(&self, allow: bool) -> Result<()> {
        self.link.send(FrameType::Option, &imperfect_payload(allow))
    }

    /// `OPTION(MOVE_RIDE)` — movement riding. `Some(window)` makes injected cursor/wheel motion ride only a
    /// native cursor-motion report seen within `window`: the box emits no synthetic motion frame, and motion
    /// left unridden past `window` is dropped (never dumped on the next move), so injected motion's report
    /// density matches the native mouse's. `None` turns it off. The window rounds to whole milliseconds (a
    /// non-zero `Some` is at least 1 ms) and clamps to `u16::MAX` ms; persisted in NVS. Fire-and-forget.
    pub fn set_movement_riding(&self, window: Option<Duration>) -> Result<()> {
        let ms = match window {
            None => 0,
            Some(d) => (d.as_millis().min(u16::MAX as u128) as u16).max(1),
        };
        self.link.send(FrameType::Option, &move_ride_payload(ms))
    }
}
