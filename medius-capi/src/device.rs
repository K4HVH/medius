//! The opaque `MediusDevice` handle and every command, query, and lifecycle function.

use std::ffi::CStr;
use std::os::raw::c_char;
use std::time::Duration;

use medius::Device;

use crate::convert::{emit_pace_to_medius, input_to_medius, lock_target_to_medius};
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

#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_move_axis(
    dev: *mut MediusDevice,
    motion: MediusMotion,
) -> MediusStatus {
    with_device(dev, |d| d.move_axis(motion.into()))
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
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_inject(
    dev: *mut MediusDevice,
    input: MediusUsage,
    action: MediusAction,
) -> MediusStatus {
    with_input(dev, input, |d, u| d.inject(u, action.into()))
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
    f: impl FnOnce(&Device, medius::LockTarget) -> Result<(), medius::Error>,
) -> MediusStatus {
    guard_status(|| {
        if dev.is_null() {
            return fail(MediusStatus::ErrInvalidArg, "null device handle");
        }
        let Some(t) = lock_target_to_medius(target) else {
            return fail(MediusStatus::ErrInvalidArg, "invalid lock target");
        };
        let d = unsafe { &(*dev).inner };
        status_of(f(d, t))
    })
}

/// Lock a target (axis or usage) on an edge. A button, key, and media usage all lock the same way.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_lock(
    dev: *mut MediusDevice,
    target: MediusLockTarget,
    dir: MediusLockDirection,
) -> MediusStatus {
    with_lock_target(dev, target, |d, t| d.lock(t, dir.into()))
}

/// Release a lock set by `medius_device_lock`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_unlock(
    dev: *mut MediusDevice,
    target: MediusLockTarget,
    dir: MediusLockDirection,
) -> MediusStatus {
    with_lock_target(dev, target, |d, t| d.unlock(t, dir.into()))
}

/// Lock a whole class blanket (cursor aim, wheel, all buttons, all keys, or all media).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_lock_all(
    dev: *mut MediusDevice,
    what: MediusBlanket,
    dir: MediusLockDirection,
) -> MediusStatus {
    with_device(dev, |d| d.lock_all(what.into(), dir.into()))
}

/// Release a blanket lock set by `medius_device_lock_all`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_unlock_all(
    dev: *mut MediusDevice,
    what: MediusBlanket,
    dir: MediusLockDirection,
) -> MediusStatus {
    with_device(dev, |d| d.unlock_all(what.into(), dir.into()))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_led(
    dev: *mut MediusDevice,
    target: MediusLedTarget,
    mode: MediusLedMode,
    level: u8,
) -> MediusStatus {
    with_device(dev, |d| d.led(target.into(), mode.into(), level))
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

#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_reboot(
    dev: *mut MediusDevice,
    target: MediusRebootTarget,
) -> MediusStatus {
    with_device(dev, |d| d.reboot(target.into()))
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

/// Set what paces injected motion; `hz` is the target rate for `Fixed` and ignored otherwise.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_set_emit_pace(
    dev: *mut MediusDevice,
    mode: MediusEmitMode,
    hz: u16,
) -> MediusStatus {
    with_device(dev, |d| d.set_emit_pace(emit_pace_to_medius(mode, hz)))
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

/// Clear the box's custom name, reverting it to its synthesized `Medius-XXXX` default.
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
    3
}

/// The medius-capi crate version as a static NUL-terminated string.
#[unsafe(no_mangle)]
pub extern "C" fn medius_version_string() -> *const c_char {
    concat!(env!("CARGO_PKG_VERSION"), "\0").as_ptr() as *const c_char
}
