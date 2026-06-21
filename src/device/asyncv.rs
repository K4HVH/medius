use crate::error::{Error, Result};
use crate::link::Link;
use crate::protocol::opcode::{Q_CAPS, Q_HEALTH, Q_MOUSE_INFO, Q_RATE, Q_STATS, Q_VERSION};
use crate::protocol::{Resp, parse_resp};
use crate::types::{
    Button, ButtonAction, Caps, Health, MouseInfo, Rate, RebootTarget, Stats, Version,
};

use super::Device;

/// An async view over a [`Device`] — the same `Link` core, with `async` queries.
#[derive(Clone, Debug)]
pub struct AsyncDevice {
    link: Link,
}

impl From<Device> for AsyncDevice {
    fn from(device: Device) -> Self {
        AsyncDevice { link: device.link }
    }
}

impl Device {
    /// Convert this device into an [`AsyncDevice`] over the same core.
    pub fn into_async(self) -> AsyncDevice {
        AsyncDevice::from(self)
    }
}

impl AsyncDevice {
    fn dev(&self) -> Device {
        Device {
            link: self.link.clone(),
        }
    }

    /// Consume back into the sync [`Device`].
    pub fn into_inner(self) -> Device {
        Device { link: self.link }
    }

    /// `MOVE` — relative cursor movement. Instant; see [`Device::move_rel`].
    pub fn move_rel(&self, dx: i16, dy: i16) -> Result<()> {
        self.dev().move_rel(dx, dy)
    }

    /// `WHEEL` — vertical scroll. Instant; see [`Device::wheel`].
    pub fn wheel(&self, delta: i16) -> Result<()> {
        self.dev().wheel(delta)
    }

    /// `BUTTON` — set an injection override. Instant; see [`Device::button`].
    pub fn button(&self, button: Button, action: ButtonAction) -> Result<()> {
        self.dev().button(button, action)
    }

    /// Press (hold) a button. Instant; see [`Device::press`].
    pub fn press(&self, button: Button) -> Result<()> {
        self.dev().press(button)
    }

    /// Soft-release a button. Instant; see [`Device::soft_release`].
    pub fn soft_release(&self, button: Button) -> Result<()> {
        self.dev().soft_release(button)
    }

    /// Force-release a button. Instant; see [`Device::force_release`].
    pub fn force_release(&self, button: Button) -> Result<()> {
        self.dev().force_release(button)
    }

    /// `RESET` — return to passthrough. Instant; see [`Device::reset`].
    pub fn reset(&self) -> Result<()> {
        self.dev().reset()
    }

    /// Reboot a chip (run or ROM download per the target). Instant; see [`Device::reboot`].
    pub fn reboot(&self, target: RebootTarget) -> Result<()> {
        self.dev().reboot(target)
    }

    /// Query the box version, awaiting the correlated `RESP` with the default timeout.
    pub async fn query_version(&self) -> Result<Version> {
        let payload = self
            .link
            .query_async(Q_VERSION, self.link.query_timeout_default())
            .await?;
        match parse_resp(&payload) {
            Some(Resp::Version(v)) => Ok(v),
            _ => Err(Error::NoReply),
        }
    }

    /// Query the box health flags, awaiting the correlated `RESP` with the default timeout.
    pub async fn query_health(&self) -> Result<Health> {
        let payload = self
            .link
            .query_async(Q_HEALTH, self.link.query_timeout_default())
            .await?;
        match parse_resp(&payload) {
            Some(Resp::Health(h)) => Ok(h),
            _ => Err(Error::NoReply),
        }
    }

    /// Query the cloned mouse's USB identity (§4.3), awaiting the correlated `RESP`.
    pub async fn query_mouse_info(&self) -> Result<MouseInfo> {
        let payload = self
            .link
            .query_async(Q_MOUSE_INFO, self.link.query_timeout_default())
            .await?;
        match parse_resp(&payload) {
            Some(Resp::MouseInfo(m)) => Ok(m),
            _ => Err(Error::NoReply),
        }
    }

    /// Query the emulated mouse's capabilities (§4.4), awaiting the correlated `RESP`.
    pub async fn query_caps(&self) -> Result<Caps> {
        let payload = self
            .link
            .query_async(Q_CAPS, self.link.query_timeout_default())
            .await?;
        match parse_resp(&payload) {
            Some(Resp::Caps(c)) => Ok(c),
            _ => Err(Error::NoReply),
        }
    }

    /// Query the live native report rate (§4.5), awaiting the correlated `RESP`.
    pub async fn query_rate(&self) -> Result<Rate> {
        let payload = self
            .link
            .query_async(Q_RATE, self.link.query_timeout_default())
            .await?;
        match parse_resp(&payload) {
            Some(Resp::Rate(r)) => Ok(r),
            _ => Err(Error::NoReply),
        }
    }

    /// Query the box's delivery/telemetry counters (§4.6), awaiting the correlated `RESP`.
    pub async fn query_stats(&self) -> Result<Stats> {
        let payload = self
            .link
            .query_async(Q_STATS, self.link.query_timeout_default())
            .await?;
        match parse_resp(&payload) {
            Some(Resp::Stats(s)) => Ok(s),
            _ => Err(Error::NoReply),
        }
    }

    /// Open a device at `path` and wrap it as an [`AsyncDevice`].
    pub fn open(path: impl AsRef<std::path::Path>) -> Result<AsyncDevice> {
        Ok(Device::open(path)?.into_async())
    }
}
