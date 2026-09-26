//! The opaque `MediusDevice` handle and every command, query, and lifecycle function.

use std::ffi::CStr;
use std::os::raw::{c_char, c_void};
use std::time::Duration;

use medius::Device;
use medius::{UpdateProgress, UpdateTarget};

use crate::convert::{
    action_from_c, axis_from_c, blanket_from_c, emit_pace_from_c, input_to_medius, led_mode_from_c,
    led_target_from_c, lock_target_to_medius, motion_from_c, move_timing_from_c, opt_slice,
    patch_from_c, pending_motion_from_c, reboot_target_from_c, rewrite_rule_from_c, setup_from_c,
    transform_from_c,
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

/// Clone a handle to the same reference-counted connection; free each clone.
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

/// Enumerate every connected box into `out` (up to `cap`), reading each in turn; writes the total to `*out_total` and returns the number written. A box on another control protocol is listed with `has_device` 0.
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

/// Open the box whose identity matches `id` (device MAC hex or CH343 serial), handshake, and write the handle to `*out`. `MEDIUS_STATUS_ERR_BAD_PROTO_VER` when that box speaks another control protocol.
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

/// Open the first box whose clone is a mouse, handshake, and write the handle to `*out`. `MEDIUS_STATUS_ERR_BAD_PROTO_VER` when no other box clones a mouse and a box on another control protocol (clone unread) is connected.
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

/// Open the first box whose clone is a keyboard, handshake, and write the handle to `*out`. `MEDIUS_STATUS_ERR_BAD_PROTO_VER` when no other box clones a keyboard and a box on another control protocol (clone unread) is connected.
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

/// A cursor move that bypasses movement riding, sent on the box's next mouse report.
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

/// An AC Pan (horizontal-scroll) move; full `i16`, no clamp.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_pan(dev: *mut MediusDevice, delta: i16) -> MediusStatus {
    with_device(dev, |d| d.pan(delta))
}

/// An AC Pan move that bypasses movement riding.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_pan_now(dev: *mut MediusDevice, delta: i16) -> MediusStatus {
    with_device(dev, |d| d.pan_now(delta))
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

/// Drive one momentary usage (button, key, or media) with an explicit action.
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

/// Weigh physical input on a target and direction. `scale` is the percent of the physical value
/// kept: `MEDIUS_LOCK_SCALE_BLOCK` blocks, `MEDIUS_LOCK_SCALE_PASS` passes, and above that
/// amplifies up to `MEDIUS_LOCK_SCALE_MAX` (2.55x). Lock and unlock are its two ends.
///
/// A negative percent, down to `MEDIUS_LOCK_SCALE_MIN`, weighs and reverses: `-100` inverts. The
/// slot is picked from the delta's sign before the weigh, so `-100` on `MEDIUS_DIRECTION_POSITIVE`
/// turns rightward input leftward and leaves leftward input alone. Axes only: on a momentary usage,
/// which carries one bit, it is `MEDIUS_STATUS_ERR_LOCK_SCALE_USAGE`. A magnitude out of range is
/// `MEDIUS_STATUS_ERR_LOCK_SCALE_RANGE`.
///
/// A delta picks up at most two scales, its absolute direction's and its relative direction's, and
/// they multiply. `MEDIUS_DIRECTION_BOTH` writes the scale to the two fixed signs and a full pass
/// to the relative pair, so a `Both` of 50 is 50% with or without a bearing. Name a relative
/// direction to weigh it.
///
/// `MEDIUS_DIRECTION_WITH` / `_AGAINST` need a live bearing (`medius_device_set_bearing`), which
/// only an axis has; on a button, key or media usage either is
/// `MEDIUS_STATUS_ERR_RELATIVE_DIRECTION`. Any scale below a full pass locks a momentary usage; at
/// or above one unlocks it. A media usage has no edges: it is sent, and `RESP(LOCKS)` reports it,
/// as `MEDIUS_DIRECTION_BOTH` whatever edge is named.
///
/// `dir` takes a `MEDIUS_DIRECTION_*` constant; any other value is `MEDIUS_STATUS_ERR_INVALID_ARG`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_scale(
    dev: *mut MediusDevice,
    target: MediusLockTarget,
    dir: u8,
    scale: i16,
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
    scale: i16,
) -> MediusStatus {
    with_blanket(dev, what, dir, |d, what, dir| d.scale_all(what, dir, scale))
}

/// Lock a target (axis or usage) on an edge. Buttons, keys and media usages lock alike.
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

/// `RESET` with the NVS flag: the `medius_device_reset` release, then the box erases its persistent
/// store and reboots to its defaults under its MAC-derived name. Erases the box name, every option,
/// and everything learned about devices seen, descriptor patch sets included. While it reboots the
/// control port stays enumerated but silent, so queries time out; the link does not drop.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_factory_reset(dev: *mut MediusDevice) -> MediusStatus {
    with_device(dev, |d| d.factory_reset())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_reapply(dev: *mut MediusDevice) -> MediusStatus {
    with_device(dev, |d| d.reapply())
}

/// Rescan by VID/PID, reopen this box, and re-apply held state. `MEDIUS_STATUS_ERR_BAD_PROTO_VER` when
/// the box replies on another control protocol; it stays disconnected.
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

/// `OPTION(IMPERFECT)`: opt into cloning devices the box cannot clone faithfully, or back to
/// faithful-only. A toggle that changes the served patch set re-presents the clone, releasing the
/// session like a device replug; the library re-sends its held state once the new clone is up, and
/// `medius_clip_lost` reports a dropped clip.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_allow_imperfect_clones(
    dev: *mut MediusDevice,
    allow: bool,
) -> MediusStatus {
    with_device(dev, |d| d.allow_imperfect_clones(allow))
}

/// `RAW` (§3.14): put `bytes[0..len]` verbatim on the clone's bare endpoint number `ep_num` (0 to
/// 15) in `dir`, fire-and-forget. `dir` is `MEDIUS_DIRECTION_POSITIVE` (IN, to the game PC) or
/// `MEDIUS_DIRECTION_NEGATIVE` (OUT, to the real device); any other is
/// `MEDIUS_STATUS_ERR_RAW_DIRECTION` (`MEDIUS_STATUS_ERR_RELATIVE_DIRECTION` for the
/// bearing-relative pair). Gated on `medius_device_allow_imperfect_clones`: with the opt-in off the
/// box drops the frame with no reply, and this still returns `MEDIUS_STATUS_OK`;
/// `medius_device_query_imperfect` reports the state.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_raw(
    dev: *mut MediusDevice,
    ep_num: u8,
    dir: u8,
    bytes: *const u8,
    len: usize,
) -> MediusStatus {
    guard_status(|| {
        if dev.is_null() {
            return fail(MediusStatus::ErrInvalidArg, "null device handle");
        }
        let Some(dir) = medius::Direction::from_u8(dir) else {
            return fail(MediusStatus::ErrInvalidArg, "invalid direction");
        };
        let Some(slice) = (unsafe { opt_slice(bytes, len) }) else {
            return fail(MediusStatus::ErrInvalidArg, "null bytes with len > 0");
        };
        status_of(unsafe { &(*dev).inner }.raw(ep_num, dir, slice))
    })
}

/// `TRANSFER` (§3.14): run one control transfer against the real device, writing the reply to
/// `*out`. `ep` is 0 for EP0 or a control endpoint the device declares; `out_data[0..out_len]` is
/// the OUT data stage (empty for IN). A non-OK `out->status` is a protocol outcome, not a failure;
/// the box replies `REFUSED` while the opt-in is off. `MEDIUS_STATUS_OK` means only that the box
/// replied.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_transfer(
    dev: *mut MediusDevice,
    ep: u8,
    setup: MediusSetup,
    out_data: *const u8,
    out_len: usize,
    out: *mut MediusTransferOutcome,
) -> MediusStatus {
    guard_status(|| {
        if dev.is_null() || out.is_null() {
            return fail(MediusStatus::ErrInvalidArg, "null pointer");
        }
        let Some(data) = (unsafe { opt_slice(out_data, out_len) }) else {
            return fail(
                MediusStatus::ErrInvalidArg,
                "null out_data with out_len > 0",
            );
        };
        match unsafe { &(*dev).inner }.transfer(ep, setup_from_c(setup), data) {
            Ok(outcome) => {
                unsafe { *out = outcome.into() };
                clear_error();
                MediusStatus::Ok
            }
            Err(e) => record(&e),
        }
    })
}

/// [`medius_device_transfer`] with a reply timeout in milliseconds. The box abandons a control
/// transfer after its own ~800 ms, so keep `timeout_ms` at or above
/// `medius_default_transfer_timeout_ms()`; a shorter one stops waiting before a slow device
/// replies.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_transfer_timeout(
    dev: *mut MediusDevice,
    ep: u8,
    setup: MediusSetup,
    out_data: *const u8,
    out_len: usize,
    timeout_ms: u32,
    out: *mut MediusTransferOutcome,
) -> MediusStatus {
    guard_status(|| {
        if dev.is_null() || out.is_null() {
            return fail(MediusStatus::ErrInvalidArg, "null pointer");
        }
        let Some(data) = (unsafe { opt_slice(out_data, out_len) }) else {
            return fail(
                MediusStatus::ErrInvalidArg,
                "null out_data with out_len > 0",
            );
        };
        match unsafe { &(*dev).inner }.transfer_timeout(
            ep,
            setup_from_c(setup),
            data,
            Duration::from_millis(timeout_ms as u64),
        ) {
            Ok(outcome) => {
                unsafe { *out = outcome.into() };
                clear_error();
                MediusStatus::Ok
            }
            Err(e) => record(&e),
        }
    })
}

fn with_rewrite_rule(
    dev: *mut MediusDevice,
    rule: *const MediusRewriteRule,
    f: impl FnOnce(&Device, medius::RewriteRule) -> Result<(), medius::Error>,
) -> MediusStatus {
    guard_status(|| {
        if dev.is_null() || rule.is_null() {
            return fail(MediusStatus::ErrInvalidArg, "null pointer");
        }
        let Some(r) = rewrite_rule_from_c(unsafe { &*rule }) else {
            return fail(
                MediusStatus::ErrInvalidArg,
                "invalid rewrite class, action or direction",
            );
        };
        // The arrays hold `MEDIUS_MAX_REWRITE_MATCH` bytes, so a longer key is refused here for set
        // and remove alike. An unequal pair goes to the crate, which checks mask length first.
        let len = r.match_bytes.len();
        if len == r.mask.len() && len > MEDIUS_MAX_REWRITE_MATCH {
            return record(&medius::Error::RewriteMatchTooLong {
                len,
                limit: MEDIUS_MAX_REWRITE_MATCH,
            });
        }
        status_of(f(unsafe { &(*dev).inner }, r))
    })
}

/// `REWRITE` (§3.14): add or overwrite one rewrite rule; gated on the imperfect-clone opt-in.
/// `rule->class_` takes a `MEDIUS_REWRITE_CLASS_*` constant, `rule->action` a
/// `MEDIUS_REWRITE_ACTION_*` one and `rule->direction` a `MEDIUS_DIRECTION_*` one; any other value is
/// `MEDIUS_STATUS_ERR_INVALID_ARG`. `match_len` must equal `mask_len`
/// (`MEDIUS_STATUS_ERR_REWRITE_MASK_LENGTH`) and be at most `MEDIUS_MAX_REWRITE_MATCH`
/// (`..._REWRITE_MATCH_TOO_LONG`), the action must suit the class (`..._REWRITE_ACTION_CLASS`), the
/// direction must not be bearing-relative (`..._RELATIVE_DIRECTION`), and the payload must fit the
/// box's head (`..._REWRITE_PAYLOAD_TOO_LARGE`). A new rule past `MEDIUS_MAX_REWRITE_ENTRIES` is
/// `..._REWRITE_TABLE_FULL`; a payload past what the held rules leave of
/// `MEDIUS_REWRITE_PAYLOAD_POOL` is `..._REWRITE_POOL_FULL`. `medius_device_query_rewrite` confirms
/// what the box holds.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_set_rewrite(
    dev: *mut MediusDevice,
    rule: *const MediusRewriteRule,
) -> MediusStatus {
    with_rewrite_rule(dev, rule, |d, r| d.set_rewrite(&r))
}

/// `REWRITE` remove (§3.14): drop the rule keyed by `rule`'s `(class, id, direction, match, mask)`,
/// ignoring its action and payload; a no-op on the box if no such rule is held.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_remove_rewrite(
    dev: *mut MediusDevice,
    rule: *const MediusRewriteRule,
) -> MediusStatus {
    with_rewrite_rule(dev, rule, |d, r| d.remove_rewrite(&r))
}

/// `REWRITE` clear (§3.14): drop the whole rewrite table. Clears the crate's held rules whatever
/// the opt-in.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_clear_rewrite(dev: *mut MediusDevice) -> MediusStatus {
    with_device(dev, |d| d.clear_rewrite())
}

/// `QUERY(REWRITE)` → `*out` (§4.17): the table summary, a row per rule without its
/// match/mask/payload bytes (read those with `medius_device_query_rewrite_entry`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_query_rewrite(
    dev: *mut MediusDevice,
    out: *mut MediusRewriteTable,
) -> MediusStatus {
    query(dev, out, |d| d.query_rewrite())
}

/// `QUERY(REWRITE_ENTRY, index)` → `*out` (§4.17): one rule in full, in the shape
/// `medius_device_set_rewrite` takes, so a read rule replays as a set. `index` is its
/// `medius_device_query_rewrite` row.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_query_rewrite_entry(
    dev: *mut MediusDevice,
    index: u8,
    out: *mut MediusRewriteRule,
) -> MediusStatus {
    query(dev, out, |d| d.query_rewrite_entry(index))
}

fn with_patch(
    dev: *mut MediusDevice,
    patch: *const MediusPatch,
    f: impl FnOnce(&Device, medius::Patch) -> Result<(), medius::Error>,
) -> MediusStatus {
    guard_status(|| {
        if dev.is_null() || patch.is_null() {
            return fail(MediusStatus::ErrInvalidArg, "null pointer");
        }
        let Some(p) = patch_from_c(unsafe { &*patch }) else {
            return fail(MediusStatus::ErrInvalidArg, "invalid patch section");
        };
        status_of(f(unsafe { &(*dev).inner }, p))
    })
}

/// `PATCH` (§3.14): store one descriptor patch, keyed by `(section, cfg, index, offset)`; `len` 0
/// removes the patch at that key. The box stores it whatever the opt-in; the set reaches the game
/// PC when the clone is next presented under the opt-in: `medius_device_apply_patch`, the opt-in
/// turning on, or the device attaching. `patch->section` takes a `MEDIUS_PATCH_SECTION_*` constant;
/// any other value is `MEDIUS_STATUS_ERR_INVALID_ARG`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_set_patch(
    dev: *mut MediusDevice,
    patch: *const MediusPatch,
) -> MediusStatus {
    with_patch(dev, patch, |d, p| d.set_patch(&p))
}

/// `PATCH` APPLY (§3.14): re-present the clone with the stored patch set (one replug to the game
/// PC). With the imperfect-clone opt-in off this is `MEDIUS_STATUS_ERR_IMPERFECT_REQUIRED`. The box
/// re-presents only while the stored set differs from the served one: applying an emptied set
/// serves the device unpatched, and a refused set unchanged since is left alone. Re-presenting
/// releases the session like a device replug; the library re-sends its held state once the new
/// clone is up, and `medius_clip_lost` reports a dropped clip.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_apply_patch(dev: *mut MediusDevice) -> MediusStatus {
    with_device(dev, |d| d.apply_patch())
}

/// `PATCH` CLEAR (§3.14): erase this device's stored set (the last attached one's when unplugged).
/// A clone serving patches re-presents unpatched (one replug to the game PC), releasing the session
/// and re-sending held state as `medius_device_apply_patch` does.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_clear_patch(dev: *mut MediusDevice) -> MediusStatus {
    with_device(dev, |d| d.clear_patch())
}

/// `QUERY(PATCHES)` → `*out` (§4.17): the stored patch set and its apply state, a row per patch
/// without its bytes (read those with `medius_device_query_patch_entry`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_query_patches(
    dev: *mut MediusDevice,
    out: *mut MediusPatchSet,
) -> MediusStatus {
    query(dev, out, |d| d.query_patches())
}

/// `QUERY(PATCH_ENTRY, index)` → `*out` (§4.17): one patch in full, in the shape
/// `medius_device_set_patch` takes, so a read patch replays as a set. `index` is its
/// `medius_device_query_patches` row.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_query_patch_entry(
    dev: *mut MediusDevice,
    index: u8,
    out: *mut MediusPatch,
) -> MediusStatus {
    query(dev, out, |d| d.query_patch_entry(index))
}

// Field transforms (§3.15): faithful field operations on the semantic path, not gated on the
// imperfect-clone opt-in.

fn with_transform(
    dev: *mut MediusDevice,
    transform: *const MediusTransform,
    f: impl FnOnce(&Device, medius::Transform) -> Result<(), medius::Error>,
) -> MediusStatus {
    guard_status(|| {
        if dev.is_null() || transform.is_null() {
            return fail(MediusStatus::ErrInvalidArg, "null pointer");
        }
        let Some(t) = transform_from_c(unsafe { &*transform }) else {
            return fail(
                MediusStatus::ErrInvalidArg,
                "invalid transform op, source or dest",
            );
        };
        status_of(f(unsafe { &(*dev).inner }, t))
    })
}

/// `TRANSFORM` (§3.15): add or overwrite one field transform, fire-and-forget. It swaps or remaps a
/// field the clone already declares, so it is faithful and needs no imperfect-clone opt-in. To
/// weigh or reverse a field, use `medius_device_scale`, which runs first and passes the transform
/// what it kept. Entries are keyed by `(source, dest)` and apply in installation order.
/// `transform->op` takes a `MEDIUS_TRANSFORM_OP_*` constant, and `source`/`dest` a
/// `MEDIUS_LOCK_TARGET_KIND_*` axis or usage. A combination the op cannot address, including one
/// field as both ends, is `MEDIUS_STATUS_ERR_TRANSFORM_OP_FIELDS`; an entry past
/// `MEDIUS_MAX_TRANSFORM_ENTRIES` is `..._TRANSFORM_TABLE_FULL`. `medius_device_query_transforms`
/// confirms what the box holds.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_transform(
    dev: *mut MediusDevice,
    transform: *const MediusTransform,
) -> MediusStatus {
    with_transform(dev, transform, |d, t| d.transform(&t))
}

/// `TRANSFORM` remove (§3.15): drop the transform keyed by `transform`'s `(source, dest)`, ignoring
/// its op; a no-op on the box if no such entry is held.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_untransform(
    dev: *mut MediusDevice,
    transform: *const MediusTransform,
) -> MediusStatus {
    with_transform(dev, transform, |d, t| d.untransform(&t))
}

/// `TRANSFORM` clear (§3.15): drop the whole transform table.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_clear_transforms(dev: *mut MediusDevice) -> MediusStatus {
    with_device(dev, |d| d.clear_transforms())
}

/// Exchange two axes on the wire: a `medius_device_transform` swap. `a` and `b` take
/// `MEDIUS_AXIS_*` constants; any other value is `MEDIUS_STATUS_ERR_INVALID_ARG`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_transform_swap(
    dev: *mut MediusDevice,
    a: u8,
    b: u8,
) -> MediusStatus {
    let (Some(a), Some(b)) = (axis_from_c(a), axis_from_c(b)) else {
        return fail(MediusStatus::ErrInvalidArg, "invalid axis");
    };
    with_device(dev, |d| d.transform_swap(a, b))
}

/// Remap a source field into a destination: a `medius_device_transform` remap. `source` and `dest`
/// are a `MEDIUS_LOCK_TARGET_KIND_*` axis or usage; an unnamed `kind` or usage is
/// `MEDIUS_STATUS_ERR_INVALID_ARG`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_transform_remap(
    dev: *mut MediusDevice,
    source: MediusLockTarget,
    dest: MediusLockTarget,
) -> MediusStatus {
    let (Some(source), Some(dest)) = (lock_target_to_medius(source), lock_target_to_medius(dest))
    else {
        return fail(
            MediusStatus::ErrInvalidArg,
            "invalid transform source or dest",
        );
    };
    with_device(dev, |d| d.transform_remap(source, dest))
}

/// `QUERY(TRANSFORMS)` → `*out` (§4.18): the transform table, a row per entry in the shape
/// `medius_device_transform` takes, so a read entry replays as a set.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_query_transforms(
    dev: *mut MediusDevice,
    out: *mut MediusTransforms,
) -> MediusStatus {
    query(dev, out, |d| d.query_transforms())
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

/// Set injected-motion pacing and the clone's rate: `hz` is the `Fixed` target rate, ignored
/// otherwise; `force_hz` is the forced wire rate (0 = native interval). `mode` takes a
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

/// Clear the box's custom name, reverting to the synthesised `Medius-XXXX` default.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_clear_name(dev: *mut MediusDevice) -> MediusStatus {
    with_device(dev, |d| d.clear_name())
}

/// Set the bearing: what `MEDIUS_DIRECTION_WITH` / `_AGAINST` are measured against. `window_ms` is how
/// long the last injected delta's direction stays the bearing; 0 turns it off, leaving the relative
/// directions inert whatever their scale. The box boots at `MEDIUS_BEARING_WINDOW_DEFAULT_MS`.
///
/// `mode` takes a `MEDIUS_BEARING_MODE_*` constant; any other value is
/// `MEDIUS_STATUS_ERR_INVALID_ARG`. Both fields share one frame and persist together, so a window
/// change carries the mode.
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

/// Set the render texture, and whether the model renders native motion instead of relaying it.
/// `mode` takes a `MEDIUS_RENDER_MODE_*` constant; any other value is
/// `MEDIUS_STATUS_ERR_INVALID_ARG`. Rendering adds a small latency, which reaches native motion
/// when `full` is on. `full` is off by default.
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

/// Set the percent of the host's command interval an injected delta is released across: 0 puts it
/// all on the box's next report, 100 spreads it across one command interval, above 100 overlaps. A
/// loop at the native report rate keeps each command whole on its own report. Until the box learns
/// the host's command period from `MOVE` arrivals, each delta goes out whole.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_set_spread(
    dev: *mut MediusDevice,
    percent: u16,
) -> MediusStatus {
    with_device(dev, |d| d.set_spread(percent))
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
/// inert until `medius_device_activate_firmware`. `progress`, if non-null, gets bytes sent and the
/// total.
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

/// Commit every staged image and reboot into it; blocks until the host chip is back.
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

/// Query the bearing into `*out`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_query_bearing(
    dev: *mut MediusDevice,
    out: *mut MediusBearing,
) -> MediusStatus {
    query(dev, out, |d| d.query_bearing())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_query_render(
    dev: *mut MediusDevice,
    out: *mut MediusRenderStatus,
) -> MediusStatus {
    query(dev, out, |d| d.query_render())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_query_spread(
    dev: *mut MediusDevice,
    out: *mut MediusSpreadStatus,
) -> MediusStatus {
    query(dev, out, |d| d.query_spread())
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

/// Default query reply timeout, in milliseconds.
#[unsafe(no_mangle)]
pub extern "C" fn medius_default_query_timeout_ms() -> u32 {
    dur_ms(medius::DEFAULT_QUERY_TIMEOUT)
}

/// Default reply wait for `medius_device_transfer`, in milliseconds.
#[unsafe(no_mangle)]
pub extern "C" fn medius_default_transfer_timeout_ms() -> u32 {
    dur_ms(medius::DEFAULT_TRANSFER_TIMEOUT)
}

/// Default keepalive cadence for held overrides, in milliseconds.
#[unsafe(no_mangle)]
pub extern "C" fn medius_default_keepalive_cadence_ms() -> u32 {
    dur_ms(medius::DEFAULT_KEEPALIVE_CADENCE)
}

/// The C ABI version this header declares, bumped on any breaking change. Compare with
/// `medius_abi_version()` once at start-up.
pub const MEDIUS_ABI_VERSION: u32 = 9;

/// The loaded library's C ABI version, bumped on any breaking header change. Compare with
/// `MEDIUS_ABI_VERSION` once at start-up. On a mismatch call nothing else: the header's struct
/// layouts differ from the library's. Rebuild against the header shipped with that library.
#[unsafe(no_mangle)]
pub extern "C" fn medius_abi_version() -> u32 {
    MEDIUS_ABI_VERSION
}

/// The medius-capi crate version as a static NUL-terminated string.
#[unsafe(no_mangle)]
pub extern "C" fn medius_version_string() -> *const c_char {
    concat!(env!("CARGO_PKG_VERSION"), "\0").as_ptr() as *const c_char
}
