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
//! See `docs/superpowers/specs/2026-06-13-medius-rust-library-design.md` for the full design and
//! `docs/protocol/control-protocol.md` for the byte-exact wire reference.
//!
//! ## Feature flags
//!
//! - `async` — a thin `AsyncDevice` wrapper over the same sync core.
//! - `mock` — a public scriptable fake box for hardware-free tests.
//! - `flash` — `esptool` reboot + flash handoff.
//! - `tracing` — library-side instrumentation.
//! - `serde` — derives on the public value types.

// Transport needs `unsafe` for platform FFI; require it to be explicitly scoped.
#![forbid(unsafe_op_in_unsafe_fn)]
#![warn(missing_debug_implementations)]
// docs.rs sets `--cfg docsrs`; gate the nightly feature-cfg badge feature on it so stable builds never see it.
#![cfg_attr(docsrs, feature(doc_auto_cfg))]

// First + `#[macro_use]` so `trace_event!`/`trace_span!` are in scope crate-wide (macro_rules is textual).
#[macro_use]
mod trace;

pub(crate) mod protocol;

mod config;
mod device;
mod error;
mod transport;

#[cfg(feature = "async")]
pub mod asyncv;

#[cfg(feature = "flash")]
pub mod flash;

#[cfg(feature = "mock")]
pub mod mock;

pub use config::ConnectOptions;

#[cfg(feature = "async")]
pub use asyncv::AsyncDevice;

pub use device::logs::LogStream;
pub use device::{CountersSnapshot, Device};
pub use error::{Error, Result};
#[cfg(feature = "mock")]
pub use mock::MockBox;
// Frame-inspection types the public `mock` surface exposes (the wire codec stays crate-private).
pub use protocol::{
    Button, ButtonAction, DecodedFrame, FrameType, Health, LogLevel, LogLine, RebootTarget, Version,
};
pub use transport::scan::{PortInfo, find_medius};
