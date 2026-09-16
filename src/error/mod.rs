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

    #[error(
        "the advanced control layer (§3.14) is gated on the imperfect-clone opt-in, which the box reports \
         off; call allow_imperfect_clones(true) first"
    )]
    ImperfectRequired,

    #[error(
        "a rewrite rule's match and mask must be the same length (match {match_len}, mask {mask_len})"
    )]
    RewriteMaskLength { match_len: usize, mask_len: usize },

    #[error("the box holds {limit} rewrite rules and they are all in use; remove one first")]
    RewriteTableFull { limit: usize },

    #[error("{action:?} is not a valid action for a {class:?} rewrite rule")]
    RewriteActionClass {
        action: crate::types::RewriteAction,
        class: crate::types::RewriteClass,
    },

    #[error(
        "a {action:?} rewrite payload of {len} bytes at offset {offset} exceeds the {cap}-byte head \
         the box holds for a {class:?} rule"
    )]
    RewritePayloadTooLarge {
        action: crate::types::RewriteAction,
        class: crate::types::RewriteClass,
        len: usize,
        offset: usize,
        cap: usize,
    },

    #[error(
        "a {op:?} transform cannot address {src:?} → {dst:?}: a scale is one axis, a swap is two \
         axes, and a remap is axis→axis, button→button, button→key or button→media"
    )]
    TransformOpFields {
        op: crate::types::TransformOp,
        src: crate::types::LockTarget,
        dst: crate::types::LockTarget,
    },

    #[error(
        "a transform scale is a percent bounded by ±{max}, which is the widest the box applies; \
         {scale} would be silently weighed at that bound instead"
    )]
    TransformScaleRange { scale: i16, max: i16 },

    #[error(
        "a {src:?} source carries one bit, so a transform on it takes only a full pass ({pass}); \
         {scale} names a percentage a single bit cannot hold"
    )]
    TransformUsageScale {
        src: crate::types::LockTarget,
        scale: i16,
        pass: i16,
    },

    #[error(
        "{limit} transforms are already held for this device; remove one first. This counts what the \
         host holds, which can include an entry the box refused for naming a field the clone does not \
         declare: query_transforms reports what the box actually has"
    )]
    TransformTableFull { limit: usize },

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
        "a raw injection puts bytes on one cloned endpoint flow, so it needs Direction::IN or \
         Direction::OUT; {direction:?} names neither"
    )]
    RawDirection { direction: crate::types::Direction },

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
    #[error("{} failed: {}", crate::types::update_doing(*op), crate::types::update_reason(*op, *status, *arg))]
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
