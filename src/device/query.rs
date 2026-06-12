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

impl Device {
    /// Send `QUERY(what)` and block for the correlated `RESP` payload, with the device's configured
    /// default timeout (from [`ConnectOptions`](crate::ConnectOptions), default 1 s).
    ///
    /// The reserved `SEQ` in `pending` is exactly the `SEQ` of the sent frame, so the reader can
    /// fulfil it. Returns the raw `RESP` **payload** (the caller decodes it); higher-level
    /// [`query_version`](Device::query_version) / [`query_health`](Device::query_health) parse it.
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
                // Timed out (or the sender was dropped). Remove the stale waiter so `pending` doesn't
                // leak — but only *our* entry: a gen-checked cancel never evicts a newer query that
                // reused this wire SEQ in the meantime.
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

    /// Reserve a free `SEQ`, register the generation-tagged bounded(1) one-shot under it in `pending`,
    /// and send the `QUERY(what)` frame — returning `(seq, gen, receiver)` for the caller to await.
    ///
    /// This is the shared registration both the sync path ([`query_timeout`](Device::query_timeout),
    /// via `recv_timeout`) and the async wrapper ([`crate::asyncv::AsyncDevice`], via `recv_async`)
    /// use, so there is exactly **one** correlation mechanism and **one** flume one-shot — no
    /// duplicated transport or query logic (§5). The `SEQ` is chosen via
    /// [`register_pending`](Device::register_pending) to be free of any currently-pending waiter, so
    /// two in-flight queries never share a wire `SEQ` (no cross-delivery). On a send failure the waiter
    /// is removed (gen-checked) and the error returned. The caller MUST `cancel_query(seq, gen)` if it
    /// gives up (the sync/async timeout paths both do).
    pub(crate) fn register_query(&self, what: u8) -> Result<(u8, u64, flume::Receiver<Vec<u8>>)> {
        // Reserve a free SEQ + tagged one-shot *before* sending, so a fast RESP can never arrive
        // before the waiter exists. `register_pending` releases `pending` before we send (lock order).
        let (seq, gen_id, rx) = self.register_pending();

        // Send the QUERY with the SAME seq the waiter is keyed on. If the send fails, drop the waiter
        // (gen-checked, so we never evict a newer query that reused this SEQ).
        if let Err(e) = self.send_with_seq(seq, FrameType::Query, &query_payload(what)) {
            self.cancel_query(seq, gen_id);
            return Err(e);
        }
        Ok((seq, gen_id, rx))
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

    /// A mock that answers ONLY `QUERY(HEALTH)` (selector 1), ignoring `QUERY(VERSION)` — so a
    /// VERSION query stays pending forever while a HEALTH query is answered.
    fn health_only_responder(health_flags: u8) -> Device {
        let mock = Arc::new(MockTransport::with_responder(move |ty, seq, payload| {
            if ty == FrameType::Query && payload.first().copied() == Some(1) {
                encode(FrameType::Resp, seq, &[1, health_flags]).unwrap()
            } else {
                Vec::new()
            }
        }));
        Device::from_transport(mock)
    }

    /// FIX 1 — SEQ-namespace wrap: a still-pending query A must NOT capture query B's RESP even after
    /// the rolling SEQ wraps a full 256 back onto A's SEQ. `register_pending` picks a SEQ free of any
    /// in-flight waiter, so B is forced onto a different SEQ; B resolves with its own value and A times
    /// out (rather than B being stolen by A's stale waiter).
    #[test]
    fn pending_query_survives_seq_wrap_without_cross_delivery() {
        use std::sync::mpsc;

        let device = health_only_responder(0x0B); // link|mouse|inject

        // Query A = VERSION, never answered by this mock. Run it on another thread so it blocks on its
        // (long) timeout while we wrap the SEQ and issue B underneath it.
        let dev_a = device.clone();
        let (done_tx, done_rx) = mpsc::channel();
        let a = std::thread::spawn(move || {
            let r = dev_a.query_timeout(0, Duration::from_millis(400)); // selector 0 = VERSION
            let _ = done_tx.send(());
            r
        });

        // Let A register its pending waiter before we advance the SEQ.
        std::thread::sleep(Duration::from_millis(20));

        // Advance the rolling SEQ so that B's *natural* next draw lands exactly back on A's still-held
        // SEQ. A drew one SEQ when it registered, so 255 more fire-and-go draws wrap the counter back
        // onto A's value — meaning B would collide with A without the free-SEQ pick.
        for _ in 0..255 {
            device.move_rel(0, 0).unwrap();
        }

        // Query B = HEALTH, which the mock DOES answer. Because A still occupies the SEQ B would
        // naturally draw, `register_pending` skips it and gives B a free SEQ — so B resolves to ITS
        // value, never stealing/being stolen by A's still-pending waiter.
        let h = device.query_health().expect("B must resolve");
        assert!(h.link_up && h.mouse_attached && h.injection_active);

        // A must still be pending (it has not been stolen by B's RESP); it then times out.
        assert!(
            done_rx.recv_timeout(Duration::from_millis(50)).is_err(),
            "A must NOT have completed early (no cross-delivery from B)"
        );
        let a_res = a.join().unwrap();
        assert!(
            matches!(a_res, Err(Error::QueryTimeout)),
            "A must time out, got {a_res:?}"
        );
    }

    /// FIX 1 — gen-checked cancel: a stale `cancel_query(seq, old_gen)` must NOT remove a newer entry
    /// that has since been registered under that same wire SEQ.
    #[test]
    fn stale_cancel_does_not_evict_newer_waiter() {
        let device = Device::from_transport(Arc::new(MockTransport::new()));

        // Register entry A and capture its (seq, gen), then cancel it so its SEQ is free again.
        let (seq_a, gen_a, _rx_a) = device.register_pending();
        device.cancel_query(seq_a, gen_a);
        assert_eq!(device.pending_len(), 0);

        // Advance the rolling SEQ so the *next* register_pending lands back on A's old SEQ (a full
        // wrap from the post-A position). register_pending draws one SEQ itself, so advance 255 here.
        for _ in 0..255 {
            let _ = device.next_seq();
        }

        // Entry B reuses A's freed SEQ but carries a newer generation.
        let (seq_b, gen_b, rx_b) = device.register_pending();
        assert_eq!(seq_b, seq_a, "B reuses A's freed SEQ");
        assert_ne!(gen_b, gen_a, "B has a newer generation");

        // A stale cancel using A's OLD gen must leave B intact.
        device.cancel_query(seq_a, gen_a);
        assert_eq!(
            device.pending_len(),
            1,
            "stale gen cancel must not evict the newer waiter B"
        );

        // B's own (current-gen) cancel removes it.
        device.cancel_query(seq_b, gen_b);
        assert_eq!(device.pending_len(), 0);
        drop(rx_b);
    }
}
