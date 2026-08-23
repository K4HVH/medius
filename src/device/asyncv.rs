use std::time::Duration;

use crate::error::{Error, Result};
use crate::link::Link;
use crate::protocol::opcode::{
    OPT_BEARING, OPT_EMIT, OPT_IMPERFECT, OPT_MOVE_RIDE, Q_CAPS, Q_CATCH, Q_CLIP, Q_DEVICE_INFO,
    Q_FIRMWARE, Q_HEALTH, Q_LOCKS, Q_RATE, Q_STATS, Q_VERSION,
};
use crate::protocol::{Resp, parse_resp};
use crate::types::{
    Action, Axis, Bearing, BearingMode, Blanket, Caps, CatchFilter, CatchState, ClipBuilder,
    ClipSettings, ClipStatus, ClipTrigger, CountersSnapshot, DeviceInfo, Direction, Edge, EmitPace,
    EmitPaceStatus, FirmwareInfo, Health, ImperfectStatus, LedMode, LedTarget, LockTarget, Locks,
    Motion, MoveTiming, PendingMotion, Rate, RebootTarget, Stats, UpdateProgress, UpdateTarget,
    Usage, Version,
};

use super::Device;
use super::catch::EventStream;
use super::clip::ClipHandle;
use super::discover::BoxInfo;
use super::input::InputStream;
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

    /// `MOVE` (cursor) bypassing movement riding. Instant; see [`Device::move_rel_now`].
    pub fn move_rel_now(&self, dx: i16, dy: i16) -> Result<()> {
        self.dev().move_rel_now(dx, dy)
    }

    /// `MOVE` (wheel) bypassing movement riding. Instant; see [`Device::wheel_now`].
    pub fn wheel_now(&self, delta: i16) -> Result<()> {
        self.dev().wheel_now(delta)
    }

    /// Emit the motion held for a ride now. Instant; see [`Device::flush_motion`].
    pub fn flush_motion(&self) -> Result<()> {
        self.dev().flush_motion()
    }

    /// Drop the motion held for a ride. Instant; see [`Device::discard_motion`].
    pub fn discard_motion(&self) -> Result<()> {
        self.dev().discard_motion()
    }

    /// `MOVE`: field-generic relative axis (cursor or wheel). Instant; see [`Device::move_axis`].
    pub fn move_axis(
        &self,
        motion: Motion,
        timing: MoveTiming,
        pending: PendingMotion,
    ) -> Result<()> {
        self.dev().move_axis(motion, timing, pending)
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

    /// `LOCK`: weigh physical input on a target. Instant; see [`Device::scale`].
    pub fn scale(
        &self,
        target: impl Into<LockTarget>,
        direction: Direction,
        scale: u8,
    ) -> Result<()> {
        self.dev().scale(target, direction, scale)
    }

    /// `LOCK`: weigh a relative axis by sign. Instant; see [`Device::scale_axis`].
    pub fn scale_axis(&self, axis: Axis, direction: Direction, scale: u8) -> Result<()> {
        self.dev().scale_axis(axis, direction, scale)
    }

    /// `LOCK`: weigh a whole [`Blanket`] group. Instant; see [`Device::scale_all`].
    pub fn scale_all(&self, what: Blanket, direction: Direction, scale: u8) -> Result<()> {
        self.dev().scale_all(what, direction, scale)
    }

    /// `LOCK`: block a usage (button/key/media) or axis. Instant; see [`Device::lock`].
    pub fn lock(&self, target: impl Into<LockTarget>, direction: Direction) -> Result<()> {
        self.dev().lock(target, direction)
    }

    /// `LOCK`: release a locked usage or axis. Instant; see [`Device::unlock`].
    pub fn unlock(&self, target: impl Into<LockTarget>, direction: Direction) -> Result<()> {
        self.dev().unlock(target, direction)
    }

    /// `LOCK`: block a relative axis by sign. Instant; see [`Device::lock_axis`].
    pub fn lock_axis(&self, axis: Axis, direction: Direction) -> Result<()> {
        self.dev().lock_axis(axis, direction)
    }

    /// `LOCK`: release an axis lock. Instant; see [`Device::unlock_axis`].
    pub fn unlock_axis(&self, axis: Axis, direction: Direction) -> Result<()> {
        self.dev().unlock_axis(axis, direction)
    }

    /// `LOCK`: blanket-block a whole group. Instant; see [`Device::lock_all`].
    pub fn lock_all(&self, what: Blanket, direction: Direction) -> Result<()> {
        self.dev().lock_all(what, direction)
    }

    /// `LOCK`: release a blanket lock. Instant; see [`Device::unlock_all`].
    pub fn unlock_all(&self, what: Blanket, direction: Direction) -> Result<()> {
        self.dev().unlock_all(what, direction)
    }

    /// Subscribe to the catch stream. Instant; see [`Device::catch_events`].
    pub fn catch_events(
        &self,
        filters: impl IntoIterator<Item = CatchFilter>,
    ) -> Result<EventStream> {
        self.dev().catch_events(filters)
    }

    /// Subscribe to decoded input edges. Instant; see [`Device::input_events`].
    pub fn input_events(
        &self,
        filters: impl IntoIterator<Item = CatchFilter>,
    ) -> Result<InputStream> {
        self.dev().input_events(filters)
    }

    /// `OPTION(IMPERFECT)`: opt into cloning an over-capacity device. Instant; see [`Device::allow_imperfect_clones`].
    pub fn allow_imperfect_clones(&self, allow: bool) -> Result<()> {
        self.dev().allow_imperfect_clones(allow)
    }

    /// `OPTION(MOVE_RIDE)`: movement riding. Instant; see [`Device::set_movement_riding`].
    pub fn set_movement_riding(&self, window: Option<Duration>) -> Result<()> {
        self.dev().set_movement_riding(window)
    }

    /// `OPTION(BEARING)`: what `With`/`Against` are measured against. Instant; see [`Device::set_bearing`].
    pub fn set_bearing(&self, window: Option<Duration>, mode: BearingMode) -> Result<()> {
        self.dev().set_bearing(window, mode)
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

    /// Both chips' firmware versions and slot state (§4.16), awaiting the correlated `RESP`.
    pub async fn firmware_info(&self) -> Result<FirmwareInfo> {
        let payload = self
            .link
            .query_async(Q_FIRMWARE, self.link.query_timeout_default())
            .await?;
        match parse_resp(&payload) {
            Some(Resp::Firmware(f)) => Ok(f),
            _ => Err(Error::NoReply),
        }
    }

    /// Write one image into the target chip's spare slot; it stays inert until
    /// [`activate_firmware`](Self::activate_firmware).
    pub async fn stage_firmware(
        &self,
        target: UpdateTarget,
        image: Vec<u8>,
        mut progress: impl FnMut(UpdateProgress) + Send + 'static,
    ) -> Result<u32> {
        self.offload(move |d| d.stage_firmware(target, &image, &mut progress))
            .await
    }

    /// Drop whatever is staged or in flight for one target.
    pub async fn abort_update(&self, target: UpdateTarget) -> Result<()> {
        self.offload(move |d| d.abort_update(target)).await
    }

    /// Commit every staged image and reboot into it.
    pub async fn activate_firmware(&self) -> Result<()> {
        self.offload(|d| d.activate_firmware()).await
    }

    /// Stage one image and activate it.
    pub async fn update_firmware(
        &self,
        target: UpdateTarget,
        image: Vec<u8>,
        mut progress: impl FnMut(UpdateProgress) + Send + 'static,
    ) -> Result<()> {
        self.offload(move |d| {
            d.stage_firmware(target, &image, &mut progress)?;
            d.activate_firmware()
        })
        .await
    }

    /// Block until neither chip is still on probation.
    pub async fn wait_firmware_confirmed(&self) -> Result<FirmwareInfo> {
        self.offload(|d| d.wait_firmware_confirmed()).await
    }

    /// Run one blocking update call on a thread of its own and await the result.
    ///
    /// The transfer is a credit-windowed conversation with its own timeouts, and this crate carries
    /// no runtime and no timer, so an `async` reimplementation would have nothing to bound its waits
    /// with. Driving the sync path instead keeps one implementation of the wire, which is the whole
    /// point: a duplicated loop is where sync and async quietly stop agreeing.
    async fn offload<T, F>(&self, f: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&Device) -> Result<T> + Send + 'static,
    {
        let device = Device {
            link: self.link.clone(),
        };
        let (tx, rx) = flume::bounded(1);
        std::thread::Builder::new()
            .name("medius-update".into())
            .spawn(move || {
                let _ = tx.send(f(&device));
            })
            .map_err(Error::Io)?;
        rx.recv_async().await.map_err(|_| Error::Disconnected)?
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

    /// Query the bearing (§4.14), awaiting the correlated `RESP`.
    pub async fn query_bearing(&self) -> Result<Bearing> {
        let payload = self
            .link
            .query_option_async(OPT_BEARING, self.link.query_timeout_default())
            .await?;
        match parse_resp(&payload) {
            Some(Resp::Bearing(b)) => Ok(b),
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

    /// Set the autolock scope. Instant; see [`ClipHandle::set_autolock`](crate::ClipHandle::set_autolock).
    pub fn set_autolock(&self, scope: &[Blanket]) -> Result<()> {
        self.inner.set_autolock(scope)
    }

    /// Set loop mode. Instant; see [`ClipHandle::set_loop`](crate::ClipHandle::set_loop).
    pub fn set_loop(&self, on: bool) -> Result<()> {
        self.inner.set_loop(on)
    }

    /// Set retained mode. Instant; see [`ClipHandle::set_retain`](crate::ClipHandle::set_retain).
    pub fn set_retain(&self, on: bool) -> Result<()> {
        self.inner.set_retain(on)
    }

    /// Set whether clip motion rides. Instant; see [`ClipHandle::set_ride`](crate::ClipHandle::set_ride).
    pub fn set_ride(&self, on: bool) -> Result<()> {
        self.inner.set_ride(on)
    }

    /// Add or overwrite a trigger binding. Instant; see [`ClipHandle::bind`](crate::ClipHandle::bind).
    pub fn bind(&self, trigger: ClipTrigger) -> Result<()> {
        self.inner.bind(trigger)
    }

    /// Remove a trigger binding. Instant; see [`ClipHandle::unbind`](crate::ClipHandle::unbind).
    pub fn unbind(&self, usage: impl Into<Usage>, edge: Edge) -> Result<()> {
        self.inner.unbind(usage, edge)
    }

    /// Remove every trigger binding. Instant; see [`ClipHandle::clear_triggers`](crate::ClipHandle::clear_triggers).
    pub fn clear_triggers(&self) -> Result<()> {
        self.inner.clear_triggers()
    }

    /// Rewind and play. Instant; see [`ClipHandle::start`](crate::ClipHandle::start).
    pub fn start(&self) -> Result<()> {
        self.inner.start()
    }

    /// Stop playback. Instant; see [`ClipHandle::stop`](crate::ClipHandle::stop).
    pub fn stop(&self) -> Result<()> {
        self.inner.stop()
    }

    /// Halt mid-clip. Instant; see [`ClipHandle::pause`](crate::ClipHandle::pause).
    pub fn pause(&self) -> Result<()> {
        self.inner.pause()
    }

    /// Continue from the paused cursor. Instant; see [`ClipHandle::resume`](crate::ClipHandle::resume).
    pub fn resume(&self) -> Result<()> {
        self.inner.resume()
    }

    /// Force a rewind and play. Instant; see [`ClipHandle::restart`](crate::ClipHandle::restart).
    pub fn restart(&self) -> Result<()> {
        self.inner.restart()
    }

    /// Toggle play/stop. Instant; see [`ClipHandle::toggle`](crate::ClipHandle::toggle).
    pub fn toggle(&self) -> Result<()> {
        self.inner.toggle()
    }

    /// Discard the loaded clip. Instant; see [`ClipHandle::clear`](crate::ClipHandle::clear).
    pub fn clear(&self) -> Result<()> {
        self.inner.clear()
    }

    /// Finalize a retained clip. Instant; see [`ClipHandle::finalize`](crate::ClipHandle::finalize).
    pub fn finalize(&self) -> Result<()> {
        self.inner.finalize()
    }

    /// `QUERY(CLIP)`: the ring depth, progress, and playback counters, awaiting the correlated `RESP`. See [`ClipHandle::query_status`](crate::ClipHandle::query_status).
    pub async fn query_status(&self) -> Result<ClipStatus> {
        let link = self.inner.link();
        let payload = link
            .query_async(Q_CLIP, link.query_timeout_default())
            .await?;
        match parse_resp(&payload) {
            Some(Resp::Clip(s)) => Ok(s),
            _ => Err(Error::NoReply),
        }
    }

    /// `QUERY(CLIP)`: the clip configuration, awaiting the correlated `RESP`. See [`ClipHandle::query_config`](crate::ClipHandle::query_config).
    pub async fn query_config(&self) -> Result<ClipSettings> {
        let link = self.inner.link();
        let payload = link
            .query_async(Q_CLIP, link.query_timeout_default())
            .await?;
        ClipSettings::from_payload(&payload).ok_or(Error::NoReply)
    }
}
