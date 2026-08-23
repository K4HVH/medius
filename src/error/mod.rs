//! The crate-wide structured error type.

use crate::protocol::FrameError;
use crate::types::CatchClass;

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

    #[error("the box holds at most {limit} catch entries and this subscription needs {needed}")]
    CatchTableFull { needed: usize, limit: usize },

    #[error("a catch subscription needs at least one filter")]
    EmptySubscription,

    #[error("{class:?} arrives decoded and carries no packet, so a capture on it does nothing")]
    CaptureNotApplicable { class: CatchClass },

    #[error("{class:?} is traffic and cannot be decoded to an input edge; use catch_events")]
    NotAnInputFilter { class: CatchClass },

    #[error(
        "CatchFilter::everything() covers traffic as well as input; use CatchFilter::all_input()"
    )]
    WildcardNotInput,

    #[error(
        "{direction:?} is measured against the bearing at emit time, which a {what} is addressed \
         before; use Both, Positive or Negative"
    )]
    RelativeDirection {
        direction: crate::types::Direction,
        what: &'static str,
    },

    #[error(
        "id 0x{id:04X} is the blanket sentinel on the wire, so an exact {class:?} subscription to it \
         would address the whole class instead"
    )]
    ReservedId { class: CatchClass, id: u16 },

    #[error(
        "an input subscription must cover both edges: without the release edge a fresh press cannot \
         be told from a chord, and without the opposite sign an axis never returns to rest. Drop the \
         direction and match on Input::Press or the sign of the delta"
    )]
    HalfEdgeInputFilter,

    /// The box refused a firmware update op (§4.16). `arg` is that status's argument: the slot size
    /// for `TOO_BIG`, the chunk it expected for `SEQ_GAP`, an `esp_err_t` for a write or image failure.
    #[error("update {op} refused: {status} (arg {arg})")]
    Update {
        op: u8,
        status: crate::types::UpdateStatus,
        arg: u32,
    },
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
