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

pub mod protocol; // M1 — pure wire layer

// Modules are added per implementation milestone:
//   mod error;          // M2
//   mod transport;      // M2
//   mod device;         // M3
//   pub mod pacer;      // M4
