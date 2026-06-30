//! The opaque `MediusDevice` handle and every command, query, and lifecycle function.

use std::ffi::CStr;
use std::os::raw::c_char;
use std::time::Duration;

use medius::{Device, Key, MediaKey};

use crate::convert::input_to_medius;
use crate::ctypes::*;
use crate::error::{MediusStatus, clear_error, fail, guard, guard_status, record, status_of};

/// An open connection to one medius box. Opaque; create with `medius_device_open`/`_find`/`_with_mock`
/// and release with `medius_device_free`.
pub struct MediusDevice {
    pub(crate) inner: Device,
}

impl MediusDevice {
    pub(crate) fn boxed(inner: Device) -> *mut MediusDevice {
        Box::into_raw(Box::new(MediusDevice { inner }))
    }
}

/// Run `f` with the borrowed device, mapping its `Result` to a status. Null handle -> `ErrInvalidArg`.
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

/// Run a query and write its converted result to `out`. Null handle/out -> `ErrInvalidArg`.
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

// --- lifecycle ---

/// Open the box at serial `path` (a NUL-terminated UTF-8 string), handshake, and write the handle to
/// `*out`. The caller owns the handle and must free it with `medius_device_free`.
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

/// Clone a device handle: another owner of the same underlying connection (the link is shared and
/// reference-counted, like `Device::clone` in Rust). Each clone must be freed. Null in -> null out.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_clone(dev: *const MediusDevice) -> *mut MediusDevice {
    guard(std::ptr::null_mut(), || {
        if dev.is_null() {
            return std::ptr::null_mut();
        }
        MediusDevice::boxed(unsafe { (*dev).inner.clone() })
    })
}

/// Free a device handle (joins the background reader/keepalive threads when the last clone drops).
/// Null is a no-op.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_free(dev: *mut MediusDevice) {
    guard((), || {
        if !dev.is_null() {
            drop(unsafe { Box::from_raw(dev) });
        }
    });
}

/// Enumerate medius serial ports into `out` (up to `cap`). Writes the total found to `*out_total`
/// (may exceed `cap`) and returns the number written. Ports with an unrepresentable path are omitted.
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

// --- movement ---

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

// --- injection: buttons ---

#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_inject(
    dev: *mut MediusDevice,
    input: MediusInput,
    action: MediusAction,
) -> MediusStatus {
    guard_status(|| {
        if dev.is_null() {
            return fail(MediusStatus::ErrInvalidArg, "null device handle");
        }
        let Some(inp) = input_to_medius(input) else {
            return fail(MediusStatus::ErrInvalidArg, "invalid input value");
        };
        let d = unsafe { &(*dev).inner };
        status_of(d.inject(inp, action.into()))
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_button(
    dev: *mut MediusDevice,
    button: MediusButton,
    action: MediusAction,
) -> MediusStatus {
    with_device(dev, |d| d.button(button.into(), action.into()))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_press(
    dev: *mut MediusDevice,
    button: MediusButton,
) -> MediusStatus {
    with_device(dev, |d| d.press(button.into()))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_soft_release(
    dev: *mut MediusDevice,
    button: MediusButton,
) -> MediusStatus {
    with_device(dev, |d| d.soft_release(button.into()))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_force_release(
    dev: *mut MediusDevice,
    button: MediusButton,
) -> MediusStatus {
    with_device(dev, |d| d.force_release(button.into()))
}

// --- injection: keyboard ---

#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_key(
    dev: *mut MediusDevice,
    key: MediusKey,
    action: MediusAction,
) -> MediusStatus {
    with_device(dev, |d| d.key(Key::new(key), action.into()))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_key_down(
    dev: *mut MediusDevice,
    key: MediusKey,
) -> MediusStatus {
    with_device(dev, |d| d.key_down(Key::new(key)))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_key_up(
    dev: *mut MediusDevice,
    key: MediusKey,
) -> MediusStatus {
    with_device(dev, |d| d.key_up(Key::new(key)))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_key_force_release(
    dev: *mut MediusDevice,
    key: MediusKey,
) -> MediusStatus {
    with_device(dev, |d| d.key_force_release(Key::new(key)))
}

// --- injection: media ---

#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_media(
    dev: *mut MediusDevice,
    media: MediusMediaKey,
    action: MediusAction,
) -> MediusStatus {
    with_device(dev, |d| d.media(MediaKey::new(media), action.into()))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_media_down(
    dev: *mut MediusDevice,
    media: MediusMediaKey,
) -> MediusStatus {
    with_device(dev, |d| d.media_down(MediaKey::new(media)))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_media_up(
    dev: *mut MediusDevice,
    media: MediusMediaKey,
) -> MediusStatus {
    with_device(dev, |d| d.media_up(MediaKey::new(media)))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_media_force_release(
    dev: *mut MediusDevice,
    media: MediusMediaKey,
) -> MediusStatus {
    with_device(dev, |d| d.media_force_release(MediaKey::new(media)))
}

// --- locks ---

#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_lock(
    dev: *mut MediusDevice,
    target: MediusLockTarget,
    dir: MediusLockDirection,
) -> MediusStatus {
    with_device(dev, |d| d.lock(target.into(), dir.into()))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_unlock(
    dev: *mut MediusDevice,
    target: MediusLockTarget,
    dir: MediusLockDirection,
) -> MediusStatus {
    with_device(dev, |d| d.unlock(target.into(), dir.into()))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_lock_key(
    dev: *mut MediusDevice,
    key: MediusKey,
    dir: MediusLockDirection,
) -> MediusStatus {
    with_device(dev, |d| d.lock_key(Key::new(key), dir.into()))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_unlock_key(
    dev: *mut MediusDevice,
    key: MediusKey,
    dir: MediusLockDirection,
) -> MediusStatus {
    with_device(dev, |d| d.unlock_key(Key::new(key), dir.into()))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_lock_media(
    dev: *mut MediusDevice,
    media: MediusMediaKey,
) -> MediusStatus {
    with_device(dev, |d| d.lock_media(MediaKey::new(media)))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_unlock_media(
    dev: *mut MediusDevice,
    media: MediusMediaKey,
) -> MediusStatus {
    with_device(dev, |d| d.unlock_media(MediaKey::new(media)))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_lock_all(
    dev: *mut MediusDevice,
    what: MediusBlanket,
) -> MediusStatus {
    with_device(dev, |d| d.lock_all(what.into()))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_unlock_all(
    dev: *mut MediusDevice,
    what: MediusBlanket,
) -> MediusStatus {
    with_device(dev, |d| d.unlock_all(what.into()))
}

// --- led ---

#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_led(
    dev: *mut MediusDevice,
    target: MediusLedTarget,
    mode: MediusLedMode,
    level: u8,
) -> MediusStatus {
    with_device(dev, |d| d.led(target.into(), mode.into(), level))
}

// --- admin ---

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

// --- options ---

#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_allow_imperfect_clones(
    dev: *mut MediusDevice,
    allow: bool,
) -> MediusStatus {
    with_device(dev, |d| d.allow_imperfect_clones(allow))
}

/// Set movement riding. When `enabled` is false the window is cleared (off); otherwise the injected
/// motion rides a native cursor report seen within `window_ms` (rounded to whole ms by the firmware).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_set_movement_riding(
    dev: *mut MediusDevice,
    enabled: bool,
    window_ms: u32,
) -> MediusStatus {
    let window = enabled.then(|| Duration::from_millis(window_ms as u64));
    with_device(dev, |d| d.set_movement_riding(window))
}

// --- queries ---

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
pub unsafe extern "C" fn medius_device_query_mouse_info(
    dev: *mut MediusDevice,
    out: *mut MediusMouseInfo,
) -> MediusStatus {
    query(dev, out, |d| d.query_mouse_info())
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

/// Query the movement-riding window. Writes whether it is on to `*out_enabled` and, when on, the
/// window in whole ms to `*out_window_ms` (0 when off).
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

// --- meta ---

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

/// The C ABI version. Bumped on any breaking change to this header.
#[unsafe(no_mangle)]
pub extern "C" fn medius_abi_version() -> u32 {
    1
}

/// The medius-capi crate version as a static NUL-terminated string.
#[unsafe(no_mangle)]
pub extern "C" fn medius_version_string() -> *const c_char {
    concat!(env!("CARGO_PKG_VERSION"), "\0").as_ptr() as *const c_char
}
