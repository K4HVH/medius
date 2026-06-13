//! Internal `tracing` shim (Task 5.2) — feature-gated instrumentation with clean call sites.
//!
//! [`trace_event!`] / [`trace_span!`] expand to `tracing::event!` / `tracing::span!` with the
//! `tracing` feature on and to nothing when off, so the default build carries no `tracing` dependency
//! and no runtime cost without scattering `#[cfg]` through every call site. `#[macro_use]`d first in
//! `lib.rs` so they are in scope crate-wide.
//!
//! Hot-path discipline (§10): per-frame TX/RX is traced at TRACE only, so a caller's tight MOVE loop
//! is never perturbed by event work at higher levels. The shim is the mechanism; placement is the
//! call site's job.

/// Emit a tracing event when the `tracing` feature is on; expand to nothing otherwise.
///
/// Mirrors `tracing::event!`'s syntax. With the feature off the whole invocation — including field
/// expressions — is dropped, so a side effect must never live in a trace field.
#[cfg(feature = "tracing")]
macro_rules! trace_event {
    ($($arg:tt)*) => {
        ::tracing::event!($($arg)*)
    };
}

/// No-op form: the event and its field expressions vanish entirely.
#[cfg(not(feature = "tracing"))]
macro_rules! trace_event {
    ($($arg:tt)*) => {{}};
}

/// Open a tracing span when the `tracing` feature is on; expand to a no-op span stub otherwise.
///
/// Mirrors `tracing::span!`. With the feature off the stub's no-op `.entered()` keeps
/// `let _g = trace_span!(…).entered()` type-checking.
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

/// Zero-sized `tracing::Span` stand-in when the feature is off, so `trace_span!(…).entered()`
/// compiles feature-free.
#[cfg(not(feature = "tracing"))]
pub(crate) struct SpanStub;

#[cfg(not(feature = "tracing"))]
impl SpanStub {
    pub(crate) fn entered(self) -> SpanStub {
        SpanStub
    }
}

/// Re-emit one decoded device `LOG` line as a host tracing event at the mapped level, under
/// `target: "medius::device"` (§10). Additional to the `logs()` channel, which still gets the line.
///
/// `event!` needs a compile-time-constant level, so this dispatches over the five levels rather than
/// passing a runtime `Level`.
#[cfg(feature = "tracing")]
pub(crate) fn emit_device_log(line: &crate::types::LogLine) {
    use crate::types::LogLevel;
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
    use crate::types::{LogLevel, LogLine};

    /// `emit_device_log` runs without panicking for every level. The capturing-subscriber assertion
    /// of the re-emit lives in `device::tests::tracing_capture`.
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
