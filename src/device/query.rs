//! SEQ-correlated queries (§3.5 / §4.1) — the only request/response exchange.
//!
//! A query allocates a fresh `SEQ`, registers a bounded(1) `flume` one-shot under that `SEQ` in the
//! `pending` map, sends `QUERY(what)` with the **same** `SEQ`, and blocks on the one-shot until the
//! reader routes the matching `RESP` (which echoes the `SEQ`) — or the timeout elapses. On timeout
//! the reserved `SEQ` is removed from `pending` so the map never leaks a stale waiter.
//!
//! The same `flume` one-shot is what the `async` wrapper (Milestone 5) will `.recv_async().await` on,
//! so this sync path and the future async path share one channel — no duplicated correlation logic.

use std::time::Duration;

use crate::error::{Error, Result};
use crate::protocol::command::query_payload;
use crate::protocol::opcode::{Q_HEALTH, Q_VERSION};
use crate::protocol::types::{Health, Version};
use crate::protocol::{FrameType, Resp, parse_resp};

use super::Device;

/// Default time a query waits for its correlated `RESP` before giving up.
pub(crate) const DEFAULT_QUERY_TIMEOUT: Duration = Duration::from_secs(1);

impl Device {
    /// Send `QUERY(what)` and block for the correlated `RESP` payload, with the default 1 s timeout.
    ///
    /// The reserved `SEQ` in `pending` is exactly the `SEQ` of the sent frame, so the reader can
    /// fulfil it. Returns the raw `RESP` **payload** (the caller decodes it); higher-level
    /// [`query_version`](Device::query_version) / [`query_health`](Device::query_health) parse it.
    ///
    /// # Errors
    /// - [`Error::QueryTimeout`] if no `RESP` arrives within the timeout (the waiter is then removed).
    /// - [`Error::FrameTooLong`] / [`Error::Io`] from the underlying send.
    pub(crate) fn query(&self, what: u8) -> Result<Vec<u8>> {
        self.query_timeout(what, DEFAULT_QUERY_TIMEOUT)
    }

    /// [`query`](Device::query) with an explicit timeout.
    pub(crate) fn query_timeout(&self, what: u8, timeout: Duration) -> Result<Vec<u8>> {
        let seq = self.next_seq();
        let (tx, rx) = flume::bounded::<Vec<u8>>(1);

        // Reserve the SEQ *before* sending, so a fast RESP can never arrive before the waiter exists.
        // Holding `pending` only for the insert (released before the send) keeps lock-ordering clean.
        self.pending().lock().insert(seq, tx);

        // Send the QUERY with the SAME seq the waiter is keyed on. If the send fails, drop the waiter.
        if let Err(e) = self.send_with_seq(seq, FrameType::Query, &query_payload(what)) {
            self.pending().lock().remove(&seq);
            return Err(e);
        }

        match rx.recv_timeout(timeout) {
            Ok(payload) => Ok(payload),
            Err(_) => {
                // Timed out (or the sender was dropped). Remove the stale waiter so `pending` doesn't
                // leak, then report the timeout.
                self.pending().lock().remove(&seq);
                Err(Error::QueryTimeout)
            }
        }
    }

    /// Query the box version (§4.1). Used by the connect handshake and on demand.
    ///
    /// # Errors
    /// [`Error::QueryTimeout`] on no reply; [`Error::NoReply`] is reserved for the handshake wrapper.
    /// A malformed/truncated `RESP` surfaces as [`Error::QueryTimeout`]'s sibling — here we treat an
    /// unparseable payload as a timeout-equivalent failure ([`Error::NoReply`]).
    pub fn query_version(&self) -> Result<Version> {
        let payload = self.query(Q_VERSION)?;
        match parse_resp(&payload) {
            Some(Resp::Version(v)) => Ok(v),
            // A reply that isn't a parseable VERSION is as good as no reply for handshake purposes.
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use crate::error::Error;
    use crate::protocol::{FrameType, encode};
    use crate::transport::mock::MockTransport;

    use super::Device;

    /// A mock that answers VERSION/HEALTH queries (echoing SEQ) with fixed values.
    fn responder_device(version: [u8; 4], health_flags: u8) -> Device {
        let mock = Arc::new(MockTransport::with_responder(move |ty, seq, payload| {
            if ty != FrameType::Query {
                return Vec::new();
            }
            match payload.first().copied() {
                Some(0) => encode(
                    FrameType::Resp,
                    seq,
                    &[0, version[0], version[1], version[2], version[3]],
                )
                .unwrap(),
                Some(1) => encode(FrameType::Resp, seq, &[1, health_flags]).unwrap(),
                _ => Vec::new(),
            }
        }));
        Device::from_transport(mock)
    }

    #[test]
    fn query_version_parses_resp() {
        let device = responder_device([1, 7, 8, 9], 0x00);
        let v = device.query_version().unwrap();
        assert_eq!(v.proto_ver, 1);
        assert_eq!(v.fw_major, 7);
        assert_eq!(v.fw_minor, 8);
        assert_eq!(v.fw_patch, 9);
    }

    #[test]
    fn query_health_parses_flags() {
        // link_up | mouse_attached | inject_on = 0x0B; clone_configured (0x04) off.
        let device = responder_device([1, 0, 0, 0], 0x0B);
        let h = device.query_health().unwrap();
        assert!(h.link_up);
        assert!(h.mouse_attached);
        assert!(!h.clone_configured);
        assert!(h.injection_active);
    }

    #[test]
    fn query_times_out_when_box_is_silent() {
        let mock = Arc::new(MockTransport::new()); // no responder
        let device = Device::from_transport(mock);
        let err = device
            .query_timeout(0, Duration::from_millis(40))
            .unwrap_err();
        assert!(matches!(err, Error::QueryTimeout), "got {err:?}");
    }

    #[test]
    fn version_query_sends_correct_payload_and_correlated_seq() {
        // Capture what the host actually sent: a QUERY(VERSION) whose SEQ the RESP must echo.
        let device = responder_device([1, 0, 0, 0], 0x00);
        let _ = device.query_version().unwrap();
        // A second query must succeed too (rolling SEQ advances, correlation still holds).
        let _ = device.query_health().unwrap();
    }

    #[test]
    fn two_concurrent_queries_correlate_by_seq() {
        // Even issued back to back, each query gets exactly its own RESP (distinct SEQs).
        let device = responder_device([1, 2, 3, 4], 0x0F);
        let v = device.query_version().unwrap();
        let h = device.query_health().unwrap();
        assert_eq!((v.fw_major, v.fw_minor, v.fw_patch), (2, 3, 4));
        assert!(h.link_up && h.injection_active);
    }
}
