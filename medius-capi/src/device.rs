//! The opaque `MediusDevice` handle and every command, query, and lifecycle function.

use std::ffi::CStr;
use std::os::raw::{c_char, c_void};
use std::time::Duration;

use medius::Device;
use medius::{UpdateProgress, UpdateTarget};

use crate::convert::{
    action_from_c, blanket_from_c, emit_pace_from_c, input_to_medius, led_mode_from_c,
    led_target_from_c, lock_target_to_medius, motion_from_c, move_timing_from_c,
    pending_motion_from_c, reboot_target_from_c,
};
use crate::ctypes::*;
use crate::error::{MediusStatus, clear_error, fail, guard, guard_status, record, status_of};

/// An open connection to one medius box; create with `medius_device_open`/`_find` and free with `medius_device_free`.
pub struct MediusDevice {
    pub(crate) inner: Device,
}

impl MediusDevice {
    pub(crate) fn boxed(inner: Device) -> *mut MediusDevice {
        Box::into_raw(Box::new(MediusDevice { inner }))
    }
}

fn with_device(
    dev: *mut MediusDevice,
    f: impl FnOnce(&Device) -> Result<(), medius::Error>,
) -> MediusStatus {
    guard_status(|| {
        if dev.is_null() {
            return fail(MediusStatus::ErrInvalidArg, "null device handle");
        }
        let d = unsafe { &(*dev).inner };
        status_of(f(d))
    })
}

fn query<T, M: From<T>>(
    dev: *mut MediusDevice,
    out: *mut M,
    f: impl FnOnce(&Device) -> Result<T, medius::Error>,
) -> MediusStatus {
    guard_status(|| {
        if dev.is_null() || out.is_null() {
            return fail(MediusStatus::ErrInvalidArg, "null pointer");
        }
        let d = unsafe { &(*dev).inner };
        match f(d) {
            Ok(v) => {
                unsafe { *out = M::from(v) };
                clear_error();
                MediusStatus::Ok
            }
            Err(e) => record(&e),
        }
    })
}

/// Open the box at serial `path` (NUL-terminated UTF-8), handshake, and write the owned handle to `*out`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_open(
    path: *const c_char,
    out: *mut *mut MediusDevice,
) -> MediusStatus {
    guard_status(|| {
        if path.is_null() || out.is_null() {
            return fail(MediusStatus::ErrInvalidArg, "null pointer");
        }
        let Ok(s) = (unsafe { CStr::from_ptr(path) }).to_str() else {
            return fail(MediusStatus::ErrInvalidArg, "path is not valid UTF-8");
        };
        match Device::open(s) {
            Ok(dev) => {
                unsafe { *out = MediusDevice::boxed(dev) };
                clear_error();
                MediusStatus::Ok
            }
            Err(e) => record(&e),
        }
    })
}

/// Discover the first medius box by USB id, open it, handshake, and write the handle to `*out`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_find(out: *mut *mut MediusDevice) -> MediusStatus {
    guard_status(|| {
        if out.is_null() {
            return fail(MediusStatus::ErrInvalidArg, "null pointer");
        }
        match Device::find() {
            Ok(dev) => {
                unsafe { *out = MediusDevice::boxed(dev) };
                clear_error();
                MediusStatus::Ok
            }
            Err(e) => record(&e),
        }
    })
}

/// Clone a device handle into another owner of the same reference-counted connection; each clone must be freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_clone(dev: *const MediusDevice) -> *mut MediusDevice {
    guard(std::ptr::null_mut(), || {
        if dev.is_null() {
            return std::ptr::null_mut();
        }
        MediusDevice::boxed(unsafe { (*dev).inner.clone() })
    })
}

/// Free a device handle; joins the background threads when the last clone drops, null is a no-op.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_free(dev: *mut MediusDevice) {
    guard((), || {
        if !dev.is_null() {
            drop(unsafe { Box::from_raw(dev) });
        }
    });
}

/// Enumerate medius serial ports into `out` (up to `cap`); writes the total to `*out_total` and returns the number written.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_find_ports(
    out: *mut MediusPortInfo,
    cap: usize,
    out_total: *mut usize,
) -> usize {
    guard(0, || {
        let ports: Vec<MediusPortInfo> = medius::find_medius()
            .iter()
            .filter_map(crate::convert::port_to_medius)
            .collect();
        let total = ports.len();
        if !out_total.is_null() {
            unsafe { *out_total = total };
        }
        if out.is_null() {
            return 0;
        }
        let n = total.min(cap);
        for (i, port) in ports.iter().take(n).enumerate() {
            unsafe { *out.add(i) = *port };
        }
        n
    })
}

/// Enumerate every connected box into `out` (up to `cap`), opening each in turn; writes the total to `*out_total` and returns the number written.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_list(
    out: *mut MediusBoxInfo,
    cap: usize,
    out_total: *mut usize,
) -> usize {
    guard(0, || {
        let boxes: Vec<MediusBoxInfo> = Device::list()
            .iter()
            .filter_map(crate::convert::box_to_medius)
            .collect();
        let total = boxes.len();
        if !out_total.is_null() {
            unsafe { *out_total = total };
        }
        if out.is_null() {
            return 0;
        }
        let n = total.min(cap);
        for (i, bx) in boxes.iter().take(n).enumerate() {
            unsafe { *out.add(i) = *bx };
        }
        n
    })
}

/// Open the box whose identity matches `id` (device MAC hex or CH343 serial), handshake, and write the handle to `*out`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_open_by_id(
    id: *const c_char,
    out: *mut *mut MediusDevice,
) -> MediusStatus {
    guard_status(|| {
        if id.is_null() || out.is_null() {
            return fail(MediusStatus::ErrInvalidArg, "null pointer");
        }
        let Ok(s) = (unsafe { CStr::from_ptr(id) }).to_str() else {
            return fail(MediusStatus::ErrInvalidArg, "id is not valid UTF-8");
        };
        match Device::open_by_id(s) {
            Ok(dev) => {
                unsafe { *out = MediusDevice::boxed(dev) };
                clear_error();
                MediusStatus::Ok
            }
            Err(e) => record(&e),
        }
    })
}

/// Open the first box whose clone is a mouse, handshake, and write the handle to `*out`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_find_mouse_box(out: *mut *mut MediusDevice) -> MediusStatus {
    guard_status(|| {
        if out.is_null() {
            return fail(MediusStatus::ErrInvalidArg, "null pointer");
        }
        match Device::find_mouse_box() {
            Ok(dev) => {
                unsafe { *out = MediusDevice::boxed(dev) };
                clear_error();
                MediusStatus::Ok
            }
            Err(e) => record(&e),
        }
    })
}

/// Open the first box whose clone is a keyboard, handshake, and write the handle to `*out`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_find_keyboard_box(
    out: *mut *mut MediusDevice,
) -> MediusStatus {
    guard_status(|| {
        if out.is_null() {
            return fail(MediusStatus::ErrInvalidArg, "null pointer");
        }
        match Device::find_keyboard_box() {
            Ok(dev) => {
                unsafe { *out = MediusDevice::boxed(dev) };
                clear_error();
                MediusStatus::Ok
            }
            Err(e) => record(&e),
        }
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_move_rel(
    dev: *mut MediusDevice,
    dx: i16,
    dy: i16,
) -> MediusStatus {
    with_device(dev, |d| d.move_rel(dx, dy))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_wheel(dev: *mut MediusDevice, delta: i16) -> MediusStatus {
    with_device(dev, |d| d.wheel(delta))
}

/// A cursor move that bypasses movement riding: it emits on the box's own clock.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_move_rel_now(
    dev: *mut MediusDevice,
    dx: i16,
    dy: i16,
) -> MediusStatus {
    with_device(dev, |d| d.move_rel_now(dx, dy))
}

/// A wheel move that bypasses movement riding.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_wheel_now(
    dev: *mut MediusDevice,
    delta: i16,
) -> MediusStatus {
    with_device(dev, |d| d.wheel_now(delta))
}

/// Emit the motion held for a ride now, ignoring the ride window.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_flush_motion(dev: *mut MediusDevice) -> MediusStatus {
    with_device(dev, |d| d.flush_motion())
}

/// Drop the motion held for a ride.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_discard_motion(dev: *mut MediusDevice) -> MediusStatus {
    with_device(dev, |d| d.discard_motion())
}

/// Drive one relative axis. `motion.kind` takes a `MEDIUS_MOTION_KIND_*` constant, `timing` a
/// `MEDIUS_MOVE_TIMING_*` one and `pending` a `MEDIUS_PENDING_MOTION_*` one; any other value is
/// `MEDIUS_STATUS_ERR_INVALID_ARG`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_move_axis(
    dev: *mut MediusDevice,
    motion: MediusMotion,
    timing: u8,
    pending: u8,
) -> MediusStatus {
    guard_status(|| {
        if dev.is_null() {
            return fail(MediusStatus::ErrInvalidArg, "null device handle");
        }
        let (Some(motion), Some(timing), Some(pending)) = (
            motion_from_c(motion),
            move_timing_from_c(timing),
            pending_motion_from_c(pending),
        ) else {
            return fail(
                MediusStatus::ErrInvalidArg,
                "invalid motion, timing or pending",
            );
        };
        status_of(unsafe { &(*dev).inner }.move_axis(motion, timing, pending))
    })
}

fn with_input(
    dev: *mut MediusDevice,
    input: MediusUsage,
    f: impl FnOnce(&Device, medius::Usage) -> Result<(), medius::Error>,
) -> MediusStatus {
    guard_status(|| {
        if dev.is_null() {
            return fail(MediusStatus::ErrInvalidArg, "null device handle");
        }
        let Some(u) = input_to_medius(input) else {
            return fail(MediusStatus::ErrInvalidArg, "invalid input value");
        };
        let d = unsafe { &(*dev).inner };
        status_of(f(d, u))
    })
}

/// Drive one momentary usage (button, key, or media) with an explicit action. The one injection verb.
/// `action` takes a `MEDIUS_ACTION_*` constant; any other value is `MEDIUS_STATUS_ERR_INVALID_ARG`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_inject(
    dev: *mut MediusDevice,
    input: MediusUsage,
    action: u8,
) -> MediusStatus {
    let Some(action) = action_from_c(action) else {
        return fail(MediusStatus::ErrInvalidArg, "invalid injection action");
    };
    with_input(dev, input, |d, u| d.inject(u, action))
}

/// Press a usage (`Action::Press`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_press(
    dev: *mut MediusDevice,
    input: MediusUsage,
) -> MediusStatus {
    with_input(dev, input, |d, u| d.press(u))
}

/// Soft-release a usage: clear an injected press, leaving a physical hold intact.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_soft_release(
    dev: *mut MediusDevice,
    input: MediusUsage,
) -> MediusStatus {
    with_input(dev, input, |d, u| d.release(u))
}

/// Force-release a usage: mask a physical hold too.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_force_release(
    dev: *mut MediusDevice,
    input: MediusUsage,
) -> MediusStatus {
    with_input(dev, input, |d, u| d.force_release(u))
}

fn with_lock_target(
    dev: *mut MediusDevice,
    target: MediusLockTarget,
    dir: u8,
    f: impl FnOnce(&Device, medius::LockTarget, medius::Direction) -> Result<(), medius::Error>,
) -> MediusStatus {
    guard_status(|| {
        if dev.is_null() {
            return fail(MediusStatus::ErrInvalidArg, "null device handle");
        }
        let Some(t) = lock_target_to_medius(target) else {
            return fail(MediusStatus::ErrInvalidArg, "invalid lock target");
        };
        let Some(dir) = medius::Direction::from_u8(dir) else {
            return fail(MediusStatus::ErrInvalidArg, "invalid direction");
        };
        let d = unsafe { &(*dev).inner };
        status_of(f(d, t, dir))
    })
}

fn with_blanket(
    dev: *mut MediusDevice,
    what: u8,
    dir: u8,
    f: impl FnOnce(&Device, medius::Blanket, medius::Direction) -> Result<(), medius::Error>,
) -> MediusStatus {
    guard_status(|| {
        if dev.is_null() {
            return fail(MediusStatus::ErrInvalidArg, "null device handle");
        }
        let Some(what) = blanket_from_c(what) else {
            return fail(MediusStatus::ErrInvalidArg, "invalid blanket group");
        };
        let Some(dir) = medius::Direction::from_u8(dir) else {
            return fail(MediusStatus::ErrInvalidArg, "invalid direction");
        };
        let d = unsafe { &(*dev).inner };
        status_of(f(d, what, dir))
    })
}

/// Weigh physical input on a target and direction. `scale` is the percent of the physical value the
/// box keeps: `MEDIUS_LOCK_SCALE_BLOCK` blocks it, `MEDIUS_LOCK_SCALE_PASS` passes it untouched, and
/// above that amplifies to `MEDIUS_LOCK_SCALE_MAX` (2.55x). Lock and unlock are its two ends.
///
/// A delta picks up at most two scales, its absolute direction's and its relative direction's, and
/// they multiply. `MEDIUS_DIRECTION_BOTH` is the exception: it writes the scale to the two fixed
/// signs and a full pass to the relative pair, so a `Both` of 50 is 50% with or without a bearing
/// rather than 25% with one. Name a relative direction to weigh it.
///
/// `MEDIUS_DIRECTION_WITH` / `_AGAINST` need a live bearing (see `medius_device_set_bearing`) and
/// only an axis has one, so either on a button, key or media usage is
/// `MEDIUS_STATUS_ERR_RELATIVE_DIRECTION`. A momentary usage carries one bit, so any scale below a full
/// pass locks it and any scale at or above one unlocks it. A media usage has no edges and is sent as
/// `MEDIUS_DIRECTION_BOTH` whatever edge is named, which is what `RESP(LOCKS)` reports it as.
///
/// `dir` takes a `MEDIUS_DIRECTION_*` constant; any other value is
/// `MEDIUS_STATUS_ERR_INVALID_ARG`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_scale(
    dev: *mut MediusDevice,
    target: MediusLockTarget,
    dir: u8,
    scale: u8,
) -> MediusStatus {
    with_lock_target(dev, target, dir, |d, t, dir| d.scale(t, dir, scale))
}

/// Weigh a whole class blanket (cursor aim, wheel, all buttons, all keys, or all media). `what` takes
/// a `MEDIUS_BLANKET_*` constant and `dir` a `MEDIUS_DIRECTION_*` one; any other value is
/// `MEDIUS_STATUS_ERR_INVALID_ARG`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_scale_all(
    dev: *mut MediusDevice,
    what: u8,
    dir: u8,
    scale: u8,
) -> MediusStatus {
    with_blanket(dev, what, dir, |d, what, dir| d.scale_all(what, dir, scale))
}

/// Lock a target (axis or usage) on an edge. A button, key, and media usage all lock the same way.
/// `dir` takes a `MEDIUS_DIRECTION_*` constant; any other value is `MEDIUS_STATUS_ERR_INVALID_ARG`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_lock(
    dev: *mut MediusDevice,
    target: MediusLockTarget,
    dir: u8,
) -> MediusStatus {
    with_lock_target(dev, target, dir, |d, t, dir| d.lock(t, dir))
}

/// Release a lock set by `medius_device_lock`. `dir` takes a `MEDIUS_DIRECTION_*` constant; any other
/// value is `MEDIUS_STATUS_ERR_INVALID_ARG`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_unlock(
    dev: *mut MediusDevice,
    target: MediusLockTarget,
    dir: u8,
) -> MediusStatus {
    with_lock_target(dev, target, dir, |d, t, dir| d.unlock(t, dir))
}

/// Lock a whole class blanket (cursor aim, wheel, all buttons, all keys, or all media).
///
/// `MEDIUS_BLANKET_KEYS` honours the direction: `Positive` blocks press edges only, `Negative`
/// release edges only. `what` takes a `MEDIUS_BLANKET_*` constant and `dir` a `MEDIUS_DIRECTION_*`
/// one; any other value is `MEDIUS_STATUS_ERR_INVALID_ARG`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_lock_all(
    dev: *mut MediusDevice,
    what: u8,
    dir: u8,
) -> MediusStatus {
    with_blanket(dev, what, dir, |d, what, dir| d.lock_all(what, dir))
}

/// Release a blanket lock set by `medius_device_lock_all`. `what` takes a `MEDIUS_BLANKET_*` constant
/// and `dir` a `MEDIUS_DIRECTION_*` one; any other value is `MEDIUS_STATUS_ERR_INVALID_ARG`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_unlock_all(
    dev: *mut MediusDevice,
    what: u8,
    dir: u8,
) -> MediusStatus {
    with_blanket(dev, what, dir, |d, what, dir| d.unlock_all(what, dir))
}

/// Drive a status LED. `target` takes a `MEDIUS_LED_TARGET_*` constant and `mode` a
/// `MEDIUS_LED_MODE_*` one; any other value is `MEDIUS_STATUS_ERR_INVALID_ARG`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_led(
    dev: *mut MediusDevice,
    target: u8,
    mode: u8,
    level: u8,
) -> MediusStatus {
    let (Some(target), Some(mode)) = (led_target_from_c(target), led_mode_from_c(mode)) else {
        return fail(MediusStatus::ErrInvalidArg, "invalid LED target or mode");
    };
    with_device(dev, |d| d.led(target, mode, level))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_reset(dev: *mut MediusDevice) -> MediusStatus {
    with_device(dev, |d| d.reset())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_reapply(dev: *mut MediusDevice) -> MediusStatus {
    with_device(dev, |d| d.reapply())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_reconnect(dev: *mut MediusDevice) -> MediusStatus {
    with_device(dev, |d| d.reconnect())
}

/// Reboot a chip. `target` takes a `MEDIUS_REBOOT_TARGET_*` constant; any other value is
/// `MEDIUS_STATUS_ERR_INVALID_ARG`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_reboot(dev: *mut MediusDevice, target: u8) -> MediusStatus {
    let Some(target) = reboot_target_from_c(target) else {
        return fail(MediusStatus::ErrInvalidArg, "invalid reboot target");
    };
    with_device(dev, |d| d.reboot(target))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_allow_imperfect_clones(
    dev: *mut MediusDevice,
    allow: bool,
) -> MediusStatus {
    with_device(dev, |d| d.allow_imperfect_clones(allow))
}

/// Set movement riding; when `enabled`, injected motion rides a native cursor report seen within `window_ms`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_set_movement_riding(
    dev: *mut MediusDevice,
    enabled: bool,
    window_ms: u32,
) -> MediusStatus {
    let window = enabled.then(|| Duration::from_millis(window_ms as u64));
    with_device(dev, |d| d.set_movement_riding(window))
}

/// Set the bearing: what `MEDIUS_DIRECTION_WITH` / `_AGAINST` are measured against. `window_ms` is how
/// long the last injected delta's direction stays the bearing; 0 turns it off, leaving the relative
/// directions inert whatever their scale. The box boots at `MEDIUS_BEARING_WINDOW_DEFAULT_MS`.
///
/// `mode` takes a `MEDIUS_BEARING_MODE_*` constant; any other value is
/// `MEDIUS_STATUS_ERR_INVALID_ARG`. Both fields ride one frame and the box persists them together,
/// so a window change carries the mode with it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_set_bearing(
    dev: *mut MediusDevice,
    window_ms: u16,
    mode: u8,
) -> MediusStatus {
    guard_status(|| {
        if dev.is_null() {
            return fail(MediusStatus::ErrInvalidArg, "null device handle");
        }
        let Some(mode) = medius::BearingMode::from_u8(mode) else {
            return fail(MediusStatus::ErrInvalidArg, "invalid bearing mode");
        };
        let window = (window_ms != 0).then(|| Duration::from_millis(window_ms as u64));
        status_of(unsafe { &(*dev).inner }.set_bearing(window, mode))
    })
}

/// Set what paces injected motion and what rate the clone runs at; `hz` is the target rate for `Fixed`
/// and ignored otherwise, `force_hz` is the forced wire rate (0 = the device's own). `mode` takes a
/// `MEDIUS_EMIT_MODE_*` constant; any other value is `MEDIUS_STATUS_ERR_INVALID_ARG`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_set_emit_pace(
    dev: *mut MediusDevice,
    mode: u8,
    hz: u16,
    force_hz: u16,
) -> MediusStatus {
    let Some(pace) = emit_pace_from_c(mode, hz) else {
        return fail(MediusStatus::ErrInvalidArg, "invalid emit pacing mode");
    };
    with_device(dev, |d| {
        d.set_emit_pace(pace, (force_hz != 0).then_some(force_hz))
    })
}

/// Set the texture the box draws motion with, and whether the device's own motion is drawn by the
/// model rather than relayed. `mode` takes a `MEDIUS_RENDER_MODE_*` constant and any other value is
/// `MEDIUS_STATUS_ERR_INVALID_ARG`. `full` costs roughly 3 ms of latency on physical mouse movement
/// and is off by default.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_set_render(
    dev: *mut MediusDevice,
    mode: u8,
    full: bool,
) -> MediusStatus {
    let Some(mode) = medius::RenderMode::from_u8(mode) else {
        return fail(MediusStatus::ErrInvalidArg, "invalid render mode");
    };
    with_device(dev, |d| d.set_render(mode, full))
}

/// Set the box's persistent name (`name`, NUL-terminated UTF-8); an empty string clears it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_set_name(
    dev: *mut MediusDevice,
    name: *const c_char,
) -> MediusStatus {
    guard_status(|| {
        if dev.is_null() || name.is_null() {
            return fail(MediusStatus::ErrInvalidArg, "null pointer");
        }
        let Ok(s) = (unsafe { CStr::from_ptr(name) }).to_str() else {
            return fail(MediusStatus::ErrInvalidArg, "name is not valid UTF-8");
        };
        let d = unsafe { &(*dev).inner };
        status_of(d.set_name(s))
    })
}

/// Clear the box's custom name, reverting it to its synthesised `Medius-XXXX` default.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_clear_name(dev: *mut MediusDevice) -> MediusStatus {
    with_device(dev, |d| d.clear_name())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_query_version(
    dev: *mut MediusDevice,
    out: *mut MediusVersion,
) -> MediusStatus {
    query(dev, out, |d| d.query_version())
}

/// Both chips' firmware versions and slot state.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_firmware_info(
    dev: *mut MediusDevice,
    out: *mut MediusFirmwareInfo,
) -> MediusStatus {
    query(dev, out, |d| d.firmware_info())
}

/// Write `len` bytes into `target`'s spare slot (0 = device chip, 1 = host chip). The image stays
/// inert until medius_device_activate_firmware. `progress`, if non-null, is called with bytes sent
/// and the total.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_stage_firmware(
    dev: *mut MediusDevice,
    target: u8,
    image: *const u8,
    len: usize,
    progress: Option<unsafe extern "C" fn(*mut c_void, usize, usize)>,
    user: *mut c_void,
) -> MediusStatus {
    let Some(tgt) = UpdateTarget::from_u8(target) else {
        return fail(MediusStatus::ErrInvalidArg, "target must be 0 or 1");
    };
    if image.is_null() || len == 0 {
        return fail(MediusStatus::ErrInvalidArg, "image is empty");
    }
    let bytes = unsafe { std::slice::from_raw_parts(image, len) };
    let user_addr = user as usize;
    with_device(dev, |d| {
        let mut cb = |p: UpdateProgress| {
            if let Some(f) = progress {
                unsafe { f(user_addr as *mut c_void, p.sent, p.total) };
            }
        };
        d.stage_firmware(tgt, bytes, &mut cb).map(|_| ())
    })
}

/// Drop whatever is staged or in flight for one target; the clone comes back without a reboot.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_abort_update(
    dev: *mut MediusDevice,
    target: u8,
) -> MediusStatus {
    let Some(tgt) = UpdateTarget::from_u8(target) else {
        return fail(MediusStatus::ErrInvalidArg, "target must be 0 or 1");
    };
    with_device(dev, |d| d.abort_update(tgt))
}

/// Commit every staged image and reboot into it. Blocks while the host chip reboots and comes back.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_activate_firmware(dev: *mut MediusDevice) -> MediusStatus {
    with_device(dev, |d| d.activate_firmware())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_query_health(
    dev: *mut MediusDevice,
    out: *mut MediusHealth,
) -> MediusStatus {
    query(dev, out, |d| d.query_health())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_device_info(
    dev: *mut MediusDevice,
    out: *mut MediusDeviceInfo,
) -> MediusStatus {
    query(dev, out, |d| d.device_info())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_caps(
    dev: *mut MediusDevice,
    out: *mut MediusCaps,
) -> MediusStatus {
    query(dev, out, |d| d.caps())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_query_rate(
    dev: *mut MediusDevice,
    out: *mut MediusRate,
) -> MediusStatus {
    query(dev, out, |d| d.query_rate())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_query_stats(
    dev: *mut MediusDevice,
    out: *mut MediusStats,
) -> MediusStatus {
    query(dev, out, |d| d.query_stats())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_query_locks(
    dev: *mut MediusDevice,
    out: *mut MediusLocks,
) -> MediusStatus {
    query(dev, out, |d| d.query_locks())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_query_catch(
    dev: *mut MediusDevice,
    out: *mut MediusCatchState,
) -> MediusStatus {
    query(dev, out, |d| d.query_catch())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_query_imperfect(
    dev: *mut MediusDevice,
    out: *mut MediusImperfectStatus,
) -> MediusStatus {
    query(dev, out, |d| d.query_imperfect())
}

/// Query the bearing into `*out`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_query_bearing(
    dev: *mut MediusDevice,
    out: *mut MediusBearing,
) -> MediusStatus {
    query(dev, out, |d| d.query_bearing())
}

/// Query the movement-riding window into `*out_enabled` and `*out_window_ms` (0 when off).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_query_movement_riding(
    dev: *mut MediusDevice,
    out_enabled: *mut bool,
    out_window_ms: *mut u32,
) -> MediusStatus {
    guard_status(|| {
        if dev.is_null() || out_enabled.is_null() || out_window_ms.is_null() {
            return fail(MediusStatus::ErrInvalidArg, "null pointer");
        }
        let d = unsafe { &(*dev).inner };
        match d.query_movement_riding() {
            Ok(window) => {
                let (enabled, ms) = match window {
                    Some(dur) => (true, dur_ms(dur)),
                    None => (false, 0),
                };
                unsafe {
                    *out_enabled = enabled;
                    *out_window_ms = ms;
                }
                clear_error();
                MediusStatus::Ok
            }
            Err(e) => record(&e),
        }
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_query_emit_pace(
    dev: *mut MediusDevice,
    out: *mut MediusEmitPaceStatus,
) -> MediusStatus {
    query(dev, out, |d| d.query_emit_pace())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_query_render(
    dev: *mut MediusDevice,
    out: *mut MediusRenderStatus,
) -> MediusStatus {
    query(dev, out, |d| d.query_render())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_counters(
    dev: *mut MediusDevice,
    out: *mut MediusCountersSnapshot,
) -> MediusStatus {
    guard_status(|| {
        if dev.is_null() || out.is_null() {
            return fail(MediusStatus::ErrInvalidArg, "null pointer");
        }
        let d = unsafe { &(*dev).inner };
        unsafe { *out = d.counters().into() };
        clear_error();
        MediusStatus::Ok
    })
}

fn dur_ms(d: Duration) -> u32 {
    d.as_millis().min(u32::MAX as u128) as u32
}

/// Default RESP wait before a query times out, in milliseconds.
#[unsafe(no_mangle)]
pub extern "C" fn medius_default_query_timeout_ms() -> u32 {
    dur_ms(medius::DEFAULT_QUERY_TIMEOUT)
}

/// Default keepalive cadence for held overrides, in milliseconds.
#[unsafe(no_mangle)]
pub extern "C" fn medius_default_keepalive_cadence_ms() -> u32 {
    dur_ms(medius::DEFAULT_KEEPALIVE_CADENCE)
}

/// The C ABI version, bumped on any breaking change to this header.
#[unsafe(no_mangle)]
pub extern "C" fn medius_abi_version() -> u32 {
    7
}

/// The medius-capi crate version as a static NUL-terminated string.
#[unsafe(no_mangle)]
pub extern "C" fn medius_version_string() -> *const c_char {
    concat!(env!("CARGO_PKG_VERSION"), "\0").as_ptr() as *const c_char
}
