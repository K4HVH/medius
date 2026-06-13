//! SEQ-correlated queries (§3.5 / §4.1) — the only request/response exchange.
//!
//! A query reserves a free `SEQ`, registers a bounded(1) `flume` one-shot under it in `pending`, sends
//! `QUERY(what)` with the same `SEQ`, and blocks on the one-shot until the reader routes the matching
//! `RESP` — or the timeout elapses, on which the reserved `SEQ` is removed so `pending` never leaks.
//! The async wrapper `.recv_async().await`s the same one-shot, so both paths share one channel.

use std::time::Duration;

use crate::error::{Error, Result};
use crate::protocol::command::query_payload;
use crate::protocol::opcode::{Q_HEALTH, Q_VERSION};
use crate::protocol::{FrameType, Resp, parse_resp};
use crate::types::{Health, Version};

use super::Device;

impl Device {
    /// Send `QUERY(what)` and block for the correlated `RESP` payload, at the device's default timeout.
    /// Returns the raw payload; [`query_version`](Device::query_version) /
    /// [`query_health`](Device::query_health) parse it.
    ///
    /// # Errors
    /// - [`Error::QueryTimeout`] if no `RESP` arrives within the timeout (the waiter is then removed).
    /// - [`Error::FrameTooLong`] / [`Error::Io`] from the underlying send.
    pub(crate) fn query(&self, what: u8) -> Result<Vec<u8>> {
        self.query_timeout(what, self.query_timeout_default())
    }

    /// [`query`](Device::query) with an explicit timeout.
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
                // Timed out (or sender dropped). Gen-checked cancel removes only our waiter, never a
                // newer query that reused this wire SEQ meanwhile.
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

    /// Reserve a free `SEQ`, register the generation-tagged one-shot under it, send `QUERY(what)`, and
    /// return `(seq, gen, receiver)` to await.
    ///
    /// Shared by the sync path ([`query_timeout`](Device::query_timeout)) and the async wrapper, so
    /// there is one correlation mechanism and one flume one-shot (§5). The caller MUST
    /// `cancel_query(seq, gen)` if it gives up (both timeout paths do).
    pub(crate) fn register_query(&self, what: u8) -> Result<(u8, u64, flume::Receiver<Vec<u8>>)> {
        // Reserve the waiter BEFORE sending, so a fast RESP can never arrive before it exists. The
        // waiter records `what` so only a RESP echoing the same selector fulfils it (§4.1).
        let (seq, gen_id, rx) = self.register_pending(what);

        // Send with the SAME seq the waiter is keyed on; on failure drop it (gen-checked).
        if let Err(e) = self.send_with_seq(seq, FrameType::Query, &query_payload(what)) {
            self.cancel_query(seq, gen_id);
            return Err(e);
        }
        Ok((seq, gen_id, rx))
    }

    /// Query the box version (§4.1).
    ///
    /// # Errors
    /// [`Error::QueryTimeout`] on no reply; [`Error::NoReply`] if the reply is not a parseable VERSION.
    pub fn query_version(&self) -> Result<Version> {
        let payload = self.query(Q_VERSION)?;
        match parse_resp(&payload) {
            Some(Resp::Version(v)) => Ok(v),
            _ => Err(Error::NoReply),
        }
    }

    /// Query the box health flags (§4.2).
    ///
    /// # Errors
    /// [`Error::QueryTimeout`] on no reply; [`Error::NoReply`] if the reply is not a parseable HEALTH.
    pub fn query_health(&self) -> Result<Health> {
        let payload = self.query(Q_HEALTH)?;
        match parse_resp(&payload) {
            Some(Resp::Health(h)) => Ok(h),
            _ => Err(Error::NoReply),
        }
    }
}
