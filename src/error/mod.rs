//! The crate-wide structured error type (§8 of the design spec).
//!
//! Fully structured, no stringly-typed catch-all: callers match on each variant. CRC failures are
//! deliberately absent — the decoder drops corrupt frames silently and only counts them, per the
//! firmware's "corrupt frames are never acted on" rule (§8).

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

    /// An outbound payload exceeded the maximum frame payload.
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
