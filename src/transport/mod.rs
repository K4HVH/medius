use std::io;
use std::time::Duration;

pub(crate) mod mock;
pub(crate) mod scan;
pub(crate) mod serial;

pub(crate) trait Transport: Send + Sync + std::fmt::Debug {
    fn write_all(&self, buf: &[u8]) -> io::Result<()>;

    // Waits a bounded time for data: `Ok(0)` is none yet and the reader retries at once; `Err` is a
    // port that is gone.
    fn read(&self, buf: &mut [u8]) -> io::Result<usize>;

    // Makes a parked read, or else the next one, return `Ok(0)` at once.
    fn wake_read(&self) {}

    // Ends anything a read left pending, before the reading thread exits or hands the port on.
    fn release_read(&self) {}
}

const IDLE_READ_WAIT: Duration = Duration::from_millis(2);

#[derive(Debug)]
pub(crate) struct Disconnected;

impl Transport for Disconnected {
    fn write_all(&self, _buf: &[u8]) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::NotConnected,
            "transport disconnected (reconnecting)",
        ))
    }

    fn read(&self, _buf: &mut [u8]) -> io::Result<usize> {
        std::thread::sleep(IDLE_READ_WAIT);
        Ok(0)
    }
}
