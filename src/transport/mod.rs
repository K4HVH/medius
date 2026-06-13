use std::io;

pub(crate) mod mock;
pub(crate) mod scan;
pub(crate) mod serial;

pub(crate) trait Transport: Send + Sync + std::fmt::Debug {
    fn write_all(&self, buf: &[u8]) -> io::Result<()>;

    fn read(&self, buf: &mut [u8]) -> io::Result<usize>;
}

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
        Ok(0)
    }
}
