//! The private transport layer (§6 of the design spec).
//!
//! A small [`Transport`] trait abstracts the byte pipe to the box, with three implementations:
//! the raw Linux serial port ([`linux`]), the raw Windows serial port ([`windows`]), and an
//! in-memory [`mock`] used by tests (and, later, exposed publicly under the `mock` feature as a
//! scriptable fake box). Port discovery lives in [`scan`].
//!
//! ## Why `&self` (not `&mut self`)
//!
//! The trait methods take `&self` and the trait is `Send + Sync`. In the device layer (Milestone 3)
//! a single `Arc<dyn Transport>` is shared between a blocking **reader thread** (which calls
//! [`Transport::read`]) and the **senders** (pacer tick / commands / queries, which call
//! [`Transport::write_all`] serialized by a device-side mutex). Concurrent read+write on one serial
//! fd / HANDLE is safe at the OS level, so the real impls just store the descriptor and issue
//! syscalls that need only the raw handle — no `&mut` required. The mock uses interior mutability
//! ([`parking_lot::Mutex`]) to satisfy `&self`.
//!
//! ## Why raw platform serial (not an off-the-shelf crate)
//!
//! Two hardware facts force it (both handled by `tools/medius.py`):
//!
//! 1. **DTR/RTS must be deasserted *before* the line is used** — asserting either resets the device
//!    chip. A crate that opens-then-configures would pulse-reset (and re-enumerate) the box on every
//!    connect.
//! 2. **4,000,000 baud is non-standard** and needs OS-specific custom-baud syscalls (`termios2` /
//!    `BOTHER` on Linux, a custom `DCB.BaudRate` on Windows).

// This whole layer is plumbing consumed by the device (M3) and pacer (M4) layers, plus the public
// scan re-exports (wired in M3). Until those land, the items here are only exercised by tests, so
// dead-code analysis flags them on the lib build; allow it crate-locally for the transport module.
#![allow(dead_code)]

use std::io;

pub(crate) mod mock;

#[cfg(target_os = "linux")]
pub(crate) mod linux;

#[cfg(windows)]
pub(crate) mod windows;

/// A byte pipe to the box: blocking writes, timeout-bounded reads.
///
/// The methods take `&self` so one `Arc<dyn Transport>` can be shared between the reader thread and
/// the (mutex-serialized) writers — see the [module docs](self#why-self-not-mut-self). The DTR/RTS
/// dance is performed once at open time inside each impl's constructor (asserting them resets the
/// device chip), so it is **not** part of this trait.
pub(crate) trait Transport: Send + Sync + std::fmt::Debug {
    /// Write all of `buf` (one full frame). Blocks until every byte is written, or errors.
    ///
    /// Implementations loop over partial writes until the whole buffer is flushed to the OS.
    fn write_all(&self, buf: &[u8]) -> io::Result<()>;

    /// Read available bytes into `buf`.
    ///
    /// Returns:
    /// - `Ok(0)` on the read **timeout** — no data arrived within the configured window. This is
    ///   *not* an error: it lets the device reader thread wake periodically to poll its stop flag.
    /// - `Ok(n)` with `n > 0` when `n` bytes were read.
    /// - `Err(_)` on a real I/O error or device disconnect.
    fn read(&self, buf: &mut [u8]) -> io::Result<usize>;
}
