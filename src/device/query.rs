use crate::error::{Error, Result};
use crate::protocol::opcode::{Q_HEALTH, Q_VERSION};
use crate::protocol::{Resp, parse_resp};
use crate::types::{Health, Version};

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
}
