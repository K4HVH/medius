//! Internal `tracing` shim: feature-gated instrumentation macros.

#[cfg(feature = "tracing")]
macro_rules! trace_event {
    ($($arg:tt)*) => {
        ::tracing::event!($($arg)*)
    };
}

#[cfg(not(feature = "tracing"))]
macro_rules! trace_event {
    ($($arg:tt)*) => {{}};
}

#[cfg(feature = "tracing")]
macro_rules! trace_span {
    ($($arg:tt)*) => {
        ::tracing::span!($($arg)*)
    };
}

#[cfg(not(feature = "tracing"))]
macro_rules! trace_span {
    ($($arg:tt)*) => {
        $crate::trace::SpanStub
    };
}

#[cfg(not(feature = "tracing"))]
pub(crate) struct SpanStub;

#[cfg(not(feature = "tracing"))]
impl SpanStub {
    pub(crate) fn entered(self) -> SpanStub {
        SpanStub
    }
}

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
