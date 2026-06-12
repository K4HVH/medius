//! # medius
//!
//! Host control library for the **medius** transparent mouse passthrough box.
//!
//! `medius` is the compiled control plane for a box whose firmware the project owns. It speaks the
//! framed binary control protocol over the device-chip USB-serial link and sustains **1 kHz** MOVE
//! injection — the production replacement for `smooth_inject.c`.
//!
//! It is a transparent, precise control + injection layer. It does **not** smooth, humanize, or
//! synthesize fake mouse behaviour; the firmware guarantees additive carry-remainder injection and
//! descriptor-faithful clamping. The library's only "shaping" job is *pacing the frame stream*.
//!
//! See `docs/superpowers/specs/2026-06-13-medius-rust-library-design.md` in the firmware repo for
//! the full design, and `docs/protocol/control-protocol.md` for the byte-exact wire reference.
//!
//! ## Feature flags
//!
//! - `async` — a thin `AsyncDevice` wrapper over the same sync core.
//! - `mock` — a public scriptable fake box for hardware-free tests.
//! - `metrics` — pacer jitter / latency histograms.
//! - `flash` — `esptool` reboot + flash handoff.
//! - `cli` — the `medius` operator/validation binary.
//! - `tracing` — library-side instrumentation.
//! - `serde` — derives on the public value types.

// Transport needs `unsafe` for platform FFI; require it to be explicitly scoped.
#![forbid(unsafe_op_in_unsafe_fn)]
#![warn(missing_debug_implementations)]
// On docs.rs (which sets `--cfg docsrs`), enable the nightly `doc_auto_cfg` feature so every
// feature-gated item renders a "this is supported on feature X" badge. Gated on `docsrs`, so stable
// builds (CI, downstream) never see the nightly feature.
#![cfg_attr(docsrs, feature(doc_auto_cfg))]

// Declared FIRST and `#[macro_use]` so its `trace_event!`/`trace_span!` macros are in scope for every
// module below (Rust macro_rules visibility is textual/top-down). It expands to `tracing::…` with the
// `tracing` feature on and to nothing when off, so call sites stay clean and the default build has no
// tracing dependency (Task 5.2).
#[macro_use]
mod trace; // M5 — internal tracing macro shim

pub mod protocol; // M1 — pure wire layer

mod config; // M5 — ConnectOptions config surface (serde)
mod device; // M3 — Device core, reader/keepalive threads, commands/queries/logs/reconcile
mod error; // M2 — structured Error enum
pub mod pacer; // M4 — paced MovementSession + precise tick clock
mod transport; // M2 — private serial transport (+ mock)

#[cfg(feature = "async")]
pub mod asyncv; // M5 — thin AsyncDevice wrapper over the same core

#[cfg(feature = "flash")]
pub mod flash; // M5 — reboot-to-download + esptool orchestration

#[cfg(feature = "mock")]
pub mod mock; // M5 — public scriptable fake box

pub use config::ConnectOptions;

#[cfg(feature = "async")]
pub use asyncv::AsyncDevice;

pub use device::{CountersSnapshot, Device};
pub use error::{Error, Result};
#[cfg(feature = "mock")]
pub use mock::MockBox;
pub use pacer::{DEFAULT_RATE_HZ, MovementSession};
pub use protocol::types::{Button, ButtonAction, Health, LogLevel, LogLine, RebootTarget, Version};
pub use transport::scan::{CH343_PID, PortInfo, WCH_VID, find_medius, find_ports};

#[cfg(feature = "metrics")]
pub use pacer::metrics::{HistogramSnapshot, PacerStats};
