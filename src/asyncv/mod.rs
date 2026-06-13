//! `AsyncDevice` (feature = `async`) — a thin async wrapper over the **same** sync core (§5).
//!
//! Named `asyncv` because `async` is a reserved keyword.
//!
//! [`AsyncDevice`] is a newtype over a [`Device`]: same `Arc<Inner>`, reader thread, `pending`
//! correlation map, and transport. No second transport, no `spawn_blocking`-per-command thread
//! (makcu's worst wart, §2/§5). The fire-and-go methods delegate verbatim to the sync impl and return
//! immediately — making them `async` would only add a pointless `.await`. Only
//! [`query_version`](AsyncDevice::query_version) / [`query_health`](AsyncDevice::query_health) await;
//! they register the same flume one-shot the sync path uses, so both paths share one correlation
//! mechanism.
//!
//! The query timeout pulls in no async runtime: a cancellable detached `std::thread` timer that, only
//! on a genuine timeout, gen-checked-cancels the pending entry (dropping its `Sender`, which resolves
//! the await as a disconnect = [`Error::QueryTimeout`]). It holds only a `Weak<Inner>`, so it never
//! defers shutdown and a reused SEQ is never evicted by a stale timer. The sole async primitive is
//! `flume`'s `recv_async()`, pollable by any executor (block_on, tokio, async-std, smol).
//!
//! The blocking *open* stays synchronous; construct via [`Device::open`] and convert with
//! [`Device::into_async`] / [`AsyncDevice::from`].

use std::time::Duration;

use crate::Device;
use crate::error::{Error, Result};
use crate::protocol::opcode::{Q_HEALTH, Q_VERSION};
use crate::protocol::types::{Button, ButtonAction, Health, RebootTarget, Version};
use crate::protocol::{Resp, parse_resp};

/// An async view over a [`Device`] — the same core, with `async` query methods (feature = `async`).
///
/// Cheap to clone. Fire-and-go methods are shared verbatim with the sync API; only the queries are
/// `async`. See the [module docs](self) for the one-core design and the runtime-agnostic timeout.
#[derive(Clone, Debug)]
pub struct AsyncDevice {
    device: Device,
}

impl From<Device> for AsyncDevice {
    fn from(device: Device) -> Self {
        AsyncDevice { device }
    }
}

impl Device {
    /// Convert this device into an [`AsyncDevice`] over the **same** core (no new transport/threads).
    pub fn into_async(self) -> AsyncDevice {
        AsyncDevice::from(self)
    }
}

impl AsyncDevice {
    /// Borrow the underlying sync [`Device`] (same core) — e.g. to read counters or reconnect.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn device(&self) -> &Device {
        &self.device
    }

    /// Consume back into the sync [`Device`].
    pub fn into_inner(self) -> Device {
        self.device
    }

    // ---- fire-and-go methods (instant, non-blocking — delegate to the sync impl) ----

    /// `MOVE` — relative cursor movement. Instant; see [`Device::move_rel`].
    pub fn move_rel(&self, dx: i16, dy: i16) -> Result<()> {
        self.device.move_rel(dx, dy)
    }

    /// `WHEEL` — vertical scroll. Instant; see [`Device::wheel`].
    pub fn wheel(&self, delta: i16) -> Result<()> {
        self.device.wheel(delta)
    }

    /// `BUTTON` — set an injection override. Instant; see [`Device::button`].
    pub fn button(&self, button: Button, action: ButtonAction) -> Result<()> {
        self.device.button(button, action)
    }

    /// Press (hold) a button. Instant; see [`Device::press`].
    pub fn press(&self, button: Button) -> Result<()> {
        self.device.press(button)
    }

    /// Soft-release a button. Instant; see [`Device::soft_release`].
    pub fn soft_release(&self, button: Button) -> Result<()> {
        self.device.soft_release(button)
    }

    /// Force-release a button. Instant; see [`Device::force_release`].
    pub fn force_release(&self, button: Button) -> Result<()> {
        self.device.force_release(button)
    }

    /// `RESET` — return to passthrough. Instant; see [`Device::reset`].
    pub fn reset(&self) -> Result<()> {
        self.device.reset()
    }

    /// Reboot a chip (run or ROM download per the target). Instant; see [`Device::reboot`].
    pub fn reboot(&self, target: RebootTarget) -> Result<()> {
        self.device.reboot(target)
    }

    // ---- async queries (the only methods that actually await) ----

    /// Query the box version (§4.1), awaiting the correlated `RESP` with the device's configured
    /// default timeout.
    pub async fn query_version(&self) -> Result<Version> {
        let payload = self
            .query(Q_VERSION, self.device.query_timeout_default())
            .await?;
        match parse_resp(&payload) {
            Some(Resp::Version(v)) => Ok(v),
            _ => Err(Error::NoReply),
        }
    }

    /// Query the box health flags (§4.2), awaiting the correlated `RESP` with the default timeout.
    pub async fn query_health(&self) -> Result<Health> {
        let payload = self
            .query(Q_HEALTH, self.device.query_timeout_default())
            .await?;
        match parse_resp(&payload) {
            Some(Resp::Health(h)) => Ok(h),
            _ => Err(Error::NoReply),
        }
    }

    /// Send `QUERY(what)` and await the correlated `RESP` payload with `timeout`.
    ///
    /// Registers the same flume one-shot the sync path uses and `recv_async().await`s it. The timeout
    /// is a cancellable detached `std::thread` timer that gen-checked-cancels the pending entry on
    /// expiry (see the [module docs](self)); it holds only a `Weak<Inner>` and is woken the instant the
    /// query resolves, so it never lingers.
    pub(crate) async fn query(&self, what: u8, timeout: Duration) -> Result<Vec<u8>> {
        let (seq, gen_id, rx) = self.device.register_query(what)?;

        // Timer waits on `cancel_rx` up to `timeout`: a real timeout gen-checked-cancels the pending
        // entry (dropping the Sender → the await wakes as a disconnect = QueryTimeout); a resolved
        // query drops `cancel_tx` so the timer wakes and does nothing. `Weak<Inner>` + gen check
        // together prevent a stale timer evicting a reused SEQ or deferring shutdown.
        let (cancel_tx, cancel_rx) = flume::bounded::<()>(1);
        let weak = self.device.weak_inner();
        std::thread::Builder::new()
            .name("medius-query-timeout".into())
            .spawn(move || {
                if let Err(flume::RecvTimeoutError::Timeout) = cancel_rx.recv_timeout(timeout)
                    && let Some(inner) = weak.upgrade()
                {
                    inner.cancel_query(seq, gen_id);
                }
            })
            .expect("spawn medius-query-timeout thread");

        // `Ok` = the reader delivered the RESP; `Err` = the sender was dropped (timer or teardown) ⇒
        // no reply in the window. Dropping `cancel_tx` wakes the timer at once on success.
        let res = rx.recv_async().await;
        drop(cancel_tx);
        match res {
            Ok(payload) => Ok(payload),
            Err(_) => Err(Error::QueryTimeout),
        }
    }

    /// Open a device at `path` and wrap it as an [`AsyncDevice`] — convenience over [`Device::open`] +
    /// [`Device::into_async`].
    ///
    /// The open is a one-time blocking syscall run on the caller's thread (not offloaded — that would
    /// need a runtime). Call it before a latency-sensitive async section.
    #[cfg(any(target_os = "linux", windows))]
    pub fn open(path: impl AsRef<std::path::Path>) -> Result<AsyncDevice> {
        Ok(Device::open(path)?.into_async())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use futures::executor::block_on;

    use crate::protocol::{FrameType, encode};
    use crate::transport::mock::MockTransport;

    use super::*;

    /// A mock box that answers VERSION/HEALTH (echoing SEQ).
    fn responder_async(version: [u8; 4], health_flags: u8) -> AsyncDevice {
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
        Device::from_transport(mock).into_async()
    }

    #[test]
    fn async_query_version_resolves_against_mock() {
        let dev = responder_async([1, 7, 8, 9], 0x0F);
        let v = block_on(dev.query_version()).unwrap();
        assert_eq!(v.proto_ver, 1);
        assert_eq!((v.fw_major, v.fw_minor, v.fw_patch), (7, 8, 9));
    }

    #[test]
    fn async_query_health_resolves_against_mock() {
        let dev = responder_async([1, 0, 0, 0], 0x0B); // link|mouse|inject
        let h = block_on(dev.query_health()).unwrap();
        assert!(h.link_up && h.mouse_attached && h.injection_active);
        assert!(!h.clone_configured);
    }

    #[test]
    fn async_query_times_out_on_silent_box() {
        let dev = Device::from_transport(Arc::new(MockTransport::new())).into_async();
        let err = block_on(dev.query(0, Duration::from_millis(40))).unwrap_err();
        assert!(matches!(err, Error::QueryTimeout), "got {err:?}");
        // The waiter must not leak after a timeout.
        assert_eq!(dev.device().pending_len(), 0);
    }

    #[test]
    fn fire_and_go_methods_delegate() {
        let mock = Arc::new(MockTransport::new());
        let dev = Device::from_transport(mock.clone()).into_async();
        dev.move_rel(3, -4).unwrap();
        dev.press(Button::Left).unwrap();
        let frames = crate::protocol::FrameDecoder::new().feed_collect(&mock.written());
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0].ty, FrameType::Move);
        assert_eq!(frames[1].ty, FrameType::Button);
    }

    #[test]
    fn into_async_shares_the_same_core() {
        // AsyncDevice and its source Device share one Arc<Inner> (same counters).
        let mock = Arc::new(MockTransport::new());
        let device = Device::from_transport(mock);
        let adev = device.clone().into_async();
        device.move_rel(1, 0).unwrap();
        assert_eq!(adev.device().counters().frames_tx, 1);
    }

    /// FIX 2 (ABA) — a completed async query's timeout timer must never evict a *reused* SEQ. We
    /// exercise exactly what the timer does (`register_pending` → `cancel_query`), so the test is
    /// deterministic with no real timer sleep.
    #[test]
    fn completed_async_timer_does_not_evict_reused_seq() {
        let dev = responder_async([1, 0, 0, 0], 0x0B);
        let device = dev.device().clone();

        let _ = block_on(dev.query_health()).unwrap();
        assert_eq!(device.pending_len(), 0);

        // Stand in for A's now-stale (seq, gen): register then cancel to capture a real freed slot.
        let (seq_a, gen_a, _rx_a) = device.register_pending(0);
        device.cancel_query(seq_a, gen_a);

        // Advance the rolling SEQ so the next register wraps back onto A's SEQ.
        for _ in 0..255 {
            let _ = device.next_seq();
        }
        // B reuses A's SEQ with a newer generation.
        let (seq_b, gen_b, rx_b) = device.register_pending(0);
        assert_eq!(seq_b, seq_a, "B reuses A's freed SEQ");
        assert_ne!(gen_b, gen_a);

        // A's stale timer cancel must NOT evict B.
        device.cancel_query(seq_a, gen_a);
        assert_eq!(
            device.pending_len(),
            1,
            "a completed query's stale timer must not evict the reused SEQ's newer waiter"
        );
        device.cancel_query(seq_b, gen_b);
        drop(rx_b);
    }

    /// `AsyncDevice` is `Send + Sync` (a clonable handle over the same shared core; callers move it
    /// across async tasks). Mirrors the guard tests for `Device`/`MockTransport`/`Counters`.
    #[test]
    fn async_device_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<AsyncDevice>();
    }
}
