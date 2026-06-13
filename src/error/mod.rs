//! The crate-wide structured error type.

use crate::protocol::FrameError;

/// The crate-wide error type.
#[derive(thiserror::Error, Debug)]
#[non_exhaustive]
pub enum Error {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("no medius device found")]
    NotFound,

    #[error("no reply to version query during handshake")]
    NoReply,

    #[error("unsupported protocol version {got} (expected {expected})", expected = crate::protocol::PROTO_VER)]
    BadProtoVer { got: u8 },

    #[error("query timed out waiting for a response")]
    QueryTimeout,

    #[error("device disconnected")]
    Disconnected,

    #[error("frame payload too long (max {max} bytes)", max = crate::protocol::MAX_PAYLOAD)]
    FrameTooLong,

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
