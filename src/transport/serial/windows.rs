//! Serial transport over an overlapped Win32 COM handle.
//!
//! Windows serialises every request on a synchronous file object, duplicated handles included: a
//! reader parked in `ReadFile` holds the port for the read timeout and a concurrent `WriteFile`
//! waits, so injection lands only between reads. `serial2` opens the port with
//! `FILE_FLAG_OVERLAPPED` and completes each request through its own `OVERLAPPED`, so a read and a
//! write are in flight at once.

use std::io;
use std::path::Path;

use serial2::os::windows::CommTimeouts;
use serial2::{SerialPort, Settings};

use super::{CTRL_BAUD, IO_TIMEOUT};
use crate::transport::Transport;

// fAbortOnError: the driver fails every transfer after a comm error until ClearCommError, which
// nothing here calls, so one overrun at 6 Mbaud would wedge the link until a reconnect.
const DCB_ABORT_ON_ERROR: u32 = 1 << 14;

#[derive(Debug)]
pub(crate) struct SerialTransport {
    port: SerialPort,
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
        // set_read_timeout arms the COMMTIMEOUTS combination that makes the WCH CH343 driver
        // schedule the read-timeout DPC it bugchecks in; a zero read constant (non-blocking) never
        // arms it, and the reader already polls on a 0-byte read.
        port.set_windows_timeouts(&CommTimeouts {
            read_interval_timeout: u32::MAX,
            read_total_timeout_multiplier: 0,
            read_total_timeout_constant: 0,
            write_total_timeout_multiplier: 0,
            write_total_timeout_constant: IO_TIMEOUT.as_millis().try_into().unwrap_or(u32::MAX),
        })?;
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
            // serial2 reports a vanished port as EOF, which must reach the reader as an error to
            // start a reconnect; an idle port returns a timeout.
            Ok(0) => Err(io::Error::from(io::ErrorKind::BrokenPipe)),
            Ok(n) => Ok(n),
            Err(e) if e.kind() == io::ErrorKind::TimedOut => Ok(0),
            Err(e) => Err(e),
        }
    }
}
