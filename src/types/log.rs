//! Device `LOG` frame value types — severity level and the decoded line.

use crate::protocol::opcode::{LOG_DEBUG, LOG_ERROR, LOG_INFO, LOG_VERBOSE, LOG_WARN};

/// A device `LOG` frame severity level (§4.3).
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
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

    /// Map a wire `level` byte to a [`LogLevel`]; unknown levels fall back to `Info` (matching
    /// `medius.py`) so a forward-compat level never panics or loses the log text.
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

/// A decoded `LOG` frame (§4.3): a severity level and its UTF-8 text.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct LogLine {
    pub level: LogLevel,
    /// Decoded lossily from UTF-8; not NUL-terminated on the wire.
    pub text: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_level_from_u8() {
        assert_eq!(LogLevel::from_u8(0), LogLevel::Error);
        assert_eq!(LogLevel::from_u8(1), LogLevel::Warn);
        assert_eq!(LogLevel::from_u8(2), LogLevel::Info);
        assert_eq!(LogLevel::from_u8(3), LogLevel::Debug);
        assert_eq!(LogLevel::from_u8(4), LogLevel::Verbose);
        assert_eq!(LogLevel::from_u8(5), LogLevel::Info);
        assert_eq!(LogLevel::from_u8(255), LogLevel::Info);
    }
}
