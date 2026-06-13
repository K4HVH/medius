//! Raw Windows serial transport via `windows-sys` (§6 of the design spec).
//!
//! Opens `\\.\COMn` directly with a custom `DCB` for the box's 4 Mbaud raw link: DTR/RTS control
//! disabled (asserting them resets the device chip), 8-N-1, no flow control. A bounded `COMMTIMEOUTS`
//! read timeout makes [`Transport::read`] return `Ok(0)` on idle so the reader thread polls.
//!
//! Type-checks under `--target x86_64-pc-windows-msvc` but is never run here.
//!
//! `windows-sys` exposes the DCB control flags as one packed `_bitfield: u32`, not named bits. We set
//! only `fBinary` (bit 0, required 1 for a valid DCB); every other flag at 0 yields no parity, no
//! CTS/DSR/XON-XOFF flow control, and `fDtrControl`/`fRtsControl = *_DISABLE(0)` — exactly §6's
//! config. See [`dcb_bitfield`].

use std::io;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;

use windows_sys::Win32::Devices::Communication::{
    COMMTIMEOUTS, DCB, GetCommState, NOPARITY, ONESTOPBIT, PURGE_RXCLEAR, PURGE_TXCLEAR, PurgeComm,
    SetCommState, SetCommTimeouts,
};
use windows_sys::Win32::Foundation::{
    CloseHandle, GENERIC_READ, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Storage::FileSystem::{CreateFileW, OPEN_EXISTING, ReadFile, WriteFile};

/// The control link baud (§6).
const CTRL_BAUD: u32 = 4_000_000;

/// `fBinary` — bit 0 of the DCB control bitfield. Must be 1 for a valid DCB.
const DCB_F_BINARY: u32 = 0x0000_0001;

/// The DCB `_bitfield`: `fBinary = 1`, every other control flag `0` (see module docs). Pure/testable.
fn dcb_bitfield() -> u32 {
    DCB_F_BINARY
}

/// Build the configured `DCB` for the 4 Mbaud raw 8-N-1 link. Pure (no I/O) so it is unit-testable.
fn configure_dcb(mut dcb: DCB) -> DCB {
    dcb.DCBlength = core::mem::size_of::<DCB>() as u32;
    dcb.BaudRate = CTRL_BAUD;
    dcb.ByteSize = 8;
    dcb.Parity = NOPARITY;
    dcb.StopBits = ONESTOPBIT;
    dcb._bitfield = dcb_bitfield();
    // Irrelevant with flow control off, but zeroed for determinism.
    dcb.XonLim = 0;
    dcb.XoffLim = 0;
    dcb
}

/// Build the `COMMTIMEOUTS` for a flat 100 ms bounded read: `ReadFile` returns whatever arrived
/// (possibly 0) within the window, so [`Transport::read`] yields `Ok(0)` on idle and the reader
/// thread can poll its stop flag. The nonzero `ReadTotalTimeoutConstant` is load-bearing: do NOT
/// "simplify" it to zero — `MAXDWORD/0/0` makes an idle `ReadFile` return instantly, spinning the
/// reader thread at 100% CPU.
fn read_timeouts() -> COMMTIMEOUTS {
    COMMTIMEOUTS {
        ReadIntervalTimeout: u32::MAX,
        ReadTotalTimeoutMultiplier: 0,
        ReadTotalTimeoutConstant: 100,
        WriteTotalTimeoutMultiplier: 0,
        WriteTotalTimeoutConstant: 0,
    }
}

/// Encode a port path as the NUL-terminated `\\.\COMn` wide string `CreateFileW` expects. A bare
/// `COMn` is prefixed with `\\.\` (needed for `COM10`+); an already-prefixed path passes through.
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

/// An owned, configured serial HANDLE, closed on drop.
pub(crate) struct WindowsSerial {
    handle: HANDLE,
}

// SAFETY: the HANDLE is an owned OS file handle, never aliased; the OS permits concurrent
// ReadFile/WriteFile on it from multiple threads.
unsafe impl Send for WindowsSerial {}
// SAFETY: see the Send impl.
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

        // SAFETY: `wide` is a valid NUL-terminated UTF-16 string for the call; null security/template
        // handle are optional, exclusive open (no sharing), OPEN_EXISTING. Result checked below.
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
        serial.flush_input()?;
        Ok(serial)
    }

    /// Discard buffered RX/TX bytes once at open, so a stale buffer (ROM-bootloader preamble, leftover
    /// frame bytes) cannot precede and mis-frame the first real reply. The decoder resyncs on SOF
    /// regardless; this just removes a connect-handshake flake source.
    fn flush_input(&self) -> io::Result<()> {
        // SAFETY: `self.handle` is a valid open serial handle; PurgeComm only clears its queues.
        let ok = unsafe { PurgeComm(self.handle, PURGE_RXCLEAR | PURGE_TXCLEAR) };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    /// Read the current `DCB`, apply our configuration, and write it back; then set read timeouts.
    fn configure(&self) -> io::Result<()> {
        // SAFETY: zeroed DCB is a valid initial value; GetCommState overwrites it.
        let mut dcb: DCB = unsafe { core::mem::zeroed() };
        dcb.DCBlength = core::mem::size_of::<DCB>() as u32;

        // SAFETY: `self.handle` is valid; `&mut dcb` is a live, correctly sized DCB the call fills.
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
            // SAFETY: `self.handle` is valid; pointer + count stay within `buf`, `&mut n` is a live
            // out param, no OVERLAPPED (synchronous write).
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
        // SAFETY: `self.handle` is valid; pointer + count stay within `buf`, `&mut n` is a live out
        // param, no OVERLAPPED (synchronous read). On the COMMTIMEOUTS timeout this succeeds n == 0.
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
        // n == 0 is the read timeout: report Ok(0) so the reader thread can poll its stop flag.
        Ok(n as usize)
    }
}

impl Drop for WindowsSerial {
    fn drop(&mut self) {
        // SAFETY: `self.handle` is an owned valid handle, closed exactly once. DTR/RTS were disabled,
        // so closing does not pulse-reset the chip.
        unsafe {
            CloseHandle(self.handle);
        }
    }
}
