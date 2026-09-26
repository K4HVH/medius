use std::time::Duration;

use crate::error::Result;
use crate::protocol::FrameType;
use crate::protocol::command::{
    bearing_payload, emit_pace_payload, imperfect_payload, move_ride_payload, name_payload,
    render_payload, spread_payload,
};
use crate::types::{BearingMode, EmitPace, RenderMode};

use super::Device;

/// Max box-name length in bytes (printable ASCII), the firmware's `CTRL_NAME_MAX`.
pub const NAME_MAX: usize = 32;

/// Emit-rate ceiling in Hz (the 1 ms frame-clock limit); the firmware clamps a higher
/// [`EmitPace::Fixed`](crate::EmitPace::Fixed) rate to it.
pub const EMIT_MAX_HZ: u16 = 1000;

// `None` = 0 (off); a non-zero `Some` is at least 1 ms and saturates at `u16::MAX` ms.
pub(crate) fn ride_window_ms(window: Option<Duration>) -> u16 {
    match window {
        None => 0,
        Some(d) => (d.as_millis().min(u16::MAX as u128) as u16).max(1),
    }
}

/// Bearing window at boot, before any host sets one.
pub const BEARING_WINDOW_DEFAULT: Duration = Duration::from_millis(20);

pub(crate) fn emit_pace_wire(pace: EmitPace) -> (u8, u16) {
    match pace {
        EmitPace::Learned => (0, 0),
        EmitPace::Interval => (1, 0),
        EmitPace::Fixed(hz) => (2, hz),
    }
}

impl Device {
    /// `OPTION(IMPERFECT)`: allow cloning a device the box cannot clone exactly, or return to
    /// faithful-only; persisted in NVS.
    ///
    /// Turning the opt-in off clears the box's rewrite table, so the crate drops its held rules too
    /// (restored if the frame never goes out); otherwise the keepalive would re-assert them once the
    /// opt-in is back on. The box also removes every clip packet trigger that
    /// [consumes](crate::ClipPacketTrigger::consume), and the crate drops those the same way. Turning
    /// the opt-in off concurrently with a `set_rewrite` on another thread is unordered: the rule may
    /// reach the box after the opt-off and be refused.
    ///
    /// A toggle that changes the served patch set presents the clone again, which releases the
    /// session like a device replug; the crate re-sends what it holds once the new clone is up, and
    /// [`ClipHandle::lost`](crate::ClipHandle::lost) reports a dropped clip.
    pub fn allow_imperfect_clones(&self, allow: bool) -> Result<()> {
        self.link.restart_watch().expect_represent();
        if !allow {
            // Serialised against re-asserts like the rewrite mutators; rolled back on a failed send
            // so DesiredState and the box stay in step.
            let _serial = self.link.reassert_guard();
            let (held, consuming) = {
                let mut d = self.link.desired().lock();
                let held = d.held_rewrites();
                d.clear_rewrites();
                (held, d.clip_packet_drop_consuming())
            };
            let sent = self.link.send(FrameType::Option, &imperfect_payload(false));
            if sent.is_err() {
                let mut d = self.link.desired().lock();
                for r in held {
                    d.apply_rewrite(r);
                }
                for t in &consuming {
                    d.clip_packet_bind(t);
                }
            }
            return sent;
        }
        self.link.send(FrameType::Option, &imperfect_payload(true))
    }

    /// `OPTION(MOVE_RIDE)`: injected motion rides a native motion report seen within `window` (else
    /// dropped), so its density matches native; `None` is off; persisted in NVS. A single move can
    /// override it ([`MoveTiming::Now`](crate::MoveTiming), [`move_rel_now`](Self::move_rel_now)).
    /// Clip playback bypasses it unless [`ClipHandle::set_ride`](crate::ClipHandle::set_ride) is on;
    /// while rendering is on with a profile armed, a clip's cursor motion follows it like a rendered
    /// move.
    pub fn set_movement_riding(&self, window: Option<Duration>) -> Result<()> {
        self.link.send(
            FrameType::Option,
            &move_ride_payload(ride_window_ms(window)),
        )
    }

    /// `OPTION(EMIT)`: pace ([`EmitPace`], the rate ceiling) and forced wire rate (`force_hz`);
    /// persisted in NVS.
    ///
    /// A non-zero `force_hz` re-clones with a `bInterval` the device did not advertise, snapped to
    /// `1000/n` Hz (needs [`allow_imperfect_clones`](Self::allow_imperfect_clones)); `Some(0)`/`None`
    /// is off.
    pub fn set_emit_pace(&self, pace: EmitPace, force_hz: Option<u16>) -> Result<()> {
        let (mode, hz) = emit_pace_wire(pace);
        self.link.send(
            FrameType::Option,
            &emit_pace_payload(mode, hz, force_hz.unwrap_or(0)),
        )
    }

    /// `OPTION(NAME)`: box name (leading printable-ASCII run, capped at [`NAME_MAX`] bytes); read
    /// back by [`query_version`](Device::query_version). Persisted in NVS.
    pub fn set_name(&self, name: &str) -> Result<()> {
        self.link.send(FrameType::Option, &name_payload(name))
    }

    /// `OPTION(NAME)` with an empty value: revert to the derived `Medius-XXXX` default.
    pub fn clear_name(&self) -> Result<()> {
        self.set_name("")
    }
    /// `OPTION(BEARING)`: how long the last injected delta's direction stays the reference for
    /// [`Direction::With`](crate::Direction) and [`Direction::Against`](crate::Direction), and whether
    /// it is read per axis or as one vector; persisted in NVS.
    ///
    /// `None` turns the bearing off, leaving the relative directions inert whatever their scale; the
    /// absolute ones are unaffected. The box boots at [`BEARING_WINDOW_DEFAULT`] with
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

    /// `OPTION(RENDER)`: render texture, and whether the model renders native motion too instead of
    /// relaying it; persisted in NVS.
    ///
    /// Rendering adds a little latency, which `full` puts on native motion too; `full` is off by
    /// default. Nothing renders until the box learns a profile for the attached device; until then
    /// motion is relayed and injection takes the paced fill
    /// ([`RenderStatus::ready`](crate::RenderStatus)).
    ///
    /// Motion asking for exact timing skips the model: [`move_rel_now`](Self::move_rel_now),
    /// [`flush_motion`](Self::flush_motion) and [`discard_motion`](Self::discard_motion) take the paced
    /// path. With `full` on, the rendered stream ignores
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

    /// `OPTION(SPREAD)`: percent of the host's command interval an injected delta is released
    /// across; persisted in NVS.
    ///
    /// 0 puts the whole delta on the next emitted report; 100 releases it evenly across one command
    /// interval; above 100 overlaps and is allowed. A loop at the native report rate keeps each
    /// command whole, on its own report.
    ///
    /// The box learns the interval from `MOVE` arrivals and spreads nothing until it has
    /// ([`SpreadStatus::span_us`](crate::SpreadStatus)). Motion asking for exact timing is released
    /// at once: [`move_rel_now`](Self::move_rel_now), [`flush_motion`](Self::flush_motion) and
    /// [`discard_motion`](Self::discard_motion).
    ///
    /// ```no_run
    /// # use medius::{Device, Result};
    /// # fn main() -> Result<()> {
    /// let device = Device::find()?;
    /// device.set_spread(100)?;
    /// # Ok(()) }
    /// ```
    pub fn set_spread(&self, percent: u16) -> Result<()> {
        self.link.send(FrameType::Option, &spread_payload(percent))
    }
}
