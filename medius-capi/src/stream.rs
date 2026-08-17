//! The opaque catch- and log-stream handles and their receive functions.

use std::time::Duration;

use crate::convert::{
    catch_filter_from_c, class_from_c, clock_domain_to_native, input_event_to_c, usage_to_c,
};
use crate::ctypes::*;
use crate::device::MediusDevice;
use crate::error::{MediusStatus, clear_error, fail, guard, guard_status, record};

/// A live CATCH event stream; create with `medius_device_catch_events`, free with `medius_event_stream_free`.
pub struct MediusEventStream {
    pub(crate) inner: medius::EventStream,
}

/// A device LOG stream. Opaque; create with `medius_device_logs`, release with `medius_log_stream_free`.
pub struct MediusLogStream {
    pub(crate) inner: medius::LogStream,
}

/// Subscribe to the catch stream for `filters[0..n]` (build them with the `medius_catch_filter_*` helpers); writes the handle to `*out`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_catch_events(
    dev: *mut MediusDevice,
    filters: *const MediusCatchFilter,
    n: usize,
    out: *mut *mut MediusEventStream,
) -> MediusStatus {
    guard_status(|| {
        if dev.is_null() || out.is_null() || filters.is_null() {
            return fail(MediusStatus::ErrInvalidArg, "null pointer");
        }
        if n == 0 {
            return fail(MediusStatus::ErrInvalidArg, "empty filter list");
        }
        let slice = unsafe { std::slice::from_raw_parts(filters, n) };
        // Reject the whole call rather than dropping the offender: a subscription silently narrower
        // than the caller asked for looks like the box producing no events.
        let mut parsed = Vec::with_capacity(n);
        for f in slice {
            match catch_filter_from_c(*f) {
                Some(f) => parsed.push(f),
                None => return fail(MediusStatus::ErrInvalidArg, "unknown catch class"),
            }
        }
        let d = unsafe { &(*dev).inner };
        match d.catch_events(parsed) {
            Ok(stream) => {
                unsafe { *out = Box::into_raw(Box::new(MediusEventStream { inner: stream })) };
                clear_error();
                MediusStatus::Ok
            }
            Err(e) => record(&e),
        }
    })
}

/// Clone an event-stream handle onto the same subscription (shared queue); null in returns null out.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_event_stream_clone(
    stream: *const MediusEventStream,
) -> *mut MediusEventStream {
    guard(std::ptr::null_mut(), || {
        if stream.is_null() {
            return std::ptr::null_mut();
        }
        Box::into_raw(Box::new(MediusEventStream {
            inner: unsafe { (*stream).inner.clone() },
        }))
    })
}

/// Free an event-stream handle. Null is a no-op.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_event_stream_free(stream: *mut MediusEventStream) {
    guard((), || {
        if !stream.is_null() {
            drop(unsafe { Box::from_raw(stream) });
        }
    });
}

/// Block until the next physical-input event, writing it to `*out`; `ErrDisconnected` on close.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_event_stream_recv(
    stream: *mut MediusEventStream,
    out: *mut MediusCatchEvent,
) -> MediusStatus {
    guard_status(|| {
        if stream.is_null() || out.is_null() {
            return fail(MediusStatus::ErrInvalidArg, "null pointer");
        }
        let s = unsafe { &(*stream).inner };
        match s.recv() {
            Ok(ev) => {
                unsafe { *out = ev.into() };
                clear_error();
                MediusStatus::Ok
            }
            Err(e) => record(&e),
        }
    })
}

/// The next buffered event, written to `*out`; returns false if none is queued (never blocks).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_event_stream_try_recv(
    stream: *mut MediusEventStream,
    out: *mut MediusCatchEvent,
) -> bool {
    guard(false, || {
        if stream.is_null() || out.is_null() {
            return false;
        }
        let s = unsafe { &(*stream).inner };
        match s.try_recv() {
            Some(ev) => {
                unsafe { *out = ev.into() };
                true
            }
            None => false,
        }
    })
}

/// Block up to `timeout_ms` for the next event, written to `*out`; returns false on timeout or close.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_event_stream_recv_timeout(
    stream: *mut MediusEventStream,
    timeout_ms: u64,
    out: *mut MediusCatchEvent,
) -> bool {
    guard(false, || {
        if stream.is_null() || out.is_null() {
            return false;
        }
        let s = unsafe { &(*stream).inner };
        match s.recv_timeout(Duration::from_millis(timeout_ms)) {
            Some(ev) => {
                unsafe { *out = ev.into() };
                true
            }
            None => false,
        }
    })
}

/// Events this stream dropped because the consumer fell behind (host-side back-pressure).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_event_stream_dropped(stream: *mut MediusEventStream) -> u64 {
    guard(0, || {
        if stream.is_null() {
            return 0;
        }
        unsafe { (*stream).inner.dropped() }
    })
}

fn fail_bool(message: &str) -> bool {
    fail(MediusStatus::ErrInvalidArg, message);
    false
}

/// The caller's clock and ours share no origin, so the arrival is fed in on OUR scale and the answer
/// shifted back onto theirs. Only differences survive the round trip, which is all either scale
/// carries -- so any consistent nanosecond source works, as long as every call uses the same one.
/// `checked_add` is belt and braces: nothing a caller can pass overflows on a 64-bit `Instant`, but
/// panicking across the FFI boundary is not an option if some target's is narrower.
///
/// # Safety
/// `t` and `out` must be non-null and valid.
unsafe fn timeline_observe(
    t: *mut MediusTimeline,
    ts_us: u32,
    clock: MediusClockDomain,
    now_ns: u64,
    out: *mut MediusStamped,
) -> bool {
    let tl = unsafe { &mut *t };
    let base = tl.origin;
    let Some(arrival) = base.checked_add(Duration::from_nanos(now_ns)) else {
        return fail_bool("now_ns is too large to be a monotonic reading");
    };
    let st = tl
        .inner
        .observe_stamp(ts_us, clock_domain_to_native(clock), arrival);
    let host_ns = st
        .host
        .saturating_duration_since(base)
        .as_nanos()
        .min(u64::MAX as u128) as u64;
    unsafe {
        *out = MediusStamped {
            host_ns,
            box_us: st.box_us,
            excess_ns: st.excess.as_nanos().min(u64::MAX as u128) as u64,
        }
    };
    true
}

/// A live decoded-input stream; create with `medius_device_input_events`, free with
/// `medius_input_stream_free`. Not clonable: it holds the per-class held sets it diffs.
pub struct MediusInputStream {
    pub(crate) inner: medius::InputStream,
}

/// Subscribe to decoded input edges for `filters[0..n]`, writing the handle to `*out`.
///
/// Every filter must name an input class and cover both edges; build them with
/// `medius_catch_filter_watch*` or `medius_catch_filter_all_input`. A traffic class, the everything
/// filter, or a filter narrowed to one edge is refused rather than silently yielding nothing.
///
/// # Safety
/// `filters` must point to `n` readable `MediusCatchFilter`; `dev` and `out` must be non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_input_events(
    dev: *mut MediusDevice,
    filters: *const MediusCatchFilter,
    n: usize,
    out: *mut *mut MediusInputStream,
) -> MediusStatus {
    guard_status(|| {
        if dev.is_null() || out.is_null() || filters.is_null() {
            return fail(MediusStatus::ErrInvalidArg, "null pointer");
        }
        if n == 0 {
            return fail(MediusStatus::ErrInvalidArg, "empty filter list");
        }
        let slice = unsafe { std::slice::from_raw_parts(filters, n) };
        let mut parsed = Vec::with_capacity(n);
        for f in slice {
            match catch_filter_from_c(*f) {
                Some(f) => parsed.push(f),
                None => return fail(MediusStatus::ErrInvalidArg, "unknown catch class"),
            }
        }
        let d = unsafe { &(*dev).inner };
        match d.input_events(parsed) {
            Ok(stream) => {
                unsafe { *out = Box::into_raw(Box::new(MediusInputStream { inner: stream })) };
                clear_error();
                MediusStatus::Ok
            }
            Err(e) => record(&e),
        }
    })
}

/// Free an input-stream handle. Null is a no-op.
///
/// # Safety
/// `stream` must come from `medius_device_input_events` and not be used afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_input_stream_free(stream: *mut MediusInputStream) {
    guard((), || {
        if !stream.is_null() {
            drop(unsafe { Box::from_raw(stream) });
        }
    });
}

/// Block until the next input event, writing it to `*out`; `ErrDisconnected` on close.
///
/// # Safety
/// `stream` and `out` must be non-null and valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_input_stream_recv(
    stream: *mut MediusInputStream,
    out: *mut MediusInputEvent,
) -> MediusStatus {
    guard_status(|| {
        if stream.is_null() || out.is_null() {
            return fail(MediusStatus::ErrInvalidArg, "null pointer");
        }
        let s = unsafe { &mut (*stream).inner };
        match s.recv() {
            Ok(ev) => {
                unsafe { *out = input_event_to_c(ev) };
                clear_error();
                MediusStatus::Ok
            }
            Err(e) => record(&e),
        }
    })
}

/// The next decoded event, written to `*out`; returns false if none is queued (never blocks).
///
/// # Safety
/// `stream` and `out` must be non-null and valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_input_stream_try_recv(
    stream: *mut MediusInputStream,
    out: *mut MediusInputEvent,
) -> bool {
    guard(false, || {
        if stream.is_null() || out.is_null() {
            return false;
        }
        let s = unsafe { &mut (*stream).inner };
        match s.try_recv() {
            Some(ev) => {
                unsafe { *out = input_event_to_c(ev) };
                true
            }
            None => false,
        }
    })
}

/// Block up to `timeout_ms` for the next input event; returns false on timeout or close.
///
/// # Safety
/// `stream` and `out` must be non-null and valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_input_stream_recv_timeout(
    stream: *mut MediusInputStream,
    timeout_ms: u64,
    out: *mut MediusInputEvent,
) -> bool {
    guard(false, || {
        if stream.is_null() || out.is_null() {
            return false;
        }
        let s = unsafe { &mut (*stream).inner };
        match s.recv_timeout(Duration::from_millis(timeout_ms)) {
            Some(ev) => {
                unsafe { *out = input_event_to_c(ev) };
                true
            }
            None => false,
        }
    })
}

/// Events the underlying subscription dropped because the consumer fell behind.
///
/// # Safety
/// `stream` must be non-null and valid, or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_input_stream_dropped(stream: *mut MediusInputStream) -> u64 {
    guard(0, || {
        if stream.is_null() {
            return 0;
        }
        unsafe { (*stream).inner.dropped() }
    })
}

/// Write the usages of `class` this stream currently holds to `out[0..cap]` and return how many there
/// are. A return above `cap` means the buffer was too small and only `cap` were written.
///
/// # Safety
/// `out` must point to space for `cap` `MediusUsage`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_input_stream_held(
    stream: *mut MediusInputStream,
    class: MediusClass,
    out: *mut MediusUsage,
    cap: usize,
) -> usize {
    guard(0, || {
        if stream.is_null() {
            return 0;
        }
        let held = unsafe { (*stream).inner.held(class_from_c(class)) };
        if !out.is_null() {
            for (i, u) in held.iter().take(cap).enumerate() {
                unsafe { *out.add(i) = usage_to_c(*u) };
            }
        }
        held.len()
    })
}

/// A host-side clock mapping; create with `medius_timeline_new`, free with `medius_timeline_free`.
///
/// A catch stamp is microseconds on a chip that booted before this process did: it wraps every ~71.6
/// minutes and has no relation to any clock here. Feed every event in as it arrives, in order,
/// passing your own monotonic `now_ns`.
pub struct MediusTimeline {
    inner: medius::Timeline,
    origin: std::time::Instant,
}

/// A fresh timeline. Free with `medius_timeline_free`.
#[unsafe(no_mangle)]
pub extern "C" fn medius_timeline_new() -> *mut MediusTimeline {
    guard(std::ptr::null_mut(), || {
        Box::into_raw(Box::new(MediusTimeline {
            inner: medius::Timeline::new(),
            origin: std::time::Instant::now(),
        }))
    })
}

/// Free a timeline. Null is a no-op.
///
/// # Safety
/// `t` must come from `medius_timeline_new` and not be used afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_timeline_free(t: *mut MediusTimeline) {
    guard((), || {
        if !t.is_null() {
            drop(unsafe { Box::from_raw(t) });
        }
    });
}

/// Place `ev` on the caller's clock, writing the result to `*out`; returns false on a null argument.
///
/// `now_ns` is the caller's own monotonic reading at the moment the event arrived, in nanoseconds
/// from any fixed origin. `MediusStamped::host_ns` comes back on that same scale.
///
/// # Safety
/// `t`, `ev` and `out` must be non-null and valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_timeline_observe(
    t: *mut MediusTimeline,
    ev: *const MediusCatchEvent,
    now_ns: u64,
    out: *mut MediusStamped,
) -> bool {
    guard(false, || {
        if t.is_null() || ev.is_null() || out.is_null() {
            return false;
        }
        let (ts_us, clock) = unsafe { ((*ev).ts_us, (*ev).clock) };
        unsafe { timeline_observe(t, ts_us, clock, now_ns, out) }
    })
}

/// Place a decoded input event on the caller's clock; the input-stream counterpart of
/// `medius_timeline_observe`. Both share one timeline, so a caller reading both streams gets one
/// comparable ordering.
///
/// # Safety
/// `t`, `ev` and `out` must be non-null and valid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_timeline_observe_input(
    t: *mut MediusTimeline,
    ev: *const MediusInputEvent,
    now_ns: u64,
    out: *mut MediusStamped,
) -> bool {
    guard(false, || {
        if t.is_null() || ev.is_null() || out.is_null() {
            return false;
        }
        let (ts_us, clock) = unsafe { ((*ev).ts_us, (*ev).clock) };
        unsafe { timeline_observe(t, ts_us, clock, now_ns, out) }
    })
}

/// Forget one domain's rollover count and measured floor, for a chip that rebooted.
///
/// # Safety
/// `t` must be non-null and valid, or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_timeline_reset(t: *mut MediusTimeline, domain: MediusClockDomain) {
    guard((), || {
        if !t.is_null() {
            unsafe { (*t).inner.reset(clock_domain_to_native(domain)) };
        }
    });
}

/// Events observed for a domain. The floor is a minimum over these: a handful is a loose estimate.
///
/// # Safety
/// `t` must be non-null and valid, or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_timeline_samples(
    t: *mut MediusTimeline,
    domain: MediusClockDomain,
) -> u64 {
    guard(0, || {
        if t.is_null() {
            return 0;
        }
        unsafe { (*t).inner.samples(clock_domain_to_native(domain)) }
    })
}

/// Whether the box is still delivering to this stream.
///
/// `medius_event_stream_recv_timeout` and `_try_recv` both return `false` for "nothing yet" and for
/// "nothing ever again". This separates them: one means wait longer, the other means stop.
///
/// # Safety
/// `stream` must be non-null and valid, or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_event_stream_is_connected(stream: *mut MediusEventStream) -> bool {
    guard(false, || {
        if stream.is_null() {
            return false;
        }
        unsafe { (*stream).inner.is_connected() }
    })
}

/// Whether the box is still delivering to this input stream. See
/// `medius_event_stream_is_connected`.
///
/// # Safety
/// `stream` must be non-null and valid, or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_input_stream_is_connected(stream: *mut MediusInputStream) -> bool {
    guard(false, || {
        if stream.is_null() {
            return false;
        }
        unsafe { (*stream).inner.is_connected() }
    })
}

/// Open the device LOG stream, writing the handle to `*out`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_logs(
    dev: *mut MediusDevice,
    out: *mut *mut MediusLogStream,
) -> MediusStatus {
    guard_status(|| {
        if dev.is_null() || out.is_null() {
            return fail(MediusStatus::ErrInvalidArg, "null pointer");
        }
        let d = unsafe { &(*dev).inner };
        let stream = d.logs();
        unsafe { *out = Box::into_raw(Box::new(MediusLogStream { inner: stream })) };
        clear_error();
        MediusStatus::Ok
    })
}

/// Clone a log-stream handle: another handle to the same LOG channel. Null in -> null out.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_log_stream_clone(
    stream: *const MediusLogStream,
) -> *mut MediusLogStream {
    guard(std::ptr::null_mut(), || {
        if stream.is_null() {
            return std::ptr::null_mut();
        }
        Box::into_raw(Box::new(MediusLogStream {
            inner: unsafe { (*stream).inner.clone() },
        }))
    })
}

/// Free a log-stream handle. Null is a no-op.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_log_stream_free(stream: *mut MediusLogStream) {
    guard((), || {
        if !stream.is_null() {
            drop(unsafe { Box::from_raw(stream) });
        }
    });
}

/// Block until the next LOG line, writing it to `*out`. Returns `ErrDisconnected` on close.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_log_stream_recv(
    stream: *mut MediusLogStream,
    out: *mut MediusLogLine,
) -> MediusStatus {
    guard_status(|| {
        if stream.is_null() || out.is_null() {
            return fail(MediusStatus::ErrInvalidArg, "null pointer");
        }
        let s = unsafe { &(*stream).inner };
        match s.recv() {
            Ok(line) => {
                unsafe { *out = (&line).into() };
                clear_error();
                MediusStatus::Ok
            }
            Err(e) => record(&e),
        }
    })
}

/// The next buffered LOG line, written to `*out`; returns false if none is queued (never blocks).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_log_stream_try_recv(
    stream: *mut MediusLogStream,
    out: *mut MediusLogLine,
) -> bool {
    guard(false, || {
        if stream.is_null() || out.is_null() {
            return false;
        }
        let s = unsafe { &(*stream).inner };
        match s.try_recv() {
            Some(line) => {
                unsafe { *out = (&line).into() };
                true
            }
            None => false,
        }
    })
}

/// Block up to `timeout_ms` for the next LOG line, written to `*out`; false on timeout or close.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_log_stream_recv_timeout(
    stream: *mut MediusLogStream,
    timeout_ms: u64,
    out: *mut MediusLogLine,
) -> bool {
    guard(false, || {
        if stream.is_null() || out.is_null() {
            return false;
        }
        let s = unsafe { &(*stream).inner };
        match s.recv_timeout(Duration::from_millis(timeout_ms)) {
            Some(line) => {
                unsafe { *out = (&line).into() };
                true
            }
            None => false,
        }
    })
}
