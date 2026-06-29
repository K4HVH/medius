//! The mock-box C ABI (feature = `mock`): a scriptable in-process fake for testing bindings.

use std::ffi::CStr;
use std::os::raw::c_char;
use std::time::Duration;

use medius::{Device, MockBox};

use crate::convert::frame_type_to_native;
use crate::ctypes::*;
use crate::device::MediusDevice;
use crate::error::{MediusStatus, clear_error, fail, guard, guard_status, record};

/// A scriptable fake box. Opaque; create with `medius_mock_new`, free with `medius_mock_free`.
/// Cloning (via `medius_device_with_mock`) shares state, so a configured mock drives a real `Device`.
pub struct MediusMockBox {
    pub(crate) inner: MockBox,
}

/// Create a fresh mock that records commands and auto-answers queries with defaults.
#[unsafe(no_mangle)]
pub extern "C" fn medius_mock_new() -> *mut MediusMockBox {
    guard(std::ptr::null_mut(), || {
        Box::into_raw(Box::new(MediusMockBox {
            inner: MockBox::new(),
        }))
    })
}

/// Free a mock handle. Null is a no-op.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_mock_free(mock: *mut MediusMockBox) {
    guard((), || {
        if !mock.is_null() {
            drop(unsafe { Box::from_raw(mock) });
        }
    });
}

/// Borrow the mock and run `f`. Null is a no-op.
fn with_mock(mock: *mut MediusMockBox, f: impl FnOnce(&MockBox)) {
    guard((), || {
        if mock.is_null() {
            return;
        }
        f(unsafe { &(*mock).inner });
    });
}

/// Set the version the mock answers to a VERSION query.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_mock_set_version(mock: *mut MediusMockBox, value: MediusVersion) {
    with_mock(mock, |m| {
        let _ = m.clone().with_version(value.into());
    });
}

/// Set the health flags the mock answers to a HEALTH query.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_mock_set_health(mock: *mut MediusMockBox, value: MediusHealth) {
    with_mock(mock, |m| {
        let _ = m.clone().with_health(value.into());
    });
}

/// Set the mouse identity the mock answers to a MOUSE_INFO query.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_mock_set_mouse_info(
    mock: *mut MediusMockBox,
    value: MediusMouseInfo,
) {
    with_mock(mock, |m| {
        let _ = m.clone().with_mouse_info(value.into());
    });
}

/// Set the whole capabilities (mouse + keyboard) the mock answers to a CAPS query.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_mock_set_caps(mock: *mut MediusMockBox, value: MediusCaps) {
    with_mock(mock, |m| {
        let _ = m.clone().with_caps(value.into());
    });
}

/// Set only the mouse half of the capabilities.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_mock_set_mouse_caps(
    mock: *mut MediusMockBox,
    value: MediusMouseCaps,
) {
    with_mock(mock, |m| {
        let _ = m.clone().with_mouse_caps(value.into());
    });
}

/// Set only the keyboard half of the capabilities (and mark the keyboard class change-driven).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_mock_set_kbd_caps(mock: *mut MediusMockBox, value: MediusKbdCaps) {
    with_mock(mock, |m| {
        let _ = m.clone().with_kbd_caps(value.into());
    });
}

/// Set the rate the mock answers to a RATE query.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_mock_set_rate(mock: *mut MediusMockBox, value: MediusRate) {
    with_mock(mock, |m| {
        let _ = m.clone().with_rate(value.into());
    });
}

/// Set the stats the mock answers to a STATS query.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_mock_set_stats(mock: *mut MediusMockBox, value: MediusStats) {
    with_mock(mock, |m| {
        let _ = m.clone().with_stats(value.into());
    });
}

/// Set the lock bitmask the mock answers to a LOCKS query.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_mock_set_locks(mock: *mut MediusMockBox, value: MediusLocks) {
    with_mock(mock, |m| {
        let _ = m.clone().with_locks(value.into());
    });
}

/// Set the catch state the mock answers to a CATCH query.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_mock_set_catch_state(
    mock: *mut MediusMockBox,
    value: MediusCatchState,
) {
    with_mock(mock, |m| {
        let _ = m.clone().with_catch_state(value.into());
    });
}

/// Set the imperfect-clone status the mock answers to an OPTION(IMPERFECT) query.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_mock_set_imperfect_status(
    mock: *mut MediusMockBox,
    value: MediusImperfectStatus,
) {
    with_mock(mock, |m| {
        let _ = m.clone().with_imperfect_status(value.into());
    });
}

/// Set the movement-riding window the mock answers to a query; `enabled == false` means off.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_mock_set_movement_riding(
    mock: *mut MediusMockBox,
    enabled: bool,
    window_ms: u32,
) {
    with_mock(mock, |m| {
        let window = enabled.then(|| Duration::from_millis(window_ms as u64));
        let _ = m.clone().with_movement_riding(window);
    });
}

/// Make the mock unresponsive to queries (it still records commands). One-way, for testing timeouts.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_mock_silent(mock: *mut MediusMockBox) {
    with_mock(mock, |m| {
        let _ = m.clone().silent();
    });
}

/// Inject raw bytes into the host's inbound stream, as if the box put them on the wire.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_mock_push_raw(
    mock: *mut MediusMockBox,
    bytes: *const u8,
    len: usize,
) {
    with_mock(mock, |m| {
        if bytes.is_null() {
            return;
        }
        let slice = unsafe { std::slice::from_raw_parts(bytes, len) };
        m.push_raw(slice);
    });
}

/// Push a LOG line as if the box emitted it (surfaces on the device's log stream).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_mock_push_log(
    mock: *mut MediusMockBox,
    level: MediusLogLevel,
    text: *const c_char,
) {
    with_mock(mock, |m| {
        if text.is_null() {
            return;
        }
        let text = unsafe { CStr::from_ptr(text) }.to_string_lossy();
        m.push_log(level.into(), &text);
    });
}

/// Push a MOUSE_EVENT as if the box emitted it (surfaces as a `Mouse` catch event).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_mock_push_event(
    mock: *mut MediusMockBox,
    seq: u8,
    report: MediusMouseEvent,
) {
    with_mock(mock, |m| m.push_event(seq, report.into()));
}

/// Push a KB_EVENT as if the box emitted it (surfaces as a `Keyboard` catch event).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_mock_push_kb_event(
    mock: *mut MediusMockBox,
    seq: u8,
    event: *const MediusKeyboardEvent,
) {
    with_mock(mock, |m| {
        if event.is_null() {
            return;
        }
        m.push_kb_event(seq, &(unsafe { &*event }).into());
    });
}

/// Push a CONS_EVENT as if the box emitted it (surfaces as a `Media` catch event).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_mock_push_cons_event(
    mock: *mut MediusMockBox,
    seq: u8,
    event: *const MediusMediaEvent,
) {
    with_mock(mock, |m| {
        if event.is_null() {
            return;
        }
        m.push_cons_event(seq, &(unsafe { &*event }).into());
    });
}

/// The number of commands the host has sent to the mock so far.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_mock_recorded(mock: *mut MediusMockBox) -> usize {
    guard(0, || {
        if mock.is_null() {
            return 0;
        }
        unsafe { (*mock).inner.recorded() }
    })
}

/// Whether the host has sent at least one frame of the given type.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_mock_saw(mock: *mut MediusMockBox, ty: MediusFrameType) -> bool {
    guard(false, || {
        if mock.is_null() {
            return false;
        }
        match frame_type_to_native(ty) {
            Some(ft) => unsafe { (*mock).inner.saw(ft) },
            None => false,
        }
    })
}

/// Clear the recorded-command log.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_mock_clear_recorded(mock: *mut MediusMockBox) {
    with_mock(mock, |m| m.clear_recorded());
}

/// Read recorded frame `idx`: its type to `*out_ty`, its SEQ to `*out_seq`, and up to `cap` payload
/// bytes to `payload_buf`. Returns the full payload length (may exceed `cap`), or 0 if `idx` is out
/// of range. Out-pointers may be null to skip them.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_mock_recorded_frame(
    mock: *mut MediusMockBox,
    idx: usize,
    out_ty: *mut MediusFrameType,
    out_seq: *mut u8,
    payload_buf: *mut u8,
    cap: usize,
) -> usize {
    guard(0, || {
        if mock.is_null() {
            return 0;
        }
        let frames = unsafe { (*mock).inner.recorded_frames() };
        let Some(frame) = frames.get(idx) else {
            return 0;
        };
        if !out_ty.is_null() {
            unsafe { *out_ty = frame.ty.into() };
        }
        if !out_seq.is_null() {
            unsafe { *out_seq = frame.seq };
        }
        let full = frame.payload.len();
        if !payload_buf.is_null() && cap > 0 {
            let n = full.min(cap);
            unsafe { std::ptr::copy_nonoverlapping(frame.payload.as_ptr(), payload_buf, n) };
        }
        full
    })
}

/// Build a `Device` over the mock WITHOUT a handshake (clones the mock's shared state).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_with_mock(
    mock: *const MediusMockBox,
    out: *mut *mut MediusDevice,
) -> MediusStatus {
    guard_status(|| {
        if mock.is_null() || out.is_null() {
            return fail(MediusStatus::ErrInvalidArg, "null pointer");
        }
        let mock = unsafe { (*mock).inner.clone() };
        unsafe { *out = MediusDevice::boxed(Device::with_mock(mock)) };
        clear_error();
        MediusStatus::Ok
    })
}

/// Build a `Device` over the mock AND run the version handshake (can fail if the mock is silent).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_open_mock(
    mock: *const MediusMockBox,
    out: *mut *mut MediusDevice,
) -> MediusStatus {
    guard_status(|| {
        if mock.is_null() || out.is_null() {
            return fail(MediusStatus::ErrInvalidArg, "null pointer");
        }
        let mock = unsafe { (*mock).inner.clone() };
        match Device::open_mock(mock) {
            Ok(dev) => {
                unsafe { *out = MediusDevice::boxed(dev) };
                clear_error();
                MediusStatus::Ok
            }
            Err(e) => record(&e),
        }
    })
}
