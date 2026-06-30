//! Status codes and the thread-local last-error detail.

use std::cell::RefCell;
use std::os::raw::c_char;
use std::panic::{AssertUnwindSafe, catch_unwind};

use medius::Error;

/// The result of a fallible `medius_*` call. `MEDIUS_OK` is zero; everything else is a failure.
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediusStatus {
    Ok = 0,
    ErrIo = 1,
    ErrNotFound = 2,
    ErrNoReply = 3,
    ErrBadProtoVer = 4,
    ErrQueryTimeout = 5,
    ErrDisconnected = 6,
    ErrFrameTooLong = 7,
    ErrFlashTool = 8,
    ErrInvalidArg = 9,
    ErrPanic = 10,
    ErrUnknown = 11,
}

#[derive(Default)]
struct LastError {
    message: String,
    proto_ver: u8,
}

thread_local! {
    static LAST_ERROR: RefCell<LastError> = RefCell::new(LastError::default());
}

fn store_error(message: String, proto_ver: u8) {
    LAST_ERROR.with(|e| {
        let mut e = e.borrow_mut();
        e.message = message;
        e.proto_ver = proto_ver;
    });
}

pub(crate) fn clear_error() {
    LAST_ERROR.with(|e| {
        let mut e = e.borrow_mut();
        e.message.clear();
        e.proto_ver = 0;
    });
}

fn status_for(err: &Error) -> MediusStatus {
    match err {
        Error::Io(_) => MediusStatus::ErrIo,
        Error::NotFound => MediusStatus::ErrNotFound,
        Error::NoReply => MediusStatus::ErrNoReply,
        Error::BadProtoVer { .. } => MediusStatus::ErrBadProtoVer,
        Error::QueryTimeout => MediusStatus::ErrQueryTimeout,
        Error::Disconnected => MediusStatus::ErrDisconnected,
        Error::FrameTooLong => MediusStatus::ErrFrameTooLong,
        #[cfg(feature = "flash")]
        Error::FlashTool(_) => MediusStatus::ErrFlashTool,
        _ => MediusStatus::ErrUnknown,
    }
}

pub(crate) fn record(err: &Error) -> MediusStatus {
    let proto_ver = match err {
        Error::BadProtoVer { got } => *got,
        _ => 0,
    };
    store_error(err.to_string(), proto_ver);
    status_for(err)
}

pub(crate) fn fail(status: MediusStatus, message: &str) -> MediusStatus {
    store_error(message.to_string(), 0);
    status
}

/// Map a `Result<(), Error>` to a status, clearing the last error on success.
pub(crate) fn status_of(result: Result<(), Error>) -> MediusStatus {
    match result {
        Ok(()) => {
            clear_error();
            MediusStatus::Ok
        }
        Err(e) => record(&e),
    }
}

/// Run a status-returning body, catching any panic (unwinding across FFI is UB).
pub(crate) fn guard_status(f: impl FnOnce() -> MediusStatus) -> MediusStatus {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(s) => s,
        Err(_) => fail(MediusStatus::ErrPanic, "internal panic at the FFI boundary"),
    }
}

/// Run a value-returning body, returning `default` on a caught panic.
pub(crate) fn guard<T>(default: T, f: impl FnOnce() -> T) -> T {
    catch_unwind(AssertUnwindSafe(f)).unwrap_or(default)
}

/// Copy the last error's display text into `buf` (NUL-terminated, truncated to `cap`). Returns the
/// full message length in bytes, excluding the NUL, so a caller can size a buffer and retry.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_last_error_message(buf: *mut c_char, cap: usize) -> usize {
    guard(0, || {
        LAST_ERROR.with(|e| {
            let e = e.borrow();
            let bytes = e.message.as_bytes();
            let full = bytes.len();
            if !buf.is_null() && cap > 0 {
                let n = full.min(cap - 1);
                unsafe {
                    std::ptr::copy_nonoverlapping(bytes.as_ptr(), buf as *mut u8, n);
                    *buf.add(n) = 0;
                }
            }
            full
        })
    })
}

/// The `BadProtoVer` version byte from the last error, or 0 if the last error carried none.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_last_error_proto_ver() -> u8 {
    guard(0, || LAST_ERROR.with(|e| e.borrow().proto_ver))
}
