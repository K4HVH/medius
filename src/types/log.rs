//! Device `LOG` frame value types — severity level and the decoded line.

use crate::protocol::opcode::{LOG_DEBUG, LOG_ERROR, LOG_INFO, LOG_VERBOSE, LOG_WARN};

/// A device `LOG` frame severity level (§4.7).
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
    Verbose,
}

impl LogLevel {
    /// The wire `level` byte for this level.
    pub fn as_u8(self) -> u8 {
        match self {
            LogLevel::Error => LOG_ERROR,
            LogLevel::Warn => LOG_WARN,
            LogLevel::Info => LOG_INFO,
            LogLevel::Debug => LOG_DEBUG,
            LogLevel::Verbose => LOG_VERBOSE,
        }
    }

    /// Map a wire `level` byte to a [`LogLevel`]; unknown levels fall back to `Info`.
    pub fn from_u8(v: u8) -> Self {
        match v {
            LOG_ERROR => LogLevel::Error,
            LOG_WARN => LogLevel::Warn,
            LOG_INFO => LogLevel::Info,
            LOG_DEBUG => LogLevel::Debug,
            LOG_VERBOSE => LogLevel::Verbose,
            _ => LogLevel::Info,
        }
    }
}

/// A decoded `LOG` frame (§4.7): a severity level and its UTF-8 text.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LogLine {
    pub level: LogLevel,
    /// Decoded lossily from UTF-8; not NUL-terminated on the wire.
    pub text: String,
}
