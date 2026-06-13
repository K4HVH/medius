//! Async wrapper over the same sync `Device` core (feature = `async`).

use std::time::Duration;

use crate::Device;
use crate::error::{Error, Result};
use crate::protocol::opcode::{Q_HEALTH, Q_VERSION};
use crate::protocol::{Resp, parse_resp};
use crate::types::{Button, ButtonAction, Health, RebootTarget, Version};

/// An async view over a [`Device`] — the same core, with `async` query methods (feature = `async`).
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
    /// Convert this device into an [`AsyncDevice`] over the same core.
    pub fn into_async(self) -> AsyncDevice {
        AsyncDevice::from(self)
    }
}

impl AsyncDevice {
    /// Consume back into the sync [`Device`].
    pub fn into_inner(self) -> Device {
        self.device
    }

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

    /// Query the box version (§4.1), awaiting the correlated `RESP` with the default timeout.
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

    pub(crate) async fn query(&self, what: u8, timeout: Duration) -> Result<Vec<u8>> {
        let (seq, gen_id, rx) = self.device.register_query(what)?;

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

        let res = rx.recv_async().await;
        drop(cancel_tx);
        match res {
            Ok(payload) => Ok(payload),
            Err(_) => Err(Error::QueryTimeout),
        }
    }

    /// Open a device at `path` and wrap it as an [`AsyncDevice`].
    pub fn open(path: impl AsRef<std::path::Path>) -> Result<AsyncDevice> {
        Ok(Device::open(path)?.into_async())
    }
}
