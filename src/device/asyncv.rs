use std::time::Duration;

use crate::error::{Error, Result};
use crate::link::Link;
use crate::protocol::opcode::{
    OPT_EMIT, OPT_IMPERFECT, OPT_MOVE_RIDE, Q_CAPS, Q_CATCH, Q_CLIP, Q_DEVICE_INFO, Q_HEALTH,
    Q_LOCKS, Q_RATE, Q_STATS, Q_VERSION,
};
use crate::protocol::{Resp, parse_resp};
use crate::types::{
    Action, Axis, Blanket, Caps, CatchMask, CatchState, ClipBuilder, ClipConfig, ClipStatus,
    CountersSnapshot, DeviceInfo, EmitPace, EmitPaceStatus, Health, ImperfectStatus, LedMode,
    LedTarget, LockDirection, LockTarget, Locks, Motion, Rate, RebootTarget, Stats, Usage, Version,
};

use super::Device;
use super::catch::EventStream;
use super::clip::ClipHandle;
use super::discover::BoxInfo;
use super::logs::LogStream;

/// An async view over a [`Device`]: the same `Link` core, with `async` queries.
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

    /// `MOVE`: relative cursor movement. Instant; see [`Device::move_rel`].
    pub fn move_rel(&self, dx: i16, dy: i16) -> Result<()> {
        self.dev().move_rel(dx, dy)
    }

    /// `WHEEL`: vertical scroll. Instant; see [`Device::wheel`].
    pub fn wheel(&self, delta: i16) -> Result<()> {
        self.dev().wheel(delta)
    }

    /// `MOVE`: field-generic relative axis (cursor or wheel). Instant; see [`Device::move_axis`].
    pub fn move_axis(&self, motion: Motion) -> Result<()> {
        self.dev().move_axis(motion)
    }

    /// `INJECT`: momentary override for any usage (button, key, or media). Instant; see [`Device::inject`].
    pub fn inject(&self, usage: impl Into<Usage>, action: Action) -> Result<()> {
        self.dev().inject(usage, action)
    }

    /// Press (force down) any usage. Instant; see [`Device::press`].
    pub fn press(&self, usage: impl Into<Usage>) -> Result<()> {
        self.dev().press(usage)
    }

    /// Soft-release any usage. Instant; see [`Device::release`].
    pub fn release(&self, usage: impl Into<Usage>) -> Result<()> {
        self.dev().release(usage)
    }

    /// Force-release any usage. Instant; see [`Device::force_release`].
    pub fn force_release(&self, usage: impl Into<Usage>) -> Result<()> {
        self.dev().force_release(usage)
    }

    /// `RESET`: return to passthrough. Instant; see [`Device::reset`].
    pub fn reset(&self) -> Result<()> {
        self.dev().reset()
    }

    /// Reboot a chip (run or ROM download per the target). Instant; see [`Device::reboot`].
    pub fn reboot(&self, target: RebootTarget) -> Result<()> {
        self.dev().reboot(target)
    }

    /// Re-assert every currently held override. Instant; see [`Device::reapply`].
    pub fn reapply(&self) -> Result<()> {
        self.dev().reapply()
    }

    /// Best-effort reconnect over the shared core; blocks the calling thread. See [`Device::reconnect`].
    pub fn reconnect(&self) -> Result<()> {
        self.dev().reconnect()
    }

    /// A snapshot of the always-on counters. See [`Device::counters`].
    pub fn counters(&self) -> CountersSnapshot {
        self.dev().counters()
    }

    /// A [`LogStream`] over the device `LOG` stream; it offers `recv_async`. See [`Device::logs`].
    pub fn logs(&self) -> LogStream {
        self.dev().logs()
    }

    /// `LED`: override a status LED. Instant; see [`Device::led`].
    pub fn led(&self, target: LedTarget, mode: LedMode, level: u8) -> Result<()> {
        self.dev().led(target, mode, level)
    }

    /// `LOCK`: block a usage (button/key/media) or axis. Instant; see [`Device::lock`].
    pub fn lock(&self, target: impl Into<LockTarget>, direction: LockDirection) -> Result<()> {
        self.dev().lock(target, direction)
    }

    /// `LOCK`: release a locked usage or axis. Instant; see [`Device::unlock`].
    pub fn unlock(&self, target: impl Into<LockTarget>, direction: LockDirection) -> Result<()> {
        self.dev().unlock(target, direction)
    }

    /// `LOCK`: block a relative axis by sign. Instant; see [`Device::lock_axis`].
    pub fn lock_axis(&self, axis: Axis, direction: LockDirection) -> Result<()> {
        self.dev().lock_axis(axis, direction)
    }

    /// `LOCK`: release an axis lock. Instant; see [`Device::unlock_axis`].
    pub fn unlock_axis(&self, axis: Axis, direction: LockDirection) -> Result<()> {
        self.dev().unlock_axis(axis, direction)
    }

    /// `LOCK`: blanket-block a whole group. Instant; see [`Device::lock_all`].
    pub fn lock_all(&self, what: Blanket, direction: LockDirection) -> Result<()> {
        self.dev().lock_all(what, direction)
    }

    /// `LOCK`: release a blanket lock. Instant; see [`Device::unlock_all`].
    pub fn unlock_all(&self, what: Blanket, direction: LockDirection) -> Result<()> {
        self.dev().unlock_all(what, direction)
    }

    /// Subscribe to the physical-input event stream. Instant; see [`Device::catch_events`].
    pub fn catch_events(&self, mask: CatchMask) -> Result<EventStream> {
        self.dev().catch_events(mask)
    }

    /// `OPTION(IMPERFECT)`: opt into cloning an over-capacity device. Instant; see [`Device::allow_imperfect_clones`].
    pub fn allow_imperfect_clones(&self, allow: bool) -> Result<()> {
        self.dev().allow_imperfect_clones(allow)
    }

    /// `OPTION(MOVE_RIDE)`: movement riding. Instant; see [`Device::set_movement_riding`].
    pub fn set_movement_riding(&self, window: Option<Duration>) -> Result<()> {
        self.dev().set_movement_riding(window)
    }

    /// `OPTION(EMIT)`: emit-rate pacing. Instant; see [`Device::set_emit_pace`].
    pub fn set_emit_pace(&self, pace: EmitPace) -> Result<()> {
        self.dev().set_emit_pace(pace)
    }

    /// `OPTION(NAME)`: set the box's persistent name. Instant; see [`Device::set_name`].
    pub fn set_name(&self, name: &str) -> Result<()> {
        self.dev().set_name(name)
    }

    /// `OPTION(NAME)` clear: revert to the synthesized default. Instant; see [`Device::clear_name`].
    pub fn clear_name(&self) -> Result<()> {
        self.dev().clear_name()
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

    /// Query the cloned device's USB identity, kind, and product (§4.3), awaiting the correlated `RESP`.
    pub async fn device_info(&self) -> Result<DeviceInfo> {
        let payload = self
            .link
            .query_async(Q_DEVICE_INFO, self.link.query_timeout_default())
            .await?;
        match parse_resp(&payload) {
            Some(Resp::DeviceInfo(m)) => Ok(m),
            _ => Err(Error::NoReply),
        }
    }

    /// Query the whole cloned device's capabilities in one request (§4.4), awaiting the correlated `RESP`.
    pub async fn caps(&self) -> Result<Caps> {
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

    /// Query the active lock bitmask (§4.8), awaiting the correlated `RESP`.
    pub async fn query_locks(&self) -> Result<Locks> {
        let payload = self
            .link
            .query_async(Q_LOCKS, self.link.query_timeout_default())
            .await?;
        match parse_resp(&payload) {
            Some(Resp::Locks(l)) => Ok(l),
            _ => Err(Error::NoReply),
        }
    }

    /// Query the catch subscription mask + box-side dropped count (§4.9), awaiting the correlated `RESP`.
    pub async fn query_catch(&self) -> Result<CatchState> {
        let payload = self
            .link
            .query_async(Q_CATCH, self.link.query_timeout_default())
            .await?;
        match parse_resp(&payload) {
            Some(Resp::Catch(c)) => Ok(c),
            _ => Err(Error::NoReply),
        }
    }

    /// Query the imperfect-clone status (§4.14), awaiting the correlated `RESP`.
    pub async fn query_imperfect(&self) -> Result<ImperfectStatus> {
        let payload = self
            .link
            .query_option_async(OPT_IMPERFECT, self.link.query_timeout_default())
            .await?;
        match parse_resp(&payload) {
            Some(Resp::Imperfect(i)) => Ok(i),
            _ => Err(Error::NoReply),
        }
    }

    /// Query the movement-riding window (§4.14), awaiting the correlated `RESP`; `None` = off.
    pub async fn query_movement_riding(&self) -> Result<Option<Duration>> {
        let payload = self
            .link
            .query_option_async(OPT_MOVE_RIDE, self.link.query_timeout_default())
            .await?;
        match parse_resp(&payload) {
            Some(Resp::MovementRiding(w)) => Ok(w),
            _ => Err(Error::NoReply),
        }
    }

    /// Query the emit-rate pacing mode + the rate in effect (§4.14), awaiting the correlated `RESP`.
    pub async fn query_emit_pace(&self) -> Result<EmitPaceStatus> {
        let payload = self
            .link
            .query_option_async(OPT_EMIT, self.link.query_timeout_default())
            .await?;
        match parse_resp(&payload) {
            Some(Resp::EmitPace(s)) => Ok(s),
            _ => Err(Error::NoReply),
        }
    }

    /// Buffered-clip playback over the async view (§3.11); see [`Device::clip`].
    pub fn clip(&self) -> AsyncClipHandle {
        AsyncClipHandle {
            inner: self.dev().clip(),
        }
    }

    /// Open a device at `path` and wrap it as an [`AsyncDevice`].
    pub fn open(path: impl AsRef<std::path::Path>) -> Result<AsyncDevice> {
        Ok(Device::open(path)?.into_async())
    }

    /// Discover the first medius box, open it, and wrap as an [`AsyncDevice`]; blocks. See [`Device::find`].
    pub fn find() -> Result<AsyncDevice> {
        Ok(Device::find()?.into_async())
    }

    /// Enumerate every connected box; blocks (scan + per-box handshake). See [`Device::list`].
    pub fn list() -> Vec<BoxInfo> {
        Device::list()
    }

    /// Open the box whose identity matches `id` (device MAC or CH343 serial). See [`Device::open_by_id`].
    pub fn open_by_id(id: &str) -> Result<AsyncDevice> {
        Ok(Device::open_by_id(id)?.into_async())
    }

    /// Open the first box whose clone is a mouse. See [`Device::find_mouse_box`].
    pub fn find_mouse_box() -> Result<AsyncDevice> {
        Ok(Device::find_mouse_box()?.into_async())
    }

    /// Open the first box whose clone is a keyboard. See [`Device::find_keyboard_box`].
    pub fn find_keyboard_box() -> Result<AsyncDevice> {
        Ok(Device::find_keyboard_box()?.into_async())
    }
}

/// An async view over a [`ClipHandle`](crate::ClipHandle) (§3.11); see [`AsyncDevice::clip`].
#[derive(Clone, Debug)]
pub struct AsyncClipHandle {
    inner: ClipHandle,
}

impl AsyncClipHandle {
    /// Append entries to the ring. Instant; see [`ClipHandle::append`](crate::ClipHandle::append).
    pub fn append(&self, clip: &ClipBuilder) -> Result<()> {
        self.inner.append(clip)
    }

    /// Begin playback with a [`ClipConfig`]. Instant; see [`ClipHandle::start`](crate::ClipHandle::start).
    pub fn start(&self, config: &ClipConfig) -> Result<()> {
        self.inner.start(config)
    }

    /// Stop playback, flush the ring, release the auto-lock. Instant; see [`ClipHandle::stop`](crate::ClipHandle::stop).
    pub fn stop(&self) -> Result<()> {
        self.inner.stop()
    }

    /// Arm an on-device catch-trigger on any [`Usage`] with a [`ClipConfig`]. Instant; see [`ClipHandle::arm_catch`](crate::ClipHandle::arm_catch).
    pub fn arm_catch(&self, trigger: impl Into<Usage>, config: &ClipConfig) -> Result<()> {
        self.inner.arm_catch(trigger, config)
    }

    /// Arm a catch-trigger on any physical input with a [`ClipConfig`]. Instant; see [`ClipHandle::arm_catch_any`](crate::ClipHandle::arm_catch_any).
    pub fn arm_catch_any(&self, config: &ClipConfig) -> Result<()> {
        self.inner.arm_catch_any(config)
    }

    /// Clear a pending catch-arm. Instant; see [`ClipHandle::disarm`](crate::ClipHandle::disarm).
    pub fn disarm(&self) -> Result<()> {
        self.inner.disarm()
    }

    /// `QUERY(CLIP)`: the ring depth and playback counters, awaiting the correlated `RESP`. See [`ClipHandle::status`](crate::ClipHandle::status).
    pub async fn status(&self) -> Result<ClipStatus> {
        let link = self.inner.link();
        let payload = link
            .query_async(Q_CLIP, link.query_timeout_default())
            .await?;
        match parse_resp(&payload) {
            Some(Resp::Clip(s)) => Ok(s),
            _ => Err(Error::NoReply),
        }
    }
}
