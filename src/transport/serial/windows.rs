//! Serial transport over an overlapped Win32 COM handle.
//!
//! Windows serialises every request on a synchronous file object, duplicated handles included: a
//! reader parked in `ReadFile` holds the port for the read timeout and a concurrent `WriteFile`
//! waits, so injection lands only between reads. `serial2` opens the port with
//! `FILE_FLAG_OVERLAPPED` and completes each request through its own `OVERLAPPED`, so a read and a
//! write are in flight at once.

use std::io;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::Path;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::ThreadId;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serial2::os::windows::CommTimeouts;
use serial2::{SerialPort, Settings};
use windows_sys::Win32::Devices::Communication::{EV_RXCHAR, SetCommMask, WaitCommEvent};
use windows_sys::Win32::Foundation::{
    ERROR_IO_PENDING, ERROR_OPERATION_ABORTED, HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows_sys::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};
use windows_sys::Win32::System::Threading::{CreateEventW, WaitForSingleObject};

use super::{CTRL_BAUD, IO_TIMEOUT};
use crate::transport::Transport;

// fAbortOnError: the driver fails every transfer after a comm error until ClearCommError, which
// nothing here calls, so one overrun at 6 Mbaud would wedge the link until a reconnect.
const DCB_ABORT_ON_ERROR: u32 = 1 << 14;

const RETIRE_WAIT: Duration = Duration::from_millis(20);

#[derive(Debug)]
pub(crate) struct SerialTransport {
    port: SerialPort,
    rx: Mutex<RxWait>,
    woken: AtomicBool,
}

impl SerialTransport {
    pub(crate) fn open(path: &Path) -> io::Result<Self> {
        // serial2 prepends the win32 device namespace itself, so it takes the bare `COM7`.
        let path = path.to_string_lossy();
        let name = path
            .strip_prefix(r"\\.\")
            .or_else(|| path.strip_prefix(r"\\?\"))
            .unwrap_or(&path);
        let port = SerialPort::open(name, |mut settings: Settings| {
            settings.set_raw();
            settings.set_baud_rate(CTRL_BAUD)?;
            settings.as_raw_dbc_mut()._bitfield &= !DCB_ABORT_ON_ERROR;
            Ok(settings)
        })?;
        // The WCH CH343 driver bugchecks in the DPC it arms for a total read timeout, so reads
        // return at once and a comm event waits for data.
        port.set_windows_timeouts(&CommTimeouts {
            read_interval_timeout: u32::MAX,
            read_total_timeout_multiplier: 0,
            read_total_timeout_constant: 0,
            write_total_timeout_multiplier: 0,
            write_total_timeout_constant: IO_TIMEOUT.as_millis().try_into().unwrap_or(u32::MAX),
        })?;
        let rx = RxWait::new(&port)?;
        let _ = port.discard_input_buffer();
        Ok(SerialTransport {
            port,
            rx: Mutex::new(rx),
            woken: AtomicBool::new(false),
        })
    }
}

impl Drop for SerialTransport {
    fn drop(&mut self) {
        let _ = self.rx.get_mut().retire(&self.port);
    }
}

impl Transport for SerialTransport {
    fn write_all(&self, buf: &[u8]) -> io::Result<()> {
        self.port.write_all(buf)
    }

    fn read(&self, buf: &mut [u8]) -> io::Result<usize> {
        let mut rx = self.rx.lock();
        let deadline = Instant::now() + IO_TIMEOUT;
        let mut empty_wakes = 0;
        loop {
            // Armed before the drain, so a byte landing after an empty read still fires it.
            let fired = rx.arm(&self.port)?;
            match self.port.read(buf) {
                // serial2 reports a vanished port as EOF, which must reach the reader as an error
                // to start a reconnect; an empty port returns a timeout.
                Ok(0) => return Err(io::Error::from(io::ErrorKind::BrokenPipe)),
                Ok(n) => return Ok(n),
                Err(e) if e.kind() == io::ErrorKind::TimedOut => {}
                Err(e) => return Err(e),
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() || self.woken.swap(false, Ordering::AcqRel) {
                return Ok(0);
            }
            if !fired && !rx.wait(&self.port, left)? {
                return Ok(0);
            }
            // A wait takes at most two empty wakes to clear the history of bytes already read; a
            // driver that keeps waking with nothing to read is paced instead of spun on.
            empty_wakes += 1;
            if empty_wakes > 2 {
                std::thread::sleep(Duration::from_millis(1));
            }
        }
    }

    fn wake_read(&self) {
        self.woken.store(true, Ordering::Release);
        RxWait::interrupt(&self.port);
    }

    fn release_read(&self) {
        let _ = self.rx.lock().retire(&self.port);
    }
}

// Windows cancels a thread's pending I/O when the thread exits, so a wait is retired, not handed to
// another thread, and retired before its own thread leaves.
struct RxWait {
    event: OwnedHandle,
    op: NonNull<WaitOp>,
    owner: Option<ThreadId>,
}

#[derive(Default)]
struct WaitOp {
    overlapped: OVERLAPPED,
    mask: u32,
}

impl std::fmt::Debug for RxWait {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RxWait")
            .field("owner", &self.owner)
            .finish_non_exhaustive()
    }
}

// SAFETY: `op` is heap memory only this struct reaches, the event handle is valid on any thread,
// and every access goes through the transport's mutex or `&mut self`.
#[allow(unsafe_code)]
unsafe impl Send for RxWait {}

#[allow(unsafe_code)]
impl RxWait {
    fn new(port: &SerialPort) -> io::Result<Self> {
        // SAFETY: plain Win32 calls; a non-null event handle is owned by nothing else.
        let event = unsafe {
            let raw = CreateEventW(std::ptr::null(), 1, 0, std::ptr::null());
            if raw.is_null() {
                return Err(io::Error::last_os_error());
            }
            OwnedHandle::from_raw_handle(raw)
        };
        // SAFETY: the port's handle is open for the call.
        if unsafe { SetCommMask(handle(port), EV_RXCHAR) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(RxWait {
            event,
            op: NonNull::from(Box::leak(Box::default())),
            owner: None,
        })
    }

    fn arm(&mut self, port: &SerialPort) -> io::Result<bool> {
        let me = std::thread::current().id();
        match self.owner {
            Some(owner) if owner == me => return Ok(false),
            Some(_) => self.retire(port)?,
            None => {}
        }
        let op = self.op.as_ptr();
        // SAFETY: no request is pending on `op`, and a pending one keeps it allocated (see `Drop`).
        let done = unsafe {
            op.write(WaitOp::default());
            (*op).overlapped.hEvent = self.event.as_raw_handle();
            WaitCommEvent(handle(port), &raw mut (*op).mask, &raw mut (*op).overlapped)
        };
        if done != 0 {
            return Ok(true);
        }
        let err = io::Error::last_os_error();
        if err.raw_os_error() != Some(ERROR_IO_PENDING as i32) {
            return Err(err);
        }
        self.owner = Some(me);
        Ok(false)
    }

    fn wait(&mut self, port: &SerialPort, timeout: Duration) -> io::Result<bool> {
        if !self.signalled(timeout)? {
            return Ok(false);
        }
        self.owner = None;
        let mut n = 0;
        // SAFETY: the event is set, so the request on `op` has completed.
        let ok = unsafe {
            GetOverlappedResult(
                handle(port),
                &raw const (*self.op.as_ptr()).overlapped,
                &mut n,
                0,
            )
        };
        let err = io::Error::last_os_error();
        // A wait cancelled by its thread's exit is reissued by the next arm.
        if ok == 0 && err.raw_os_error() != Some(ERROR_OPERATION_ABORTED as i32) {
            return Err(err);
        }
        Ok(true)
    }

    fn signalled(&self, timeout: Duration) -> io::Result<bool> {
        let ms = timeout
            .as_millis()
            .max(1)
            .try_into()
            .unwrap_or(u32::MAX - 1);
        // SAFETY: the event handle is owned by `self`.
        match unsafe { WaitForSingleObject(self.event.as_raw_handle(), ms) } {
            WAIT_OBJECT_0 => Ok(true),
            WAIT_TIMEOUT => Ok(false),
            _ => Err(io::Error::last_os_error()),
        }
    }

    // Setting the mask completes a pending wait, from any thread and without the reader's lock.
    fn interrupt(port: &SerialPort) {
        // SAFETY: the port's handle is open for the call.
        unsafe { SetCommMask(handle(port), EV_RXCHAR) };
    }

    // A cancel is only the fallback: WCH's cancel routine races its own completion path.
    fn retire(&mut self, port: &SerialPort) -> io::Result<()> {
        if self.owner.is_none() {
            return Ok(());
        }
        Self::interrupt(port);
        if !self.signalled(RETIRE_WAIT)? {
            // SAFETY: the port's handle is open and `op` is the pending request's own.
            unsafe { CancelIoEx(handle(port), &raw const (*self.op.as_ptr()).overlapped) };
            if !self.signalled(RETIRE_WAIT)? {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "comm event wait did not end",
                ));
            }
        }
        self.owner = None;
        Ok(())
    }
}

impl Drop for RxWait {
    fn drop(&mut self) {
        // A wait that never ended still has the kernel writing `op`, so it is leaked.
        if self.owner.is_none() {
            #[allow(unsafe_code)]
            // SAFETY: `new` allocated `op` and no request is pending on it.
            drop(unsafe { Box::from_raw(self.op.as_ptr()) });
        }
    }
}

fn handle(port: &SerialPort) -> HANDLE {
    port.as_raw_handle()
}
