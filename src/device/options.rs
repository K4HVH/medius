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

/// The emit-rate ceiling in Hz: the firmware clamps a [`EmitPace::Fixed`](crate::EmitPace::Fixed) rate
/// above this down to it (the 1 ms frame-clock limit). Exposed so a caller can pre-check a rate without
/// duplicating the firmware's number. A movement-ride window (`set_movement_riding`) has no such constant,
/// its range is simply 0 (off) to `u16::MAX` ms.
pub const EMIT_MAX_HZ: u16 = 1000;

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
    /// poll rate), or [`EmitPace::Fixed`] at a rate in Hz (snapped to `1000/n`, capped at [`EMIT_MAX_HZ`]). This raises
    /// the emit-rate ceiling only — idle stays emit-when-pending — so a host that models report density
    /// itself stops the box re-pacing its stream. Persisted in NVS; no reboot. Fire-and-forget.
    pub fn set_emit_pace(&self, pace: EmitPace) -> Result<()> {
        let (mode, hz) = emit_pace_wire(pace);
        self.link
            .send(FrameType::Option, &emit_pace_payload(mode, hz))
    }

    /// `OPTION(NAME)` — set the box's persistent human-readable name, its readable partner to the MAC and
    /// what a multi-box picker shows. Like the other setters this sends the value and lets the box own the
    /// rules: the firmware keeps the leading printable-ASCII run of `name`, capped at [`NAME_MAX`] bytes,
    /// so anything past that (or a non-printable byte) is dropped, and an empty `name` clears it. Persisted
    /// in NVS, no reboot. Read it back off [`query_version`](Device::query_version)
    /// ([`Version::name`](crate::Version::name)), not a `QUERY(OPTIONS)`: the name rides every
    /// `RESP(VERSION)` and the boot hello. Fire-and-forget.
    pub fn set_name(&self, name: &str) -> Result<()> {
        self.link.send(FrameType::Option, &name_payload(name))
    }

    /// `OPTION(NAME)` with an empty value — clear the custom name, reverting the box to its synthesized
    /// `Medius-XXXX` default. Fire-and-forget.
    pub fn clear_name(&self) -> Result<()> {
        self.link.send(FrameType::Option, &name_payload(""))
    }
}
