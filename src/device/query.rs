use crate::error::{Error, Result};
use crate::protocol::opcode::{Q_CAPS, Q_HEALTH, Q_MOUSE_INFO, Q_RATE, Q_STATS, Q_VERSION};
use crate::protocol::{Resp, parse_resp};
use crate::types::{Caps, Health, MouseInfo, Rate, Stats, Version};

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
    pub fn query_caps(&self) -> Result<Caps> {
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
}
