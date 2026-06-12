//! Raw Windows serial transport via `windows-sys` (§6 of the design spec).
//!
//! Opens `\\.\COMn` directly and configures a custom `DCB` for the box's 4 Mbaud raw link with DTR
//! and RTS control **disabled** (`DTR_CONTROL_DISABLE` / `RTS_CONTROL_DISABLE`, the bitfield value
//! `0`), 8-N-1, and no flow control. A bounded read timeout via `COMMTIMEOUTS` makes
//! [`Transport::read`] return `Ok(0)` on idle so the reader thread can poll its stop flag.
//!
//! This file only type-checks on Linux (`cargo check --target x86_64-pc-windows-msvc`); it is never
//! run here. The HANDLE is owned and closed once in [`Drop`].
//!
//! ## `DCB` bitfield
//!
//! `windows-sys` exposes the `DCB` control flags as a single packed `_bitfield: u32` rather than
//! named bits. We build it explicitly (see [`dcb_bitfield`]): set `fBinary` (bit 0 — required to be
//! 1 for a valid DCB) and leave every other flag at 0, which means `fParity=0`,
//! `fOutxCtsFlow/fOutxDsrFlow=0`, `fDtrControl=DTR_CONTROL_DISABLE(0)`, `fDsrSensitivity=0`,
//! `fOutX/fInX=0` (no XON/XOFF), `fRtsControl=RTS_CONTROL_DISABLE(0)`, `fAbortOnError=0`. That is
//! exactly the "no flow control, DTR/RTS disabled, binary mode" configuration §6 requires.

use std::io;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;

use windows_sys::Win32::Devices::Communication::{
    COMMTIMEOUTS, DCB, GetCommState, NOPARITY, ONESTOPBIT, SetCommState, SetCommTimeouts,
};
use windows_sys::Win32::Foundation::{
    CloseHandle, GENERIC_READ, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Storage::FileSystem::{CreateFileW, OPEN_EXISTING, ReadFile, WriteFile};

/// The control link baud (§6).
const CTRL_BAUD: u32 = 4_000_000;

/// `fBinary` — bit 0 of the `DCB` control bitfield. Must be 1 for a valid DCB (Windows ignores any
/// request to set it to 0).
const DCB_F_BINARY: u32 = 0x0000_0001;

/// Compute the `DCB` `_bitfield`: `fBinary = 1`, every other control flag `0`.
///
/// Pure and testable. With all other bits zero, the DCB has no parity checking, no CTS/DSR/XON-XOFF
/// flow control, `fDtrControl = DTR_CONTROL_DISABLE`, and `fRtsControl = RTS_CONTROL_DISABLE` — the
/// configuration §6 requires (DTR/RTS must not be asserted: that resets the device chip).
fn dcb_bitfield() -> u32 {
    DCB_F_BINARY
}

/// Build the fully configured `DCB` for the 4 Mbaud raw 8-N-1 link.
///
/// Pure (no I/O): takes the `DCB` read back from `GetCommState` and returns the one to write with
/// `SetCommState`, so the field-setting logic is unit-testable without a device.
fn configure_dcb(mut dcb: DCB) -> DCB {
    dcb.DCBlength = core::mem::size_of::<DCB>() as u32;
    dcb.BaudRate = CTRL_BAUD;
    dcb.ByteSize = 8;
    dcb.Parity = NOPARITY;
    dcb.StopBits = ONESTOPBIT;
    dcb._bitfield = dcb_bitfield();
    // Flow-control char limits are irrelevant with flow control disabled, but zero them for
    // determinism.
    dcb.XonLim = 0;
    dcb.XoffLim = 0;
    dcb
}

/// Build the `COMMTIMEOUTS` for a bounded read: return whatever bytes are available, but block at
/// most ~100 ms when idle so [`Transport::read`] yields `Ok(0)` and the reader thread can poll its
/// stop flag.
///
/// With `ReadIntervalTimeout = MAXDWORD`, `ReadTotalTimeoutMultiplier = 0`, and
/// `ReadTotalTimeoutConstant = 100`, the total read timeout is a flat 100 ms: `ReadFile` waits up to
/// 100 ms and returns with whatever bytes arrived (possibly 0). This is deliberately **not** the
/// `MAXDWORD / MAXDWORD / nonzero` "wait for the first byte then return" special case, and crucially
/// **not** the `MAXDWORD / 0 / 0` "return immediately with whatever is buffered" mode — that last one
/// makes an idle `ReadFile` return `Ok(0)` *instantly*, spinning the reader thread at 100% CPU. The
/// nonzero `ReadTotalTimeoutConstant` is load-bearing; do not "simplify" it to zero.
fn read_timeouts() -> COMMTIMEOUTS {
    COMMTIMEOUTS {
        // Don't wait between bytes once data starts arriving.
        ReadIntervalTimeout: u32::MAX,
        ReadTotalTimeoutMultiplier: 0,
        // Block at most 100 ms total for a read when idle.
        ReadTotalTimeoutConstant: 100,
        // Writes are blocking with no artificial timeout.
        WriteTotalTimeoutMultiplier: 0,
        WriteTotalTimeoutConstant: 0,
    }
}

/// Encode a port path as the `\\.\COMn` wide string `CreateFileW` expects.
///
/// Pure/testable. A bare `COMn` is prefixed with `\\.\` (needed for `COM10`+); a path that already
/// starts with `\\.\` is passed through. Returns a NUL-terminated UTF-16 buffer.
fn device_path_wide(path: &str) -> Vec<u16> {
    let full = if path.starts_with(r"\\.\") {
        path.to_string()
    } else {
        format!(r"\\.\{path}")
    };
    std::ffi::OsStr::new(&full)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

/// An owned, configured serial HANDLE. Closes it on drop.
pub(crate) struct WindowsSerial {
    handle: HANDLE,
}

// HANDLE is a raw pointer (`*mut c_void`); a Windows file HANDLE is safe to use across threads
// (concurrent ReadFile/WriteFile on one handle is supported), so we assert Send + Sync. The handle
// is owned exclusively by this struct.
// SAFETY: the HANDLE is an owned OS file handle; the OS permits concurrent read/write on it and we
// never alias the raw value elsewhere.
unsafe impl Send for WindowsSerial {}
// SAFETY: see the Send impl — concurrent access from multiple threads is OS-supported.
unsafe impl Sync for WindowsSerial {}

impl std::fmt::Debug for WindowsSerial {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WindowsSerial")
            .field("handle", &self.handle)
            .finish()
    }
}

impl WindowsSerial {
    /// Open and configure a COM port (e.g. `COM7`) for the 4 Mbaud raw control link.
    pub(crate) fn open(path: &Path) -> io::Result<Self> {
        let path_str = path.to_string_lossy();
        let wide = device_path_wide(&path_str);

        // SAFETY: `wide` is a valid NUL-terminated UTF-16 string living for the call. We pass null
        // security attributes and template handle (both documented as optional), no sharing
        // (exclusive open), OPEN_EXISTING (the port must exist), and no special flags. The returned
        // HANDLE is checked against INVALID_HANDLE_VALUE below.
        let handle = unsafe {
            CreateFileW(
                wide.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                0, // no sharing
                core::ptr::null(),
                OPEN_EXISTING,
                0,
                core::ptr::null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }

        // Own it immediately so any early return below still closes via Drop.
        let serial = WindowsSerial { handle };
        serial.configure()?;
        Ok(serial)
    }

    /// Read the current `DCB`, apply our configuration, and write it back; then set read timeouts.
    fn configure(&self) -> io::Result<()> {
        // SAFETY: zeroed DCB is a valid initial value; GetCommState overwrites it.
        let mut dcb: DCB = unsafe { core::mem::zeroed() };
        dcb.DCBlength = core::mem::size_of::<DCB>() as u32;

        // SAFETY: `self.handle` is a valid open serial handle; `&mut dcb` is a live, correctly sized
        // DCB the call fills in.
        let ok = unsafe { GetCommState(self.handle, &mut dcb) };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }

        let dcb = configure_dcb(dcb);
        // SAFETY: `self.handle` is valid; `&dcb` is a fully initialized DCB the call reads.
        let ok = unsafe { SetCommState(self.handle, &dcb) };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }

        let timeouts = read_timeouts();
        // SAFETY: `self.handle` is valid; `&timeouts` is a fully initialized COMMTIMEOUTS.
        let ok = unsafe { SetCommTimeouts(self.handle, &timeouts) };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}

impl super::Transport for WindowsSerial {
    fn write_all(&self, buf: &[u8]) -> io::Result<()> {
        let mut written = 0usize;
        while written < buf.len() {
            let mut n: u32 = 0;
            let remaining = (buf.len() - written).min(u32::MAX as usize) as u32;
            // SAFETY: `self.handle` is valid; we pass a pointer into `buf` at offset `written` with a
            // count bounded by the remaining length, and a live `&mut n` for the bytes-written out
            // param. No OVERLAPPED (synchronous write).
            let ok = unsafe {
                WriteFile(
                    self.handle,
                    buf[written..].as_ptr(),
                    remaining,
                    &mut n,
                    core::ptr::null_mut(),
                )
            };
            if ok == 0 {
                return Err(io::Error::last_os_error());
            }
            if n == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "serial write returned 0",
                ));
            }
            written += n as usize;
        }
        Ok(())
    }

    fn read(&self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let mut n: u32 = 0;
        let to_read = buf.len().min(u32::MAX as usize) as u32;
        // SAFETY: `self.handle` is valid; we pass `buf`'s pointer and a count bounded by its length,
        // and a live `&mut n` out param. No OVERLAPPED (synchronous read). On the COMMTIMEOUTS read
        // timeout this succeeds with `n == 0`.
        let ok = unsafe {
            ReadFile(
                self.handle,
                buf.as_mut_ptr(),
                to_read,
                &mut n,
                core::ptr::null_mut(),
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        // n == 0 here is the read timeout (no data within the window) — report it as Ok(0) so the
        // reader thread can poll its stop flag.
        Ok(n as usize)
    }
}

impl Drop for WindowsSerial {
    fn drop(&mut self) {
        // SAFETY: `self.handle` is an owned valid handle (never INVALID_HANDLE_VALUE here) and is
        // closed exactly once. DTR/RTS were disabled, so closing does not pulse-reset the chip.
        unsafe {
            CloseHandle(self.handle);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dcb_bitfield_sets_only_fbinary() {
        let bits = dcb_bitfield();
        // fBinary (bit 0) set.
        assert_eq!(bits & DCB_F_BINARY, DCB_F_BINARY);
        // Every other bit clear ⇒ DTR/RTS control DISABLE (0), no parity, no flow control.
        assert_eq!(bits, 0x0000_0001);
    }

    #[test]
    fn configure_dcb_sets_4m_8n1() {
        // SAFETY: a zeroed DCB is a valid POD starting point for this pure-logic test.
        let dcb: DCB = unsafe { core::mem::zeroed() };
        let dcb = configure_dcb(dcb);
        assert_eq!(dcb.BaudRate, 4_000_000);
        assert_eq!(dcb.ByteSize, 8);
        assert_eq!(dcb.Parity, NOPARITY);
        assert_eq!(dcb.StopBits, ONESTOPBIT);
        assert_eq!(dcb._bitfield, 0x0000_0001); // fBinary only
        assert_eq!(dcb.DCBlength, core::mem::size_of::<DCB>() as u32);
    }

    #[test]
    fn read_timeouts_are_bounded() {
        let t = read_timeouts();
        assert_eq!(t.ReadIntervalTimeout, u32::MAX);
        assert_eq!(t.ReadTotalTimeoutConstant, 100);
        assert_eq!(t.ReadTotalTimeoutMultiplier, 0);
    }

    #[test]
    fn device_path_prefixes_com_port() {
        let wide = device_path_wide("COM7");
        let s = String::from_utf16_lossy(&wide);
        assert!(s.starts_with(r"\\.\COM7"));
        assert_eq!(*wide.last().unwrap(), 0, "must be NUL-terminated");
    }

    #[test]
    fn device_path_passes_through_unc() {
        let wide = device_path_wide(r"\\.\COM12");
        let s: String = String::from_utf16_lossy(&wide);
        // No double prefix.
        assert!(s.starts_with(r"\\.\COM12"));
        assert!(!s.starts_with(r"\\.\\\.\"));
    }
}
