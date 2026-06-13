use std::time::Duration;

use crate::error::{Error, Result};
use crate::protocol::command::query_payload;
use crate::protocol::opcode::{Q_HEALTH, Q_VERSION};
use crate::protocol::{FrameType, Resp, parse_resp};
use crate::types::{Health, Version};

use super::Device;

impl Device {
    pub(crate) fn query(&self, what: u8) -> Result<Vec<u8>> {
        self.query_timeout(what, self.query_timeout_default())
    }

    pub(crate) fn query_timeout(&self, what: u8, timeout: Duration) -> Result<Vec<u8>> {
        let (seq, gen_id, rx) = self.register_query(what)?;

        match rx.recv_timeout(timeout) {
            Ok(payload) => {
                trace_event!(
                    target: "medius::device",
                    tracing::Level::DEBUG,
                    selector = what,
                    seq,
                    resp_len = payload.len(),
                    "query resolved",
                );
                Ok(payload)
            }
            Err(_) => {
                self.cancel_query(seq, gen_id);
                trace_event!(
                    target: "medius::device",
                    tracing::Level::WARN,
                    selector = what,
                    seq,
                    "query timed out",
                );
                Err(Error::QueryTimeout)
            }
        }
    }

    pub(crate) fn register_query(&self, what: u8) -> Result<(u8, u64, flume::Receiver<Vec<u8>>)> {
        let (seq, gen_id, rx) = self.register_pending(what);

        if let Err(e) = self.send_with_seq(seq, FrameType::Query, &query_payload(what)) {
            self.cancel_query(seq, gen_id);
            return Err(e);
        }
        Ok((seq, gen_id, rx))
    }

    /// Query the box version.
    pub fn query_version(&self) -> Result<Version> {
        let payload = self.query(Q_VERSION)?;
        match parse_resp(&payload) {
            Some(Resp::Version(v)) => Ok(v),
            _ => Err(Error::NoReply),
        }
    }

    /// Query the box health flags.
    pub fn query_health(&self) -> Result<Health> {
        let payload = self.query(Q_HEALTH)?;
        match parse_resp(&payload) {
            Some(Resp::Health(h)) => Ok(h),
            _ => Err(Error::NoReply),
        }
    }
}
