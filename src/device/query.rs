use std::time::Duration;

use crate::error::{Error, Result};
use crate::protocol::opcode::{
    OPT_BEARING, OPT_EMIT, OPT_IMPERFECT, OPT_MOVE_RIDE, OPT_RENDER, Q_CAPS, Q_CATCH,
    Q_DEVICE_INFO, Q_HEALTH, Q_LOCKS, Q_RATE, Q_STATS, Q_VERSION,
};
use crate::protocol::{Resp, parse_resp};
use crate::types::{
    Bearing, Caps, CatchState, DeviceInfo, EmitPaceStatus, Health, ImperfectStatus, Locks, Rate,
    RenderStatus, Stats, Version,
};

use super::Device;

impl Device {
    /// Query the box version.
    pub fn query_version(&self) -> Result<Version> {
        let payload = self.link.query(Q_VERSION)?;
        match parse_resp(&payload) {
            Some(Resp::Version(v)) => Ok(v),
            _ => Err(Error::NoReply),
        }
    }

    /// Query the box health flags.
    pub fn query_health(&self) -> Result<Health> {
        let payload = self.link.query(Q_HEALTH)?;
        match parse_resp(&payload) {
            Some(Resp::Health(h)) => Ok(h),
            _ => Err(Error::NoReply),
        }
    }

    /// Query the cloned device's USB identity (§4.3).
    pub fn device_info(&self) -> Result<DeviceInfo> {
        let payload = self.link.query(Q_DEVICE_INFO)?;
        match parse_resp(&payload) {
            Some(Resp::DeviceInfo(m)) => Ok(m),
            _ => Err(Error::NoReply),
        }
    }

    /// Query the cloned device's semantic capabilities (§4.4).
    pub fn caps(&self) -> Result<Caps> {
        let payload = self.link.query(Q_CAPS)?;
        match parse_resp(&payload) {
            Some(Resp::Caps(c)) => Ok(c),
            _ => Err(Error::NoReply),
        }
    }

    /// Query the live native report rate and clone poll period (§4.5).
    pub fn query_rate(&self) -> Result<Rate> {
        let payload = self.link.query(Q_RATE)?;
        match parse_resp(&payload) {
            Some(Resp::Rate(r)) => Ok(r),
            _ => Err(Error::NoReply),
        }
    }

    /// Query the box's delivery/telemetry counters (§4.6).
    pub fn query_stats(&self) -> Result<Stats> {
        let payload = self.link.query(Q_STATS)?;
        match parse_resp(&payload) {
            Some(Resp::Stats(s)) => Ok(s),
            _ => Err(Error::NoReply),
        }
    }

    /// Query the active lock bitmask (§4.8).
    pub fn query_locks(&self) -> Result<Locks> {
        let payload = self.link.query(Q_LOCKS)?;
        match parse_resp(&payload) {
            Some(Resp::Locks(l)) => Ok(l),
            _ => Err(Error::NoReply),
        }
    }

    /// Query the active catch subscription mask and box-side dropped-event count (§4.9).
    pub fn query_catch(&self) -> Result<CatchState> {
        let payload = self.link.query(Q_CATCH)?;
        match parse_resp(&payload) {
            Some(Resp::Catch(c)) => Ok(c),
            _ => Err(Error::NoReply),
        }
    }

    /// Query the imperfect-clone opt-in and over-capacity status (§4.14).
    pub fn query_imperfect(&self) -> Result<ImperfectStatus> {
        let payload = self.link.query_option(OPT_IMPERFECT)?;
        match parse_resp(&payload) {
            Some(Resp::Imperfect(i)) => Ok(i),
            _ => Err(Error::NoReply),
        }
    }

    /// Query the movement-riding window (§4.14); `None` = off.
    pub fn query_movement_riding(&self) -> Result<Option<Duration>> {
        let payload = self.link.query_option(OPT_MOVE_RIDE)?;
        match parse_resp(&payload) {
            Some(Resp::MovementRiding(w)) => Ok(w),
            _ => Err(Error::NoReply),
        }
    }

    /// Query the bearing (§4.14): the window `Direction::With`/`Against` are held over, and how it is read.
    pub fn query_bearing(&self) -> Result<Bearing> {
        let payload = self.link.query_option(OPT_BEARING)?;
        match parse_resp(&payload) {
            Some(Resp::Bearing(b)) => Ok(b),
            _ => Err(Error::NoReply),
        }
    }

    /// Query the emit-rate pacing mode and the rate in effect (§4.14).
    pub fn query_emit_pace(&self) -> Result<EmitPaceStatus> {
        let payload = self.link.query_option(OPT_EMIT)?;
        match parse_resp(&payload) {
            Some(Resp::EmitPace(s)) => Ok(s),
            _ => Err(Error::NoReply),
        }
    }

    /// Query what motion is rendered with, whether the device's own goes through it, and whether a
    /// profile has armed (§4.14).
    pub fn query_render(&self) -> Result<RenderStatus> {
        let payload = self.link.query_option(OPT_RENDER)?;
        match parse_resp(&payload) {
            Some(Resp::Render(s)) => Ok(s),
            _ => Err(Error::NoReply),
        }
    }
}
