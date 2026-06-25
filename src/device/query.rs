use crate::error::{Error, Result};
use crate::protocol::opcode::{
    Q_CAPS, Q_CATCH, Q_HEALTH, Q_KBD_CAPS, Q_LOCKS, Q_MOUSE_INFO, Q_RATE, Q_STATS, Q_VERSION,
};
use crate::protocol::{Resp, parse_resp};
use crate::types::{
    CatchState, Health, KbdCaps, Locks, MouseCaps, MouseInfo, Rate, Stats, Version,
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

    /// Query the cloned mouse's USB identity (vid/pid/bcd + serial/BOS flags, §4.3).
    pub fn query_mouse_info(&self) -> Result<MouseInfo> {
        let payload = self.link.query(Q_MOUSE_INFO)?;
        match parse_resp(&payload) {
            Some(Resp::MouseInfo(m)) => Ok(m),
            _ => Err(Error::NoReply),
        }
    }

    /// Query the emulated mouse's semantic capabilities (button count, axes, interfaces, §4.4).
    pub fn query_mouse_caps(&self) -> Result<MouseCaps> {
        let payload = self.link.query(Q_CAPS)?;
        match parse_resp(&payload) {
            Some(Resp::MouseCaps(c)) => Ok(c),
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

    /// Query the cloned keyboard's semantic capabilities (key count, NKRO, Consumer/System, §4.11).
    /// All-zero/false when no keyboard is bound — check [`Health::kbd_attached`](crate::Health) first.
    pub fn query_kbd_caps(&self) -> Result<KbdCaps> {
        let payload = self.link.query(Q_KBD_CAPS)?;
        match parse_resp(&payload) {
            Some(Resp::KbdCaps(k)) => Ok(k),
            _ => Err(Error::NoReply),
        }
    }
}
