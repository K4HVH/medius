//! Serial transport via the `serialport` crate.

use std::io::{self, Read, Write};
use std::path::Path;
use std::time::Duration;

use parking_lot::Mutex;
use serialport::SerialPort;

use super::Transport;

const CTRL_BAUD: u32 = 4_000_000;

const READ_TIMEOUT: Duration = Duration::from_millis(100);

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
    pub(crate) fn open(path: &Path) -> io::Result<Self> {
        let read = open_handle(&path.to_string_lossy())?;
        let _ = read.clear(serialport::ClearBuffer::Input);
        let write = read.try_clone().map_err(to_io)?;
        Ok(SerialTransport {
            read: Mutex::new(read),
            write: Mutex::new(write),
        })
    }
}

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
            Err(e) if e.kind() == io::ErrorKind::TimedOut => Ok(0),
            Err(e) => Err(e),
        }
    }
}
