//! Serial transport over an overlapped Win32 COM handle.
//!
//! Windows serialises every request queued against a synchronous file object, and duplicating its
//! handle does not escape that: a reader parked in `ReadFile` owns the port for the whole read
//! timeout and any concurrent `WriteFile` waits behind it, so injection only lands in the gaps
//! between reads. `serial2` opens the port with `FILE_FLAG_OVERLAPPED` and completes each request
//! through its own `OVERLAPPED`, which leaves a read and a write in flight at the same time.

use std::io;
use std::path::Path;

use serial2::{SerialPort, Settings};

use super::{CTRL_BAUD, IO_TIMEOUT};
use crate::transport::Transport;

// fAbortOnError. The driver fails every transfer after a comm error until ClearCommError, which
// nothing here calls, so a single overrun at 4 Mbaud would wedge the link until it reconnected.
const DCB_ABORT_ON_ERROR: u32 = 1 << 14;

#[derive(Debug)]
pub(crate) struct SerialTransport {
    port: SerialPort,
}

impl SerialTransport {
    pub(crate) fn open(path: &Path) -> io::Result<Self> {
        // serial2 always prepends the win32 device namespace, so it wants the bare `COM7`.
        let path = path.to_string_lossy();
        let name = path
            .strip_prefix(r"\\.\")
            .or_else(|| path.strip_prefix(r"\\?\"))
            .unwrap_or(&path);
        let mut port = SerialPort::open(name, |mut settings: Settings| {
            settings.set_raw();
            settings.set_baud_rate(CTRL_BAUD)?;
            settings.as_raw_dbc_mut()._bitfield &= !DCB_ABORT_ON_ERROR;
            Ok(settings)
        })?;
        port.set_read_timeout(IO_TIMEOUT)?;
        port.set_write_timeout(IO_TIMEOUT)?;
        let _ = port.discard_input_buffer();
        Ok(SerialTransport { port })
    }
}

impl Transport for SerialTransport {
    fn write_all(&self, buf: &[u8]) -> io::Result<()> {
        self.port.write_all(buf)
    }

    fn read(&self, buf: &mut [u8]) -> io::Result<usize> {
        match self.port.read(buf) {
            // serial2 reports a vanished port as EOF, which the reader has to see as an error to
            // start reconnecting; an idle port comes back as a timeout instead.
            Ok(0) => Err(io::Error::from(io::ErrorKind::BrokenPipe)),
            Ok(n) => Ok(n),
            Err(e) if e.kind() == io::ErrorKind::TimedOut => Ok(0),
            Err(e) => Err(e),
        }
    }
}
