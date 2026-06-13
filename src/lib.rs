//! # medius
//!
//! Host control library for the **medius** transparent mouse passthrough box.
//!
//! The compiled control plane for a box whose firmware the project owns: speaks the framed binary
//! control protocol over the device-chip USB-serial link, exposing the firmware's command primitives
//! 1:1 (the production replacement for the C reference client).
//!
//! It does **not** smooth, humanize, pace, or synthesize mouse behaviour — each method binds one
//! firmware frame, and the firmware owns additive carry-remainder injection and descriptor-faithful
//! clamping. The caller drives the timing of its own MOVE stream.
//!
//! See `OVERVIEW.md` for the current library overview and
//! `docs/protocol/control-protocol.md` (firmware repo) for the byte-exact wire reference.
//!
//! ## Feature flags
//!
//! - `async` — a thin `AsyncDevice` wrapper over the same sync core.
//! - `mock` — a public scriptable fake box for hardware-free tests.
//! - `flash` — `esptool` reboot + flash handoff.
//! - `tracing` — library-side instrumentation.

// The library is unsafe-free (serial I/O goes through the `serialport` crate) — enforce it.
#![forbid(unsafe_code)]
#![warn(missing_debug_implementations)]

// `#[macro_use]` first so `trace_event!` / `trace_span!` are in scope crate-wide (macro_rules is textual).
#[macro_use]
mod trace;

mod device;
mod error;
pub(crate) mod protocol;
mod transport;
pub mod types;

#[cfg(feature = "async")]
mod asyncv;
#[cfg(feature = "flash")]
pub mod flash;
#[cfg(feature = "mock")]
mod mock;

#[cfg(test)]
mod tests;

pub use device::logs::LogStream;
pub use device::{DEFAULT_KEEPALIVE_CADENCE, DEFAULT_QUERY_TIMEOUT, Device};
pub use error::{Error, Result};
// Frame-inspection types the public `mock` surface exposes (the wire codec stays crate-private).
pub use protocol::{DecodedFrame, FrameType};
pub use transport::scan::find_medius;
// The public value vocabulary (also browsable as `medius::types::*`).
pub use types::{
    Button, ButtonAction, CountersSnapshot, Health, LogLevel, LogLine, PortInfo, RebootTarget,
    Version,
};

#[cfg(feature = "async")]
pub use asyncv::AsyncDevice;
#[cfg(feature = "mock")]
pub use mock::MockBox;
