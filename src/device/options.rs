use std::time::Duration;

use crate::error::Result;
use crate::protocol::FrameType;
use crate::protocol::command::{
    emit_pace_payload, imperfect_payload, move_ride_payload, name_payload,
};
use crate::types::EmitPace;

use super::Device;

/// The max box-name length on the wire, in bytes (printable ASCII), matching the firmware's `CTRL_NAME_MAX`.
pub const NAME_MAX: usize = 32;

/// The emit-rate ceiling in Hz: the firmware clamps a [`EmitPace::Fixed`](crate::EmitPace::Fixed) rate above this down to it (the 1 ms frame-clock limit).
pub const EMIT_MAX_HZ: u16 = 1000;

/// Encode a movement-riding window to the wire `u16` ms: `None` = 0 (off); a non-zero `Some` is at least 1 ms and saturates at `u16::MAX` ms.
pub(crate) fn ride_window_ms(window: Option<Duration>) -> u16 {
    match window {
        None => 0,
        Some(d) => (d.as_millis().min(u16::MAX as u128) as u16).max(1),
    }
}

/// Encode an [`EmitPace`] to the wire `(mode, rate_hz)`: `Fixed(hz)` carries its rate, the other modes send 0.
pub(crate) fn emit_pace_wire(pace: EmitPace) -> (u8, u16) {
    match pace {
        EmitPace::Learned => (0, 0),
        EmitPace::Interval => (1, 0),
        EmitPace::Fixed(hz) => (2, hz),
    }
}

impl Device {
    /// `OPTION(IMPERFECT)`: opt into cloning an over-capacity device (one interface left dead) or back to faithful-only; persisted in NVS.
    pub fn allow_imperfect_clones(&self, allow: bool) -> Result<()> {
        self.link.send(FrameType::Option, &imperfect_payload(allow))
    }

    /// `OPTION(MOVE_RIDE)`: injected motion rides a native motion report seen within `window` (else dropped)
    /// so its density matches the native mouse's; `None` off; persisted in NVS. A single move can override
    /// it ([`MoveTiming::Now`](crate::MoveTiming), [`move_rel_now`](Self::move_rel_now)), and clip playback
    /// bypasses it unless [`ClipHandle::set_ride`](crate::ClipHandle::set_ride) is on.
    pub fn set_movement_riding(&self, window: Option<Duration>) -> Result<()> {
        self.link.send(
            FrameType::Option,
            &move_ride_payload(ride_window_ms(window)),
        )
    }

    /// `OPTION(EMIT)`: pick what paces injected motion ([`EmitPace::Learned`], [`EmitPace::Interval`], or [`EmitPace::Fixed`] Hz capped at [`EMIT_MAX_HZ`]); persisted in NVS.
    pub fn set_emit_pace(&self, pace: EmitPace) -> Result<()> {
        let (mode, hz) = emit_pace_wire(pace);
        self.link
            .send(FrameType::Option, &emit_pace_payload(mode, hz))
    }

    /// `OPTION(NAME)`: set the box's persistent name (leading printable-ASCII run, capped at [`NAME_MAX`] bytes); read it back off [`query_version`](Device::query_version). Persisted in NVS.
    pub fn set_name(&self, name: &str) -> Result<()> {
        self.link.send(FrameType::Option, &name_payload(name))
    }

    /// `OPTION(NAME)` with an empty value: clear the custom name, reverting the box to its synthesized `Medius-XXXX` default.
    pub fn clear_name(&self) -> Result<()> {
        self.set_name("")
    }
}
