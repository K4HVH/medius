//! The opaque catch- and log-stream handles and their receive functions.

use std::time::Duration;

use medius::CatchMask;

use crate::ctypes::*;
use crate::device::MediusDevice;
use crate::error::{MediusStatus, clear_error, fail, guard, guard_status, record};

/// A live CATCH event stream. Opaque; create with `medius_device_catch_events`, release with
/// `medius_event_stream_free` (which unsubscribes when the last handle drops).
pub struct MediusEventStream {
    pub(crate) inner: medius::EventStream,
}

/// A device LOG stream. Opaque; create with `medius_device_logs`, release with `medius_log_stream_free`.
pub struct MediusLogStream {
    pub(crate) inner: medius::LogStream,
}

/// Subscribe to the physical-input event stream for the given class `mask` (the `MEDIUS_CATCH_MASK_*`
/// bits). Writes the stream handle to `*out`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_device_catch_events(
    dev: *mut MediusDevice,
    mask: MediusCatchMask,
    out: *mut *mut MediusEventStream,
) -> MediusStatus {
    guard_status(|| {
        if dev.is_null() || out.is_null() {
            return fail(MediusStatus::ErrInvalidArg, "null pointer");
        }
        let d = unsafe { &(*dev).inner };
        match d.catch_events(CatchMask::from_bits_truncate(mask)) {
            Ok(stream) => {
                unsafe { *out = Box::into_raw(Box::new(MediusEventStream { inner: stream })) };
                clear_error();
                MediusStatus::Ok
            }
            Err(e) => record(&e),
        }
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

/// Block until the next physical-input event, writing it to `*out`. Returns `ErrDisconnected` when the
/// stream closes (after reset or link loss).
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
