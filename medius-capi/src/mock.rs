//! The mock-box C ABI (feature = `mock`): a scriptable in-process fake for testing bindings.

use std::ffi::CStr;
use std::os::raw::c_char;
use std::time::Duration;

use medius::{Device, MockBox};

use crate::convert::{
    clip_action_to_c, clip_status_from_c, clock_domain_from_c, device_info_from_c,
    emit_pace_from_c, frame_type_from_c, opt_slice, traffic_class_from_c,
};
use crate::ctypes::*;
use crate::device::MediusDevice;
use crate::error::{MediusStatus, clear_error, fail, guard, guard_status, record};

/// A scriptable fake box. Opaque; create with `medius_mock_new`, free with `medius_mock_free`.
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

/// Clone a mock handle: another handle sharing the same recorded state (like `MockBox::clone`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_mock_clone(mock: *const MediusMockBox) -> *mut MediusMockBox {
    guard(std::ptr::null_mut(), || {
        if mock.is_null() {
            return std::ptr::null_mut();
        }
        Box::into_raw(Box::new(MediusMockBox {
            inner: unsafe { (*mock).inner.clone() },
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

/// Set the device identity the mock answers to a DEVICE_INFO query. `value.kind` takes a
/// `MEDIUS_DEVICE_KIND_*` constant; any other value is ignored, as these setters have no status.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_mock_set_device_info(
    mock: *mut MediusMockBox,
    value: MediusDeviceInfo,
) {
    with_mock(mock, |m| {
        if let Some(info) = device_info_from_c(value) {
            let _ = m.clone().with_device_info(info);
        }
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

/// Set the imperfect-clone status the mock answers to an OPTION(IMPERFECT) query. With the opt-in
/// off the mock drops its consuming clip packet triggers, as the box does when OPTION(IMPERFECT) goes
/// off.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_mock_set_imperfect_status(
    mock: *mut MediusMockBox,
    value: MediusImperfectStatus,
) {
    with_mock(mock, |m| {
        let _ = m.clone().with_imperfect_status(value.into());
    });
}

/// Set the canned `(status, IN data)` the mock answers a `TRANSFER` with while the opt-in is on; with
/// it off the mock answers `REFUSED` regardless. A non-OK status carries no data, as the box does.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_mock_set_transfer_reply(
    mock: *mut MediusMockBox,
    status: u8,
    data: *const u8,
    len: usize,
) {
    with_mock(mock, |m| {
        let slice = if data.is_null() || len == 0 {
            &[][..]
        } else {
            unsafe { std::slice::from_raw_parts(data, len) }
        };
        m.set_transfer_reply(status, slice);
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

/// Set the bearing the mock answers to an OPTION(BEARING) query; `window_ms` 0 = off. `mode` takes a
/// `MEDIUS_BEARING_MODE_*` constant; any other value is ignored, as the box ignores it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_mock_set_bearing(
    mock: *mut MediusMockBox,
    window_ms: u16,
    mode: u8,
) {
    with_mock(mock, |m| {
        let Some(mode) = medius::BearingMode::from_u8(mode) else {
            return;
        };
        m.set_bearing(medius::Bearing {
            window: (window_ms != 0).then(|| Duration::from_millis(window_ms as u64)),
            mode,
        });
    });
}

/// Set the emit-rate pacing mode and the forced wire rate the mock answers to an OPTION(EMIT) query;
/// `hz` matters only for `Fixed`, `force_hz` 0 means unforced. `mode` takes a `MEDIUS_EMIT_MODE_*`
/// constant; any other value leaves the pacing mode alone.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_mock_set_emit_pace(
    mock: *mut MediusMockBox,
    mode: u8,
    hz: u16,
    force_hz: u16,
) {
    with_mock(mock, |m| {
        if let Some(pace) = emit_pace_from_c(mode, hz) {
            m.set_emit_pace(pace);
        }
        m.set_rate_force((force_hz != 0).then_some(force_hz));
    });
}

/// Set what the mock answers to an OPTION(RENDER) query: the texture, whether native motion
/// goes through it, and whether a profile has armed. `mode` takes a `MEDIUS_RENDER_MODE_*` constant;
/// any other value leaves the texture alone. `ready` is what gates rendering on a real box, so a mock
/// left unarmed is the state every box passes through after a power cut.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_mock_set_render(
    mock: *mut MediusMockBox,
    mode: u8,
    full: bool,
    ready: bool,
) {
    with_mock(mock, |m| {
        if let Some(mode) = medius::RenderMode::from_u8(mode) {
            m.set_render(mode, full);
        }
        m.set_render_ready(ready);
    });
}

/// Set the command period the mock has learned, in microseconds. A real box learns it off `MOVE`
/// arrivals and releases nothing across an interval until it has, so a mock left at 0 answers a span
/// of 0 whatever percent is set, which is the state every box starts in.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_mock_set_spread_learned(mock: *mut MediusMockBox, period_us: u32) {
    with_mock(mock, |m| m.set_spread_learned(period_us));
}

/// Set the rate the mock's clone advertises unforced, in Hz; 0 means no clone.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_mock_set_advertised_hz(mock: *mut MediusMockBox, hz: u16) {
    with_mock(mock, |m| {
        let _ = m.clone().with_advertised_hz(hz);
    });
}

/// Set the [`ClipStatus`](medius::ClipStatus) the mock answers to `medius_clip_query_status`.
/// `value.state` takes a `MEDIUS_CLIP_STATE_*` constant; any other value is ignored.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_mock_set_clip_status(
    mock: *mut MediusMockBox,
    value: MediusClipStatus,
) {
    with_mock(mock, |m| {
        if let Some(status) = clip_status_from_c(value) {
            m.set_clip_status(status);
        }
    });
}

/// Set the [`ClipSettings`](medius::ClipSettings) the mock answers to `medius_clip_query_config`.
/// `value.packet_triggers[0..packet_n]` are bound in order, as `medius_clip_bind_packet` binds them,
/// under the opt-in the mock holds when they are scripted. The mock holds the ones the box would
/// take, each with its scripted `hits`, and leaves out the rest as the box's own answer would: a
/// direction the class never carries, a match bit outside the mask, a run with no condition,
/// `consume` on `CONTROL` or with the opt-in off, and entries past the match pool. A trigger with a
/// byte no constant names is skipped. Script the opt-in with `medius_mock_set_imperfect_status`
/// before a consuming trigger. The triggers held are the set `medius_clip_bind_packet` adds to and
/// `medius_mock_clip_packet` runs a packet through.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_mock_set_clip_settings(
    mock: *mut MediusMockBox,
    value: MediusClipSettings,
) {
    with_mock(mock, |m| {
        m.set_clip_settings(crate::convert::clip_settings_from_c(&value))
    });
}

/// Run one packet through the mock's packet triggers, as the box does for a packet crossing `class`
/// at `id` in `direction` whose first bytes are `head[0..head_len]`. The most specific trigger the
/// head matches counts it in its `hits`. Returns whether that trigger drives its action on this
/// packet, with the `MEDIUS_CLIP_ACTION_*` value in `*out_action`; false when no trigger matches,
/// and when that trigger is `once_per_run` and the packet continues a run. `*out_consumed` is
/// whether that trigger consumes the packet, whatever the return. A null out is skipped.
///
/// A packet travels `POSITIVE` (IN) or `NEGATIVE` (OUT) across a surface that carries that flow: IN
/// for `MEDIUS_CATCH_CLASS_HID_IN` and `_EMIT`, OUT for `_HID_OUT`, either for the vendor classes and
/// `_CONTROL`. Any other `class` and `direction`, a byte no constant names among them, is no packet:
/// it returns false with `*out_consumed` false, counts in no `hits` and leaves every run as it was.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_mock_clip_packet(
    mock: *mut MediusMockBox,
    class: u8,
    id: u16,
    direction: u8,
    head: *const u8,
    head_len: usize,
    out_action: *mut u8,
    out_consumed: *mut bool,
) -> bool {
    guard(false, || {
        if !out_consumed.is_null() {
            unsafe { *out_consumed = false };
        }
        if mock.is_null() {
            return false;
        }
        let (Some(class), Some(direction), Some(head)) = (
            traffic_class_from_c(class),
            medius::Direction::from_u8(direction),
            unsafe { opt_slice(head, head_len) },
        ) else {
            return false;
        };
        let (action, consumed) = unsafe { &(*mock).inner }.clip_packet(class, id, direction, head);
        if !out_consumed.is_null() {
            unsafe { *out_consumed = consumed };
        }
        match action {
            Some(a) => {
                if !out_action.is_null() {
                    unsafe { *out_action = clip_action_to_c(a) };
                }
                true
            }
            None => false,
        }
    })
}

/// Simulate a device-chip restart: the mock drops its session state, keeps what it stores, sends its
/// hello now and again on the next frame it receives, and has its clone back 100 ms later.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_mock_restart(mock: *mut MediusMockBox) {
    with_mock(mock, |m| m.restart());
}

/// Simulate the inter-chip link dropping and coming back: the mock releases the session a host set
/// (counted in `MediusStats::session`) and the clone stays up.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_mock_link_lost(mock: *mut MediusMockBox) {
    with_mock(mock, |m| m.link_lost());
}

/// Simulate the real device detaching: the mock releases the session a host set at once. With
/// `back_within_grace` the same device re-attaches inside the 250 ms grace and the clone stays up;
/// otherwise the clone is torn down when the grace ends, which counts again only after a command
/// arrived in it, and stays down until `medius_mock_attach`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_mock_detach(mock: *mut MediusMockBox, back_within_grace: bool) {
    with_mock(mock, |m| m.detach(back_within_grace));
}

/// Simulate the device attaching again: inside a detach's grace the clone stays as it is; after the
/// teardown a fresh clone starts, which has nothing to release.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_mock_attach(mock: *mut MediusMockBox) {
    with_mock(mock, |m| m.attach());
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

/// Push a LOG line as if the box emitted it (surfaces on the device's log stream). `level` takes a
/// `MEDIUS_LOG_LEVEL_*` constant; any other value reads as `INFO`, which is what the wire decoder
/// does with a level byte it does not know.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_mock_push_log(
    mock: *mut MediusMockBox,
    level: u8,
    text: *const c_char,
) {
    with_mock(mock, |m| {
        if text.is_null() {
            return;
        }
        let text = unsafe { CStr::from_ptr(text) }.to_string_lossy();
        m.push_log(medius::LogLevel::from_u8(level), &text);
    });
}

/// Push a MOTION_EVENT as if the box emitted it (surfaces as a `Motion` catch event).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_mock_push_motion(
    mock: *mut MediusMockBox,
    seq: u8,
    ts_us: u32,
    event: MediusMotionEvent,
) {
    with_mock(mock, |m| {
        m.push_motion(seq, ts_us, event.dx, event.dy, event.dz, event.pan)
    });
}

/// Push a USAGE_EVENT as if the box emitted it (surfaces as a `Usages` catch event).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_mock_push_usages(
    mock: *mut MediusMockBox,
    seq: u8,
    ts_us: u32,
    event: *const MediusUsageEvent,
) {
    with_mock(mock, |m| {
        if event.is_null() {
            return;
        }
        let e = unsafe { &*event };
        let (Some(class), Some(direction)) = (
            medius::Class::from_u8(e.class),
            medius::Direction::from_u8(e.direction),
        ) else {
            return;
        };
        let usages = crate::convert::usage_event_to_medius(e);
        m.push_usages(seq, ts_us, class, direction, &usages);
    });
}

/// Push a TRAFFIC_EVENT as if the box emitted it (surfaces as a `Traffic` catch event).
/// `true_len` above `len` is how a snaplen-truncated capture looks.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_mock_push_traffic(
    mock: *mut MediusMockBox,
    seq: u8,
    ts_us: u32,
    clock: u8,
    event: *const MediusTrafficEvent,
) {
    with_mock(mock, |m| {
        if event.is_null() {
            return;
        }
        let e = unsafe { &*event };
        let Some(clock) = clock_domain_from_c(clock) else {
            return;
        };
        let Some(class) = medius::CatchClass::from_u8(e.class) else {
            return;
        };
        let Some(direction) = medius::Direction::from_u8(e.direction) else {
            return;
        };
        let n = (e.len as usize).min(MEDIUS_MAX_TRAFFIC_BYTES);
        m.push_traffic(
            seq,
            ts_us,
            clock,
            class,
            e.id,
            direction,
            e.flags,
            e.true_len,
            &e.bytes[..n],
        );
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

/// Whether the host has sent at least one frame of the given type. `ty` takes a
/// `MEDIUS_FRAME_TYPE_*` constant; any other value reads as false.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_mock_saw(mock: *mut MediusMockBox, ty: u8) -> bool {
    guard(false, || {
        if mock.is_null() {
            return false;
        }
        match frame_type_from_c(ty) {
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

/// Read recorded frame `idx`: type to `*out_ty`, SEQ to `*out_seq`, up to `cap` payload bytes to `payload_buf`; returns the full payload length, or 0 if `idx` is out of range.
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
