//! The firmware-flash C ABI (feature = `flash`).

use std::ffi::CStr;
use std::os::raw::c_char;

use crate::error::{MediusStatus, fail, guard_status};

/// Reboot a chip to ROM download and flash `bin_path` via esptool on PATH. `host` selects the host
/// chip (otherwise the device chip). Blocking (~2 s settle + subprocess). Platform-gated to Linux and
/// Windows; returns `ErrUnknown` elsewhere.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_flash(
    port: *const c_char,
    bin_path: *const c_char,
    host: bool,
) -> MediusStatus {
    guard_status(|| {
        if port.is_null() || bin_path.is_null() {
            return fail(MediusStatus::ErrInvalidArg, "null pointer");
        }
        let Ok(port) = (unsafe { CStr::from_ptr(port) }).to_str() else {
            return fail(MediusStatus::ErrInvalidArg, "port is not valid UTF-8");
        };
        let Ok(bin) = (unsafe { CStr::from_ptr(bin_path) }).to_str() else {
            return fail(MediusStatus::ErrInvalidArg, "bin_path is not valid UTF-8");
        };

        #[cfg(any(target_os = "linux", windows))]
        {
            use crate::error::{clear_error, record};
            match medius::flash::flash(port, bin, host) {
                Ok(()) => {
                    clear_error();
                    MediusStatus::Ok
                }
                Err(e) => record(&e),
            }
        }
        #[cfg(not(any(target_os = "linux", windows)))]
        {
            let _ = (port, bin, host);
            fail(
                MediusStatus::ErrUnknown,
                "flash is only supported on Linux and Windows",
            )
        }
    })
}
