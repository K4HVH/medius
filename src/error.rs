//! The crate-wide structured error type (§8 of the design spec).
//!
//! Fully structured, no stringly-typed catch-all: callers match on each variant. CRC failures are
//! deliberately absent — the decoder drops corrupt frames silently and only counts them
//! ([`crate::protocol::FrameDecoder::crc_error_count`]), per the firmware's "corrupt frames are never
//! acted on" rule (§8).

use crate::protocol::FrameError;

/// The crate-wide error type (§8).
#[derive(thiserror::Error, Debug)]
#[non_exhaustive]
pub enum Error {
    /// An underlying I/O / OS error from the transport (open, read, write, ioctl, …).
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    /// No serial port matched the medius VID/PID during discovery.
    #[error("no medius device found")]
    NotFound,

    /// Handshake: the box never answered the `QUERY(VERSION)` probe.
    #[error("no reply to version query during handshake")]
    NoReply,

    /// Handshake: the box answered, but with a protocol version this library does not speak.
    #[error("unsupported protocol version {got} (expected {expected})", expected = crate::protocol::PROTO_VER)]
    BadProtoVer {
        /// The protocol version the box reported.
        got: u8,
    },

    /// A `QUERY` did not receive its correlated `RESP` within the query timeout.
    #[error("query timed out waiting for a response")]
    QueryTimeout,

    /// The serial port vanished mid-session (device unplugged / re-enumerated).
    #[error("device disconnected")]
    Disconnected,

    /// An outbound payload exceeded the maximum frame payload ([`crate::protocol::MAX_PAYLOAD`]).
    #[error("frame payload too long (max {max} bytes)", max = crate::protocol::MAX_PAYLOAD)]
    FrameTooLong,

    /// The external flash tool (`esptool`) failed: a non-zero exit (with captured stderr) or a spawn
    /// failure. A spawn `io::Error` surfaces as [`Error::Io`]; this is the tool's own bad exit.
    #[cfg(feature = "flash")]
    #[error("flash tool failed: {0}")]
    FlashTool(String),
}

/// The crate-wide [`Result`](core::result::Result) alias.
pub type Result<T> = core::result::Result<T, Error>;

impl From<FrameError> for Error {
    fn from(err: FrameError) -> Self {
        match err {
            FrameError::PayloadTooLong { .. } => Error::FrameTooLong,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One representative value of every variant.
    fn one_of_each() -> Vec<Error> {
        vec![
            Error::Io(std::io::Error::other("boom")),
            Error::NotFound,
            Error::NoReply,
            Error::BadProtoVer { got: 7 },
            Error::QueryTimeout,
            Error::Disconnected,
            Error::FrameTooLong,
        ]
    }

    /// Every variant's `Display` string is non-empty.
    #[test]
    fn display_strings_non_empty() {
        for e in one_of_each() {
            assert!(!e.to_string().is_empty(), "empty Display for {e:?}");
        }
    }

    /// Every variant's `Display` string is distinct from the others (no accidental duplication).
    #[test]
    fn display_strings_distinct() {
        let errs = one_of_each();
        for i in 0..errs.len() {
            for j in (i + 1)..errs.len() {
                assert_ne!(
                    errs[i].to_string(),
                    errs[j].to_string(),
                    "Display collision between {:?} and {:?}",
                    errs[i],
                    errs[j],
                );
            }
        }
    }

    /// `BadProtoVer` reports the offending value and the expected one.
    #[test]
    fn bad_proto_ver_message_mentions_versions() {
        let msg = Error::BadProtoVer { got: 9 }.to_string();
        assert!(msg.contains('9'), "missing got: {msg}");
        assert!(
            msg.contains(&crate::protocol::PROTO_VER.to_string()),
            "missing expected: {msg}"
        );
    }

    /// A `FrameError` converts into `Error::FrameTooLong` via `From`/`?`.
    #[test]
    fn frame_error_maps_to_frame_too_long() {
        let fe = FrameError::PayloadTooLong { len: 9_999 };
        let err: Error = fe.into();
        assert!(matches!(err, Error::FrameTooLong));
    }

    /// An `std::io::Error` converts into `Error::Io` via `#[from]` (so `?` works in transport code).
    #[test]
    fn io_error_maps_via_from() {
        let io = std::io::Error::from(std::io::ErrorKind::TimedOut);
        let err: Error = io.into();
        assert!(matches!(err, Error::Io(_)));
    }
}
