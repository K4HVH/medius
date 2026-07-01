use std::time::Duration;

use crate::error::Result;
use crate::protocol::FrameType;
use crate::protocol::command::{emit_pace_payload, imperfect_payload, move_ride_payload};
use crate::types::EmitPace;

use super::Device;

/// Encode a movement-riding window to the wire `u16` ms: `None` = 0 (off); a non-zero `Some` is at least
/// 1 ms (so a sub-millisecond window never silently rounds down to off) and saturates at `u16::MAX` ms.
/// Shared by the device setter and the `mock` box so the two can't drift.
pub(crate) fn ride_window_ms(window: Option<Duration>) -> u16 {
    match window {
        None => 0,
        Some(d) => (d.as_millis().min(u16::MAX as u128) as u16).max(1),
    }
}

/// Encode an [`EmitPace`] to the wire `(mode, rate_hz)`: `Fixed(hz)` carries its rate, the other modes
/// send 0. Shared by the device setter and the `mock` box so the two can't drift.
pub(crate) fn emit_pace_wire(pace: EmitPace) -> (u8, u16) {
    match pace {
        EmitPace::Learned => (0, 0),
        EmitPace::Interval => (1, 0),
        EmitPace::Fixed(hz) => (2, hz),
    }
}

impl Device {
    /// `OPTION(IMPERFECT)` — opt into cloning an over-capacity device (one interface left dead) or back to
    /// faithful-only; persisted in NVS. When the value changes for an *attached over-capacity* device the
    /// box reboots itself to re-apply it; a normal device (≤5 IN endpoints) is never over-capacity, so the
    /// toggle has no effect on it and no reboot. Fire-and-forget.
    pub fn allow_imperfect_clones(&self, allow: bool) -> Result<()> {
        self.link.send(FrameType::Option, &imperfect_payload(allow))
    }

    /// `OPTION(MOVE_RIDE)` — movement riding. `Some(window)` makes injected cursor/wheel motion ride only a
    /// native cursor-motion report seen within `window`: the box emits no synthetic motion frame, and motion
    /// left unridden past `window` is dropped (never dumped on the next move), so injected motion's report
    /// density matches the native mouse's. `None` turns it off. The window rounds to whole milliseconds (a
    /// non-zero `Some` is at least 1 ms) and clamps to `u16::MAX` ms; persisted in NVS. Fire-and-forget.
    pub fn set_movement_riding(&self, window: Option<Duration>) -> Result<()> {
        self.link.send(
            FrameType::Option,
            &move_ride_payload(ride_window_ms(window)),
        )
    }

    /// `OPTION(EMIT)` — pick what paces injected motion: [`EmitPace::Learned`] (default — the box paces
    /// to the mouse's learnt report rate), [`EmitPace::Interval`] (follow the cloned mouse's `bInterval`
    /// poll rate), or [`EmitPace::Fixed`] at a rate in Hz (snapped to `1000/n`, capped 1 kHz). This raises
    /// the emit-rate ceiling only — idle stays emit-when-pending — so a host that models report density
    /// itself stops the box re-pacing its stream. Persisted in NVS; no reboot. Fire-and-forget.
    pub fn set_emit_pace(&self, pace: EmitPace) -> Result<()> {
        let (mode, hz) = emit_pace_wire(pace);
        self.link
            .send(FrameType::Option, &emit_pace_payload(mode, hz))
    }
}
