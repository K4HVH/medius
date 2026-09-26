use std::time::Duration;

use crate::error::{Error, Result};
use crate::link::Link;
use crate::protocol::opcode::{
    OPT_BEARING, OPT_EMIT, OPT_IMPERFECT, OPT_MOVE_RIDE, OPT_RENDER, OPT_SPREAD, Q_CAPS, Q_CATCH,
    Q_CLIP, Q_DEVICE_INFO, Q_FIRMWARE, Q_HEALTH, Q_LOCKS, Q_PATCH_ENTRY, Q_PATCHES, Q_RATE,
    Q_REWRITE, Q_REWRITE_ENTRY, Q_STATS, Q_TRANSFORMS, Q_VERSION,
};
use crate::protocol::{Resp, parse_resp};
use crate::types::{
    Action, Axis, Bearing, BearingMode, Blanket, Caps, CatchFilter, CatchState, ClipBuilder,
    ClipPacketTrigger, ClipSettings, ClipStatus, ClipTrigger, CountersSnapshot, DeviceInfo,
    Direction, Edge, EmitPace, EmitPaceStatus, FirmwareInfo, Health, ImperfectStatus, LedMode,
    LedTarget, LockTarget, Locks, Motion, MoveTiming, Patch, PatchSet, PendingMotion, Rate,
    RebootTarget, RenderMode, RenderStatus, RewriteRule, RewriteTable, Setup, SpreadStatus, Stats,
    TransferOutcome, TransferStatus, Transform, Transforms, UpdateProgress, UpdateTarget, Usage,
    Version,
};

use super::Device;
use super::catch::EventStream;
use super::clip::ClipHandle;
use super::discover::BoxInfo;
use super::input::InputStream;
use super::logs::LogStream;
use super::raw::DEFAULT_TRANSFER_TIMEOUT;

/// Async view over a [`Device`]: the same `Link` core. Queries are `async` and await the correlated
/// `RESP` with the default timeout; commands send at once.
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
    /// [`AsyncDevice`] over the same core.
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

    /// Back to the sync [`Device`].
    pub fn into_inner(self) -> Device {
        Device { link: self.link }
    }

    /// `MOVE`: relative cursor movement; see [`Device::move_rel`].
    pub fn move_rel(&self, dx: i16, dy: i16) -> Result<()> {
        self.dev().move_rel(dx, dy)
    }

    /// `WHEEL`: vertical scroll; see [`Device::wheel`].
    pub fn wheel(&self, delta: i16) -> Result<()> {
        self.dev().wheel(delta)
    }

    /// `MOVE` (cursor) bypassing movement riding; see [`Device::move_rel_now`].
    pub fn move_rel_now(&self, dx: i16, dy: i16) -> Result<()> {
        self.dev().move_rel_now(dx, dy)
    }

    /// `MOVE` (wheel) bypassing movement riding; see [`Device::wheel_now`].
    pub fn wheel_now(&self, delta: i16) -> Result<()> {
        self.dev().wheel_now(delta)
    }

    /// `MOVE` (AC Pan): horizontal scroll; see [`Device::pan`].
    pub fn pan(&self, delta: i16) -> Result<()> {
        self.dev().pan(delta)
    }

    /// `MOVE` (AC Pan) bypassing movement riding; see [`Device::pan_now`].
    pub fn pan_now(&self, delta: i16) -> Result<()> {
        self.dev().pan_now(delta)
    }

    /// Emit motion held for a ride now; see [`Device::flush_motion`].
    pub fn flush_motion(&self) -> Result<()> {
        self.dev().flush_motion()
    }

    /// Drop motion held for a ride; see [`Device::discard_motion`].
    pub fn discard_motion(&self) -> Result<()> {
        self.dev().discard_motion()
    }

    /// `MOVE`: any relative axis (cursor, wheel, AC Pan); see [`Device::move_axis`].
    pub fn move_axis(
        &self,
        motion: Motion,
        timing: MoveTiming,
        pending: PendingMotion,
    ) -> Result<()> {
        self.dev().move_axis(motion, timing, pending)
    }

    /// `INJECT`: override a button, key or media usage; see [`Device::inject`].
    pub fn inject(&self, usage: impl Into<Usage>, action: Action) -> Result<()> {
        self.dev().inject(usage, action)
    }

    /// Press (force down) a usage; see [`Device::press`].
    pub fn press(&self, usage: impl Into<Usage>) -> Result<()> {
        self.dev().press(usage)
    }

    /// Soft-release a usage; see [`Device::release`].
    pub fn release(&self, usage: impl Into<Usage>) -> Result<()> {
        self.dev().release(usage)
    }

    /// Force-release a usage; see [`Device::force_release`].
    pub fn force_release(&self, usage: impl Into<Usage>) -> Result<()> {
        self.dev().force_release(usage)
    }

    /// `RESET`: return to passthrough; see [`Device::reset`].
    pub fn reset(&self) -> Result<()> {
        self.dev().reset()
    }

    /// `RESET`, erase the store and reboot; see [`Device::factory_reset`].
    pub fn factory_reset(&self) -> Result<()> {
        self.dev().factory_reset()
    }

    /// Reboot a chip (run or ROM download per the target); see [`Device::reboot`].
    pub fn reboot(&self, target: RebootTarget) -> Result<()> {
        self.dev().reboot(target)
    }

    /// Re-send every held override; see [`Device::reapply`].
    pub fn reapply(&self) -> Result<()> {
        self.dev().reapply()
    }

    /// Best-effort reconnect over the shared core; blocks the calling thread. See
    /// [`Device::reconnect`].
    pub fn reconnect(&self) -> Result<()> {
        self.dev().reconnect()
    }

    /// Snapshot of the always-on counters; see [`Device::counters`].
    pub fn counters(&self) -> CountersSnapshot {
        self.dev().counters()
    }

    /// [`LogStream`] over the device `LOG` stream, with `recv_async`; see [`Device::logs`].
    pub fn logs(&self) -> LogStream {
        self.dev().logs()
    }

    /// `LED`: override a status LED; see [`Device::led`].
    pub fn led(&self, target: LedTarget, mode: LedMode, level: u8) -> Result<()> {
        self.dev().led(target, mode, level)
    }

    /// `LOCK`: weigh physical input on a target; see [`Device::scale`].
    pub fn scale(
        &self,
        target: impl Into<LockTarget>,
        direction: Direction,
        scale: i16,
    ) -> Result<()> {
        self.dev().scale(target, direction, scale)
    }

    /// `LOCK`: weigh a relative axis by sign; see [`Device::scale_axis`].
    pub fn scale_axis(&self, axis: Axis, direction: Direction, scale: i16) -> Result<()> {
        self.dev().scale_axis(axis, direction, scale)
    }

    /// `LOCK`: weigh a [`Blanket`] group; see [`Device::scale_all`].
    pub fn scale_all(&self, what: Blanket, direction: Direction, scale: i16) -> Result<()> {
        self.dev().scale_all(what, direction, scale)
    }

    /// `LOCK`: block a usage (button/key/media) or axis; see [`Device::lock`].
    pub fn lock(&self, target: impl Into<LockTarget>, direction: Direction) -> Result<()> {
        self.dev().lock(target, direction)
    }

    /// `LOCK`: release a locked usage or axis; see [`Device::unlock`].
    pub fn unlock(&self, target: impl Into<LockTarget>, direction: Direction) -> Result<()> {
        self.dev().unlock(target, direction)
    }

    /// `LOCK`: block a relative axis by sign; see [`Device::lock_axis`].
    pub fn lock_axis(&self, axis: Axis, direction: Direction) -> Result<()> {
        self.dev().lock_axis(axis, direction)
    }

    /// `LOCK`: release an axis lock; see [`Device::unlock_axis`].
    pub fn unlock_axis(&self, axis: Axis, direction: Direction) -> Result<()> {
        self.dev().unlock_axis(axis, direction)
    }

    /// `LOCK`: block a whole group; see [`Device::lock_all`].
    pub fn lock_all(&self, what: Blanket, direction: Direction) -> Result<()> {
        self.dev().lock_all(what, direction)
    }

    /// `LOCK`: release a blanket lock; see [`Device::unlock_all`].
    pub fn unlock_all(&self, what: Blanket, direction: Direction) -> Result<()> {
        self.dev().unlock_all(what, direction)
    }

    /// Subscribe to the catch stream; see [`Device::catch_events`].
    pub fn catch_events(
        &self,
        filters: impl IntoIterator<Item = CatchFilter>,
    ) -> Result<EventStream> {
        self.dev().catch_events(filters)
    }

    /// Subscribe to decoded input edges; see [`Device::input_events`].
    pub fn input_events(
        &self,
        filters: impl IntoIterator<Item = CatchFilter>,
    ) -> Result<InputStream> {
        self.dev().input_events(filters)
    }

    /// `OPTION(IMPERFECT)`: allow cloning a device the box cannot clone exactly; see
    /// [`Device::allow_imperfect_clones`].
    pub fn allow_imperfect_clones(&self, allow: bool) -> Result<()> {
        self.dev().allow_imperfect_clones(allow)
    }

    /// `OPTION(MOVE_RIDE)`: movement riding; see [`Device::set_movement_riding`].
    pub fn set_movement_riding(&self, window: Option<Duration>) -> Result<()> {
        self.dev().set_movement_riding(window)
    }

    /// `OPTION(EMIT)`: emit pacing and forced wire rate; see [`Device::set_emit_pace`].
    pub fn set_emit_pace(&self, pace: EmitPace, force_hz: Option<u16>) -> Result<()> {
        self.dev().set_emit_pace(pace, force_hz)
    }

    /// `OPTION(NAME)`: box name; see [`Device::set_name`].
    pub fn set_name(&self, name: &str) -> Result<()> {
        self.dev().set_name(name)
    }

    /// `OPTION(NAME)` clear: revert to the derived default; see [`Device::clear_name`].
    pub fn clear_name(&self) -> Result<()> {
        self.dev().clear_name()
    }

    /// `OPTION(BEARING)`: what `With`/`Against` are measured against; see [`Device::set_bearing`].
    pub fn set_bearing(&self, window: Option<Duration>, mode: BearingMode) -> Result<()> {
        self.dev().set_bearing(window, mode)
    }

    /// `OPTION(RENDER)`: render texture, and whether native motion goes through it; see
    /// [`Device::set_render`].
    pub fn set_render(&self, mode: RenderMode, full: bool) -> Result<()> {
        self.dev().set_render(mode, full)
    }

    /// `OPTION(SPREAD)`: percent of the host's command interval an injected delta is released
    /// across; see [`Device::set_spread`].
    pub fn set_spread(&self, percent: u16) -> Result<()> {
        self.dev().set_spread(percent)
    }

    /// Box version.
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

    /// Both chips' firmware versions and slot state (§4.16).
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

    /// Write one image into the target chip's spare slot, inert until
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
    ///
    /// Not cancellable: dropping the future stops the wait, not the transfer, which runs to completion
    /// on its own thread, and the box still reboots into the new image.
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

    /// Wait until neither chip is on probation.
    pub async fn wait_firmware_confirmed(&self) -> Result<FirmwareInfo> {
        self.offload(|d| d.wait_firmware_confirmed()).await
    }

    // Not cancellable: dropping the future stops the wait, not the thread, and `update_firmware`
    // still reboots the box. The crate has no runtime or timer to bound async waits with, so it
    // drives the sync path and keeps one implementation of the wire.
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

    /// Box health flags.
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

    /// Cloned device's USB identity, kind and product (§4.3).
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

    /// Cloned device's capabilities (§4.4).
    pub async fn caps(&self) -> Result<Caps> {
        let payload = self
            .link
            .query_async(Q_CAPS, self.link.query_timeout_default())
            .await?;
        match parse_resp(&payload) {
            Some(Resp::Caps(c)) => {
                // As the sync path: a later button blanket covers every declared button.
                self.link
                    .desired()
                    .lock()
                    .note_declared_buttons(c.mouse.n_buttons);
                Ok(c)
            }
            _ => Err(Error::NoReply),
        }
    }

    /// Live native report rate (§4.5).
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

    /// Delivery and telemetry counters (§4.6).
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

    /// Active locks (§4.8).
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

    /// Live catch table, box-side drop counts and inter-chip clock (§4.9).
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

    /// Imperfect-clone status (§4.14).
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

    /// Movement-riding window (§4.14); `None` = off.
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

    /// Emit pacing mode and the rate in effect (§4.14).
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

    /// Bearing (§4.14).
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

    /// Render texture and whether a profile has armed (§4.14).
    pub async fn query_render(&self) -> Result<RenderStatus> {
        let payload = self
            .link
            .query_option_async(OPT_RENDER, self.link.query_timeout_default())
            .await?;
        match parse_resp(&payload) {
            Some(Resp::Render(s)) => Ok(s),
            _ => Err(Error::NoReply),
        }
    }

    /// Spread percent and the interval in effect (§4.14).
    pub async fn query_spread(&self) -> Result<SpreadStatus> {
        let payload = self
            .link
            .query_option_async(OPT_SPREAD, self.link.query_timeout_default())
            .await?;
        match parse_resp(&payload) {
            Some(Resp::Spread(s)) => Ok(s),
            _ => Err(Error::NoReply),
        }
    }

    // The advanced control layer (§3.14).
    async fn require_imperfect(&self) -> Result<()> {
        if self.query_imperfect().await?.allowed {
            Ok(())
        } else {
            Err(Error::ImperfectRequired)
        }
    }

    /// `RAW`: put raw bytes on a cloned endpoint in a direction; see [`Device::raw`].
    // Nothing to await, but it stays a future: every caller awaits it.
    pub async fn raw(&self, ep: u8, direction: Direction, bytes: &[u8]) -> Result<()> {
        self.dev().raw(ep, direction, bytes)
    }

    /// `TRANSFER`: one control transfer against the device; see [`Device::transfer`].
    pub async fn transfer(&self, ep: u8, setup: Setup, out: &[u8]) -> Result<TransferOutcome> {
        self.transfer_timeout(ep, setup, out, DEFAULT_TRANSFER_TIMEOUT)
            .await
    }

    /// [`transfer`](Self::transfer) with an explicit reply timeout; see [`Device::transfer_timeout`].
    pub async fn transfer_timeout(
        &self,
        ep: u8,
        setup: Setup,
        out: &[u8],
        timeout: Duration,
    ) -> Result<TransferOutcome> {
        let (status, data) = self.link.transfer_async(ep, setup, out, timeout).await?;
        Ok(TransferOutcome {
            status: TransferStatus::from_u8(status),
            data,
        })
    }

    /// `REWRITE`: add one rewrite rule; see [`Device::set_rewrite`].
    pub async fn set_rewrite(&self, rule: &RewriteRule) -> Result<()> {
        crate::device::rewrite::validate_rule(rule)?;
        self.require_imperfect().await?;
        self.dev().set_rewrite_checked(rule)
    }

    /// `REWRITE` remove: drop one rule by key; see [`Device::remove_rewrite`].
    pub fn remove_rewrite(&self, rule: &RewriteRule) -> Result<()> {
        self.dev().remove_rewrite(rule)
    }

    /// `REWRITE` clear: drop the whole table; see [`Device::clear_rewrite`].
    pub fn clear_rewrite(&self) -> Result<()> {
        self.dev().clear_rewrite()
    }

    /// `QUERY(REWRITE)`: rewrite-table summary; see [`Device::query_rewrite`].
    pub async fn query_rewrite(&self) -> Result<RewriteTable> {
        let payload = self
            .link
            .query_async(Q_REWRITE, self.link.query_timeout_default())
            .await?;
        match parse_resp(&payload) {
            Some(Resp::Rewrite(t)) => Ok(t),
            _ => Err(Error::NoReply),
        }
    }

    /// `QUERY(REWRITE_ENTRY)`: one rule in full; see [`Device::query_rewrite_entry`].
    pub async fn query_rewrite_entry(&self, index: u8) -> Result<RewriteRule> {
        let payload = self
            .link
            .query_indexed_async(Q_REWRITE_ENTRY, index, self.link.query_timeout_default())
            .await?;
        crate::types::rewrite::rewrite_entry_from_payload(&payload).ok_or(Error::NoReply)
    }

    /// `PATCH`: store one descriptor patch; see [`Device::set_patch`].
    pub fn set_patch(&self, patch: &Patch) -> Result<()> {
        self.dev().set_patch(patch)
    }

    /// `PATCH` APPLY: re-present the clone with the stored set; see [`Device::apply_patch`].
    pub async fn apply_patch(&self) -> Result<()> {
        self.require_imperfect().await?;
        self.dev().apply_patch_send()
    }

    /// `PATCH` CLEAR: erase this device's stored set; see [`Device::clear_patch`].
    pub fn clear_patch(&self) -> Result<()> {
        self.dev().clear_patch()
    }

    /// `QUERY(PATCHES)`: patch set summary; see [`Device::query_patches`].
    pub async fn query_patches(&self) -> Result<PatchSet> {
        let payload = self
            .link
            .query_async(Q_PATCHES, self.link.query_timeout_default())
            .await?;
        match parse_resp(&payload) {
            Some(Resp::Patches(s)) => Ok(s),
            _ => Err(Error::NoReply),
        }
    }

    /// `QUERY(PATCH_ENTRY)`: one patch in full; see [`Device::query_patch_entry`].
    pub async fn query_patch_entry(&self, index: u8) -> Result<Patch> {
        let payload = self
            .link
            .query_indexed_async(Q_PATCH_ENTRY, index, self.link.query_timeout_default())
            .await?;
        crate::types::patch::patch_entry_from_payload(&payload).ok_or(Error::NoReply)
    }

    /// `TRANSFORM`: add one field transform (ungated); see [`Device::transform`].
    pub fn transform(&self, t: &Transform) -> Result<()> {
        self.dev().transform(t)
    }

    /// `TRANSFORM` remove: drop one transform by key; see [`Device::untransform`].
    pub fn untransform(&self, t: &Transform) -> Result<()> {
        self.dev().untransform(t)
    }

    /// `TRANSFORM` clear: drop the whole table; see [`Device::clear_transforms`].
    pub fn clear_transforms(&self) -> Result<()> {
        self.dev().clear_transforms()
    }

    /// `TRANSFORM`: exchange two axes; see [`Device::transform_swap`].
    pub fn transform_swap(&self, a: Axis, b: Axis) -> Result<()> {
        self.dev().transform_swap(a, b)
    }

    /// `TRANSFORM`: remap a source field into a destination; see [`Device::transform_remap`].
    pub fn transform_remap(
        &self,
        source: impl Into<LockTarget>,
        dest: impl Into<LockTarget>,
    ) -> Result<()> {
        self.dev().transform_remap(source, dest)
    }

    /// `QUERY(TRANSFORMS)`: transform table; see [`Device::query_transforms`].
    pub async fn query_transforms(&self) -> Result<Transforms> {
        let payload = self
            .link
            .query_async(Q_TRANSFORMS, self.link.query_timeout_default())
            .await?;
        match parse_resp(&payload) {
            Some(Resp::Transforms(t)) => Ok(t),
            _ => Err(Error::NoReply),
        }
    }

    /// Buffered-clip playback (§3.11); see [`Device::clip`].
    pub fn clip(&self) -> AsyncClipHandle {
        AsyncClipHandle {
            inner: self.dev().clip(),
        }
    }

    /// Open the box at `path` as an [`AsyncDevice`].
    pub fn open(path: impl AsRef<std::path::Path>) -> Result<AsyncDevice> {
        Ok(Device::open(path)?.into_async())
    }

    /// Open the first box found as an [`AsyncDevice`]; blocks. See [`Device::find`].
    pub fn find() -> Result<AsyncDevice> {
        Ok(Device::find()?.into_async())
    }

    /// Every connected box; blocks (a scan and a version read per box). See [`Device::list`].
    pub fn list() -> Vec<BoxInfo> {
        Device::list()
    }

    /// Open the box whose device MAC or CH343 serial matches `id`; see [`Device::open_by_id`].
    pub fn open_by_id(id: &str) -> Result<AsyncDevice> {
        Ok(Device::open_by_id(id)?.into_async())
    }

    /// Open the first box cloning a mouse; see [`Device::find_mouse_box`].
    pub fn find_mouse_box() -> Result<AsyncDevice> {
        Ok(Device::find_mouse_box()?.into_async())
    }

    /// Open the first box cloning a keyboard; see [`Device::find_keyboard_box`].
    pub fn find_keyboard_box() -> Result<AsyncDevice> {
        Ok(Device::find_keyboard_box()?.into_async())
    }
}

/// Async view over a [`ClipHandle`](crate::ClipHandle) (§3.11); see [`AsyncDevice::clip`].
#[derive(Clone, Debug)]
pub struct AsyncClipHandle {
    inner: ClipHandle,
}

impl AsyncClipHandle {
    /// Append entries to the ring; see [`ClipHandle::append`](crate::ClipHandle::append).
    pub fn append(&self, clip: &ClipBuilder) -> Result<()> {
        self.inner.append(clip)
    }

    /// Autolock scope; see [`ClipHandle::set_autolock`](crate::ClipHandle::set_autolock).
    pub fn set_autolock(&self, scope: &[Blanket]) -> Result<()> {
        self.inner.set_autolock(scope)
    }

    /// Loop mode; see [`ClipHandle::set_loop`](crate::ClipHandle::set_loop).
    pub fn set_loop(&self, on: bool) -> Result<()> {
        self.inner.set_loop(on)
    }

    /// Retained mode; see [`ClipHandle::set_retain`](crate::ClipHandle::set_retain).
    pub fn set_retain(&self, on: bool) -> Result<()> {
        self.inner.set_retain(on)
    }

    /// Whether clip motion rides; see [`ClipHandle::set_ride`](crate::ClipHandle::set_ride).
    pub fn set_ride(&self, on: bool) -> Result<()> {
        self.inner.set_ride(on)
    }

    /// Add or overwrite an input trigger; see [`ClipHandle::bind`](crate::ClipHandle::bind).
    pub fn bind(&self, trigger: ClipTrigger) -> Result<()> {
        self.inner.bind(trigger)
    }

    /// Remove an input trigger; see [`ClipHandle::unbind`](crate::ClipHandle::unbind).
    pub fn unbind(&self, usage: impl Into<Usage>, edge: Edge) -> Result<()> {
        self.inner.unbind(usage, edge)
    }

    /// Add or overwrite a packet trigger; see
    /// [`ClipHandle::bind_packet`](crate::ClipHandle::bind_packet).
    pub fn bind_packet(&self, trigger: &ClipPacketTrigger) -> Result<()> {
        self.inner.bind_packet(trigger)
    }

    /// Remove a packet trigger; see [`ClipHandle::unbind_packet`](crate::ClipHandle::unbind_packet).
    pub fn unbind_packet(&self, trigger: &ClipPacketTrigger) -> Result<()> {
        self.inner.unbind_packet(trigger)
    }

    /// Remove every trigger of both kinds; see
    /// [`ClipHandle::clear_triggers`](crate::ClipHandle::clear_triggers).
    pub fn clear_triggers(&self) -> Result<()> {
        self.inner.clear_triggers()
    }

    /// Rewind and play; see [`ClipHandle::start`](crate::ClipHandle::start).
    pub fn start(&self) -> Result<()> {
        self.inner.start()
    }

    /// Stop playback; see [`ClipHandle::stop`](crate::ClipHandle::stop).
    pub fn stop(&self) -> Result<()> {
        self.inner.stop()
    }

    /// Halt mid-clip; see [`ClipHandle::pause`](crate::ClipHandle::pause).
    pub fn pause(&self) -> Result<()> {
        self.inner.pause()
    }

    /// Continue from the paused cursor; see [`ClipHandle::resume`](crate::ClipHandle::resume).
    pub fn resume(&self) -> Result<()> {
        self.inner.resume()
    }

    /// Force a rewind and play; see [`ClipHandle::restart`](crate::ClipHandle::restart).
    pub fn restart(&self) -> Result<()> {
        self.inner.restart()
    }

    /// Toggle play/stop; see [`ClipHandle::toggle`](crate::ClipHandle::toggle).
    pub fn toggle(&self) -> Result<()> {
        self.inner.toggle()
    }

    /// Discard the loaded clip; see [`ClipHandle::clear`](crate::ClipHandle::clear).
    pub fn clear(&self) -> Result<()> {
        self.inner.clear()
    }

    /// Finalize a retained clip; see [`ClipHandle::finalize`](crate::ClipHandle::finalize).
    pub fn finalize(&self) -> Result<()> {
        self.inner.finalize()
    }

    /// Whether the box dropped the appended clip; see [`ClipHandle::lost`](crate::ClipHandle::lost).
    pub fn lost(&self) -> bool {
        self.inner.lost()
    }

    /// `QUERY(CLIP)`: ring depth, progress and playback counters; see
    /// [`ClipHandle::query_status`](crate::ClipHandle::query_status).
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

    /// `QUERY(CLIP)`: clip config; see [`ClipHandle::query_config`](crate::ClipHandle::query_config).
    pub async fn query_config(&self) -> Result<ClipSettings> {
        let link = self.inner.link();
        let payload = link
            .query_async(Q_CLIP, link.query_timeout_default())
            .await?;
        ClipSettings::from_payload(&payload).ok_or(Error::NoReply)
    }
}
