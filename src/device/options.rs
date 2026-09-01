use std::time::Duration;

use crate::error::Result;
use crate::protocol::FrameType;
use crate::protocol::command::{
    bearing_payload, emit_pace_payload, imperfect_payload, move_ride_payload, name_payload,
    render_payload,
};
use crate::types::{BearingMode, EmitPace, RenderMode};

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

/// The bearing window a box boots with, before any host sets one.
pub const BEARING_WINDOW_DEFAULT: Duration = Duration::from_millis(20);

/// Encode an [`EmitPace`] to the wire `(mode, rate_hz)`: `Fixed(hz)` carries its rate, the other paces send 0.
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

    /// `OPTION(EMIT)`: set the pace ([`EmitPace`], the rate ceiling) and the forced wire rate (`force_hz`); persisted in NVS.
    ///
    /// A non-zero `force_hz` re-clones the box to advertise a `bInterval` the device did not (needs
    /// [`allow_imperfect_clones`](Self::allow_imperfect_clones)), snapping to `1000/n` Hz; `Some(0)`/`None` is off.
    pub fn set_emit_pace(&self, pace: EmitPace, force_hz: Option<u16>) -> Result<()> {
        let (mode, hz) = emit_pace_wire(pace);
        self.link.send(
            FrameType::Option,
            &emit_pace_payload(mode, hz, force_hz.unwrap_or(0)),
        )
    }

    /// `OPTION(NAME)`: set the box's persistent name (leading printable-ASCII run, capped at [`NAME_MAX`] bytes); read it back off [`query_version`](Device::query_version). Persisted in NVS.
    pub fn set_name(&self, name: &str) -> Result<()> {
        self.link.send(FrameType::Option, &name_payload(name))
    }

    /// `OPTION(NAME)` with an empty value: clear the custom name, reverting the box to its synthesised `Medius-XXXX` default.
    pub fn clear_name(&self) -> Result<()> {
        self.set_name("")
    }
    /// `OPTION(BEARING)`: how long the direction of the last injected delta stays the thing
    /// [`Direction::With`](crate::Direction) and [`Direction::Against`](crate::Direction) are measured
    /// against, and whether it is read per axis or as one vector; persisted in NVS.
    ///
    /// `None` turns the bearing off, which leaves the relative directions inert whatever their scale;
    /// the absolute ones are unaffected. The box boots at [`BEARING_WINDOW_DEFAULT`] with
    /// [`BearingMode::PerAxis`], so nothing engages until a scale is set.
    ///
    /// The window rounds to whole milliseconds; a non-zero `Some` is at least 1 ms and saturates at
    /// 65535 ms.
    ///
    /// ```no_run
    /// # use medius::{Axis, BearingMode, Device, Direction, Result};
    /// # use std::time::Duration;
    /// # fn main() -> Result<()> {
    /// let device = Device::find()?;
    /// device.set_bearing(Some(Duration::from_millis(20)), BearingMode::PerAxis)?;
    /// device.scale(Axis::X, Direction::Against, 40)?;
    /// # Ok(()) }
    /// ```
    pub fn set_bearing(&self, window: Option<Duration>, mode: BearingMode) -> Result<()> {
        self.link.send(
            FrameType::Option,
            &bearing_payload(ride_window_ms(window), mode.as_u8()),
        )
    }

    /// `OPTION(RENDER)`: set the texture motion is rendered with, and whether the device's own motion is
    /// rendered by the model too rather than relayed; persisted in NVS.
    ///
    /// Rendering adds a small amount of latency, which reaches the mouse's own motion when `full` is
    /// on, so `full` is off by default. Nothing is rendered until the box has learned a profile for the
    /// attached device: until then motion is relayed and injection takes the paced fill
    /// ([`RenderStatus::ready`](crate::RenderStatus)).
    ///
    /// Motion asking for exact timing skips the model: [`move_rel_now`](Self::move_rel_now),
    /// [`flush_motion`](Self::flush_motion) and [`discard_motion`](Self::discard_motion) take the paced
    /// path, and with `full` on the rendered stream ignores
    /// [`set_movement_riding`](Self::set_movement_riding).
    ///
    /// ```no_run
    /// # use medius::{Device, RenderMode, Result};
    /// # fn main() -> Result<()> {
    /// let device = Device::find()?;
    /// device.set_render(RenderMode::Despiked, true)?;
    /// # Ok(()) }
    /// ```
    pub fn set_render(&self, mode: RenderMode, full: bool) -> Result<()> {
        self.link
            .send(FrameType::Option, &render_payload(mode.to_wire(), full))
    }
}
