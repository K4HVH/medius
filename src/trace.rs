//! Internal `tracing` shim (Task 5.2) — feature-gated instrumentation with clean call sites.
//!
//! The crate instruments connect/query/transport/pacer/error paths, but the default build must carry
//! **no** `tracing` dependency and pay **no** runtime cost. Rather than scatter `#[cfg(feature =
//! "tracing")]` through every call site, this module defines two `macro_rules!` macros —
//! [`trace_event!`] and [`trace_span!`] — that expand to `tracing::event!` / `tracing::span!` when the
//! `tracing` feature is on and to **nothing** (a no-op) when it is off. The module is `#[macro_use]`d
//! first in `lib.rs`, so the macros are in scope crate-wide.
//!
//! ## Hot-path discipline (Medius-specific, §10)
//!
//! The pacer must **never** trace per tick — per-tick events would perturb the 1 kHz path. The pacer
//! loop therefore accumulates and emits only a **~1/sec aggregate** at DEBUG (`target:
//! "medius::pacer"`); see `pacer/mod.rs`. Per-frame TX/RX events exist only at TRACE (`target:
//! "medius::transport"`) and are documented as timing-perturbing. This shim provides the mechanism;
//! the placement (aggregate vs per-tick) is the caller's responsibility and is enforced by a test.

/// Emit a tracing event when the `tracing` feature is on; expand to nothing otherwise.
///
/// Mirrors `tracing::event!`'s syntax (`trace_event!(target: "…", LEVEL, field = value, "message")`),
/// so call sites read exactly like a `tracing` call. With the feature off the whole invocation —
/// including any field expressions — is dropped, so it is genuinely zero-cost (the field expressions
/// are not even evaluated). Call sites that *need* a side effect must not put it in a trace field.
#[cfg(feature = "tracing")]
macro_rules! trace_event {
    ($($arg:tt)*) => {
        ::tracing::event!($($arg)*)
    };
}

/// No-op form: the feature is off, so the event (and its field expressions) vanish entirely.
#[cfg(not(feature = "tracing"))]
macro_rules! trace_event {
    ($($arg:tt)*) => {{}};
}

/// Open a tracing span when the `tracing` feature is on; expand to a unit `()` otherwise.
///
/// Mirrors `tracing::span!`. With the feature off it yields `()`, so `let _g = trace_span!(…).entered()`
/// still type-checks — the no-op [`SpanStub`] below provides a matching `.entered()`.
#[cfg(feature = "tracing")]
macro_rules! trace_span {
    ($($arg:tt)*) => {
        ::tracing::span!($($arg)*)
    };
}

/// No-op form: yields a [`SpanStub`] whose `.entered()` is also a no-op.
#[cfg(not(feature = "tracing"))]
macro_rules! trace_span {
    ($($arg:tt)*) => {
        $crate::trace::SpanStub
    };
}

/// A zero-sized stand-in for a `tracing::Span` when the feature is off, so `trace_span!(…).entered()`
/// compiles feature-free. All methods are no-ops the optimizer removes.
#[cfg(not(feature = "tracing"))]
pub(crate) struct SpanStub;

#[cfg(not(feature = "tracing"))]
impl SpanStub {
    /// No-op stand-in for `Span::entered`; returns another stub guard.
    pub(crate) fn entered(self) -> SpanStub {
        SpanStub
    }
}

/// Re-emit one decoded device `LOG` line as a host tracing event at the mapped level, under
/// `target: "medius::device"` (§10). The line still goes on the `logs()` channel regardless; this is
/// the *additional* tracing surface. A no-op without the feature (the reader does not call it then).
///
/// `tracing`'s `event!` needs a *compile-time constant* level, so this dispatches over the five levels
/// rather than passing a runtime `Level` (which `event!` does not accept).
#[cfg(feature = "tracing")]
pub(crate) fn emit_device_log(line: &crate::protocol::types::LogLine) {
    use crate::protocol::types::LogLevel;
    let text = line.text.as_str();
    match line.level {
        LogLevel::Error => {
            trace_event!(target: "medius::device", ::tracing::Level::ERROR, device_log = true, "{text}")
        }
        LogLevel::Warn => {
            trace_event!(target: "medius::device", ::tracing::Level::WARN, device_log = true, "{text}")
        }
        LogLevel::Info => {
            trace_event!(target: "medius::device", ::tracing::Level::INFO, device_log = true, "{text}")
        }
        LogLevel::Debug => {
            trace_event!(target: "medius::device", ::tracing::Level::DEBUG, device_log = true, "{text}")
        }
        LogLevel::Verbose => {
            trace_event!(target: "medius::device", ::tracing::Level::TRACE, device_log = true, "{text}")
        }
    }
}

#[cfg(all(test, feature = "tracing"))]
mod tests {
    use crate::protocol::types::{LogLevel, LogLine};

    /// `emit_device_log` runs without panicking for every level (the level → tracing mapping is
    /// exercised inside it). A capturing-subscriber assertion of the re-emit lives in
    /// `device::tests::tracing_capture`.
    #[test]
    fn emit_device_log_handles_every_level() {
        for level in [
            LogLevel::Error,
            LogLevel::Warn,
            LogLevel::Info,
            LogLevel::Debug,
            LogLevel::Verbose,
        ] {
            super::emit_device_log(&LogLine {
                level,
                text: "x".into(),
            });
        }
    }
}
