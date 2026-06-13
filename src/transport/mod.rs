//! The private transport layer (§6 of the design spec).
//!
//! A small [`Transport`] trait over the byte pipe to the box: a cross-platform [`serial`] connection
//! (the `serialport` crate) and an in-memory [`mock`] for tests. Port discovery lives in [`scan`].
//!
//! Methods take `&self` (trait is `Send + Sync`) so one `Arc<dyn Transport>` can be shared between the
//! device reader thread and the mutex-serialized writers. The real impl holds independent read/write
//! handles to the same port so concurrent read+write don't contend; the mock uses interior mutability.

use std::io;

pub(crate) mod mock;
pub(crate) mod scan;
pub(crate) mod serial;

/// A byte pipe to the box: blocking writes, timeout-bounded reads.
pub(crate) trait Transport: Send + Sync + std::fmt::Debug {
    /// Write all of `buf` (one full frame), blocking over partial writes until flushed, or error.
    fn write_all(&self, buf: &[u8]) -> io::Result<()>;

    /// Read available bytes into `buf`. `Ok(0)` is the read timeout (not an error) — it lets the
    /// reader thread wake to poll its stop flag; `Ok(n>0)` read `n` bytes; `Err` is I/O / disconnect.
    fn read(&self, buf: &mut [u8]) -> io::Result<usize>;
}

/// A no-op transport swapped in briefly during [`reconnect`](crate::Device::reconnect): it lets the
/// real port (held exclusively by `serialport`) be dropped and closed before the new one is opened.
/// Reads time out (`Ok(0)`) so the reader idles; writes report a disconnect.
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
