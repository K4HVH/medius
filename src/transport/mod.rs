//! The private transport layer (§6 of the design spec).
//!
//! A small [`Transport`] trait over the byte pipe to the box: raw [`linux`] serial, raw [`windows`]
//! serial, and an in-memory [`mock`] for tests. Port discovery lives in [`scan`].
//!
//! Methods take `&self` (trait is `Send + Sync`) so one `Arc<dyn Transport>` can be shared between
//! the device reader thread and the mutex-serialized writers; concurrent read+write on one serial
//! fd/HANDLE is OS-safe, so the real impls just hold the descriptor. The mock uses interior
//! mutability to satisfy `&self`.
//!
//! Raw platform serial (not an off-the-shelf crate) is forced by two hardware facts: DTR/RTS must be
//! deasserted *before* the line is used (asserting either resets the device chip, so an
//! open-then-configure crate would pulse-reset the box), and 4 Mbaud is non-standard and needs
//! OS-specific custom-baud syscalls (`termios2`/`BOTHER` on Linux, custom `DCB.BaudRate` on Windows).

use std::io;

pub(crate) mod mock;
pub(crate) mod scan;

#[cfg(target_os = "linux")]
pub(crate) mod linux;

#[cfg(windows)]
pub(crate) mod windows;

/// A byte pipe to the box: blocking writes, timeout-bounded reads. The DTR/RTS dance happens once at
/// open inside each impl's constructor, so it is not part of this trait.
pub(crate) trait Transport: Send + Sync + std::fmt::Debug {
    /// Write all of `buf` (one full frame), blocking over partial writes until flushed, or error.
    fn write_all(&self, buf: &[u8]) -> io::Result<()>;

    /// Read available bytes into `buf`. `Ok(0)` is the read timeout (not an error) — it lets the
    /// reader thread wake to poll its stop flag; `Ok(n>0)` read `n` bytes; `Err` is I/O / disconnect.
    fn read(&self, buf: &mut [u8]) -> io::Result<usize>;
}
