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
use crate::protocol::{Resp, parse_resp};
use crate::types::{Button, ButtonAction, Health, RebootTarget, Version};

/// An async view over a [`Device`] — the same core, with `async` query methods (feature = `async`).
///
/// Cheap to clone. Fire-and-go methods are shared verbatim with the sync API; only the queries are
/// `async`. See the module docs for the one-core design and the runtime-agnostic timeout.
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
    /// expiry (see the module docs); it holds only a `Weak<Inner>` and is woken the instant the
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
    pub fn open(path: impl AsRef<std::path::Path>) -> Result<AsyncDevice> {
        Ok(Device::open(path)?.into_async())
    }
}
