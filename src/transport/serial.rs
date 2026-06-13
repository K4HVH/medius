//! Serial transport via the `serialport` crate — cross-platform, no `unsafe`.
//!
//! Replaces the old raw libc/`termios2` (Linux) and `DCB` (Windows) FFI. `serialport` handles the
//! platform specifics and arbitrary baud (including the box's 4 Mbaud), and a normal open does not
//! reset the box (verified on hardware). Read and write use independent cloned handles to the same
//! port, so the reader thread's blocking read never stalls a writer.

use std::io::{self, Read, Write};
use std::path::Path;
use std::time::Duration;

use parking_lot::Mutex;
use serialport::SerialPort;

use super::Transport;

/// The control link baud (§6).
const CTRL_BAUD: u32 = 4_000_000;

/// Read timeout: an idle `read` returns `Ok(0)` ~10×/s so the reader thread can poll its stop flag.
const READ_TIMEOUT: Duration = Duration::from_millis(100);

/// A serial connection to the box, with separate read/write handles to the same port.
pub(crate) struct SerialTransport {
    read: Mutex<Box<dyn SerialPort>>,
    write: Mutex<Box<dyn SerialPort>>,
}

impl std::fmt::Debug for SerialTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SerialTransport").finish_non_exhaustive()
    }
}

impl SerialTransport {
    /// Open and configure `path` (e.g. `/dev/ttyACM0` / `COM7`) for the 4 Mbaud raw control link.
    pub(crate) fn open(path: &Path) -> io::Result<Self> {
        let read = open_handle(&path.to_string_lossy())?;
        // Discard any stale RX (boot preamble / leftover frame bytes) before the handshake.
        let _ = read.clear(serialport::ClearBuffer::Input);
        let write = read.try_clone().map_err(to_io)?;
        Ok(SerialTransport {
            read: Mutex::new(read),
            write: Mutex::new(write),
        })
    }
}

/// Open one handle to `path`. On Unix we clear the exclusive lock so `reconnect` can reopen the same
/// path while the old handle is still being torn down (the handshake is the real client gate); Windows
/// COM ports are exclusive at the OS level regardless, which is fine since a real reconnect's old port
/// is already gone.
fn open_handle(path: &str) -> io::Result<Box<dyn SerialPort>> {
    let builder = serialport::new(path, CTRL_BAUD).timeout(READ_TIMEOUT);
    #[cfg(unix)]
    {
        let mut port = serialport::TTYPort::open(&builder).map_err(to_io)?;
        port.set_exclusive(false).map_err(to_io)?;
        Ok(Box::new(port))
    }
    #[cfg(not(unix))]
    {
        builder.open().map_err(to_io)
    }
}

/// Map a `serialport::Error` to `io::Error`, preserving the I/O kind where there is one.
fn to_io(e: serialport::Error) -> io::Error {
    match e.kind() {
        serialport::ErrorKind::Io(kind) => io::Error::new(kind, e),
        serialport::ErrorKind::NoDevice => io::Error::new(io::ErrorKind::NotFound, e),
        _ => io::Error::other(e),
    }
}

impl Transport for SerialTransport {
    fn write_all(&self, buf: &[u8]) -> io::Result<()> {
        self.write.lock().write_all(buf)
    }

    fn read(&self, buf: &mut [u8]) -> io::Result<usize> {
        match self.read.lock().read(buf) {
            Ok(n) => Ok(n),
            // serialport reports an idle timeout as `TimedOut`; map to `Ok(0)` so the reader polls `stop`.
            Err(e) if e.kind() == io::ErrorKind::TimedOut => Ok(0),
            Err(e) => Err(e),
        }
    }
}
