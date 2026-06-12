//! `AsyncDevice` (feature = `async`) — a thin async wrapper over the **same** sync core (§5).
//!
//! Module is named `asyncv` because `async` is a reserved keyword.
//!
//! ## One core, no duplication
//!
//! [`AsyncDevice`] is a newtype over a [`Device`] — i.e. the same `Arc<Inner>`, the same reader
//! thread, the same `pending` correlation map, the same transport. There is **no** second transport
//! and **no** `spawn_blocking`-per-command thread (makcu's worst wart, §2/§5). The fire-and-go methods
//! (`move_rel`/`wheel`/`button`/`press`/`release`/`force_release`/`reset`/`reboot`/`reboot_download`/
//! `reboot_download`) are *instant non-blocking writes* — they delegate verbatim to the sync impl and
//! return immediately, so making them `async` would add a pointless `.await`; they are plain methods
//! that the caller may call from async code freely.
//!
//! Only [`query_version`](AsyncDevice::query_version) / [`query_health`](AsyncDevice::query_health)
//! differ: they register the **same** flume one-shot the sync path uses
//! ([`Device::register_query`]) and `recv_async().await` it — so the sync and async query paths share
//! one channel and one correlation mechanism.
//!
//! ## Runtime-agnostic query timeout (no async runtime pulled in)
//!
//! The timeout is realized **without** a full async runtime: a lightweight **detached `std::thread`
//! timer** sleeps for the timeout and then calls [`Device::cancel_pending`], which drops the query's
//! one-shot `Sender`. Dropping the sender makes the awaited `recv_async()` resolve with a
//! disconnect, which we map to [`Error::QueryTimeout`]. If the `RESP` arrives first, the await
//! resolves with the payload and the still-pending timer simply runs out and no-ops (removing an
//! already-removed seq is harmless). The only async primitive used is `flume`'s own
//! `recv_async()` future — pollable by **any** executor (`futures::executor::block_on`, tokio,
//! async-std, smol, …). No tokio dependency, no `spawn_blocking`.
//!
//! The blocking *open* stays synchronous (it is a one-time `open()` syscall); construct the device
//! with [`Device::open`] etc. and convert via [`Device::into_async`] / [`AsyncDevice::from`].

use std::time::Duration;

use crate::error::{Error, Result};
use crate::protocol::opcode::{Q_HEALTH, Q_VERSION};
use crate::protocol::types::{Button, ButtonAction, Health, RebootTarget, Version};
use crate::protocol::{Resp, parse_resp};
use crate::{ConnectOptions, Device};

/// An async view over a [`Device`] — the same core, with `async` query methods (feature = `async`).
///
/// Cheap to clone (it wraps a `Device`, itself an `Arc`). All fire-and-go methods are shared verbatim
/// with the sync API; only the queries are `async`. See the [module docs](self) for the one-core
/// design and the runtime-agnostic timeout.
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
    /// Borrow the underlying sync [`Device`] (same core) — e.g. to open a
    /// [`MovementSession`](crate::MovementSession) or read counters.
    pub fn device(&self) -> &Device {
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

    /// Soft-release a button. Instant; see [`Device::release`].
    pub fn release(&self, button: Button) -> Result<()> {
        self.device.release(button)
    }

    /// Force-release a button. Instant; see [`Device::force_release`].
    pub fn force_release(&self, button: Button) -> Result<()> {
        self.device.force_release(button)
    }

    /// `RESET` — return to passthrough. Instant; see [`Device::reset`].
    pub fn reset(&self) -> Result<()> {
        self.device.reset()
    }

    /// Reboot a chip to run. Instant; see [`Device::reboot`].
    pub fn reboot(&self, target: RebootTarget) -> Result<()> {
        self.device.reboot(target)
    }

    /// Reboot a chip to ROM download. Instant; see [`Device::reboot_download`].
    pub fn reboot_download(&self, target: RebootTarget) -> Result<()> {
        self.device.reboot_download(target)
    }

    // ---- async queries (the only methods that actually await) ----

    /// Query the box version (§4.1), awaiting the correlated `RESP` with the device's configured
    /// default timeout.
    pub async fn query_version(&self) -> Result<Version> {
        let payload = self.query(Q_VERSION, self.device.query_timeout_default()).await?;
        match parse_resp(&payload) {
            Some(Resp::Version(v)) => Ok(v),
            _ => Err(Error::NoReply),
        }
    }

    /// Query the box health flags (§4.2), awaiting the correlated `RESP` with the default timeout.
    pub async fn query_health(&self) -> Result<Health> {
        let payload = self.query(Q_HEALTH, self.device.query_timeout_default()).await?;
        match parse_resp(&payload) {
            Some(Resp::Health(h)) => Ok(h),
            _ => Err(Error::NoReply),
        }
    }

    /// Send `QUERY(what)` and await the correlated `RESP` payload with `timeout`.
    ///
    /// Registers the SAME flume one-shot the sync path uses ([`Device::register_query`]) and
    /// `recv_async().await`s it. The timeout is a detached `std::thread` timer that drops the waiter on
    /// expiry (see the [module docs](self)) — no async runtime is pulled in.
    pub async fn query(&self, what: u8, timeout: Duration) -> Result<Vec<u8>> {
        let (seq, rx) = self.device.register_query(what)?;

        // Detached timer: after `timeout`, invalidate the pending entry. Dropping its `Sender` makes
        // the await below resolve with a disconnect, which we treat as a timeout. Runtime-agnostic —
        // this is a plain OS thread, no executor involved.
        let timer_device = self.device.clone();
        std::thread::Builder::new()
            .name("medius-query-timeout".into())
            .spawn(move || {
                std::thread::sleep(timeout);
                // No-op if the RESP already removed this seq.
                timer_device.cancel_pending(seq);
            })
            .expect("spawn medius-query-timeout thread");

        // Await the shared one-shot. `Ok` = the reader delivered the RESP; `Err` = the sender was
        // dropped (by the timer, or by a transport teardown) ⇒ no reply within the window.
        match rx.recv_async().await {
            Ok(payload) => Ok(payload),
            Err(_) => {
                // Ensure the entry is gone even if we lost a race with a late insert (defensive).
                self.device.cancel_pending(seq);
                Err(Error::QueryTimeout)
            }
        }
    }

    /// Open a device at `path` (the blocking open runs on the caller's thread, documented), then wrap
    /// it as an [`AsyncDevice`]. A convenience over [`Device::open`] + [`Device::into_async`].
    ///
    /// The open is a one-time blocking syscall; it is **not** offloaded to a thread (that would need a
    /// runtime / `spawn_blocking`). Call it before entering a latency-sensitive async section, or wrap
    /// it yourself if your executor offers a blocking-offload primitive.
    #[cfg(any(target_os = "linux", windows))]
    pub fn open(path: impl AsRef<std::path::Path>) -> Result<AsyncDevice> {
        Ok(Device::open(path)?.into_async())
    }

    /// As [`open`](AsyncDevice::open) but with explicit [`ConnectOptions`].
    #[cfg(any(target_os = "linux", windows))]
    pub fn open_with(
        path: impl AsRef<std::path::Path>,
        opts: &ConnectOptions,
    ) -> Result<AsyncDevice> {
        Ok(Device::open_with(path, opts)?.into_async())
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
        // The AsyncDevice and the Device it came from share one Arc<Inner> (same counters).
        let mock = Arc::new(MockTransport::new());
        let device = Device::from_transport(mock);
        let adev = device.clone().into_async();
        device.move_rel(1, 0).unwrap();
        // The shared counters reflect the frame sent via the sync handle.
        assert_eq!(adev.device().counters().frames_tx, 1);
    }
}
