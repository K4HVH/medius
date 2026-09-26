//! Crate-wide error type.

use crate::protocol::FrameError;
use crate::types::CatchClass;

/// Crate-wide error type.
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

    #[error("query timed out")]
    QueryTimeout,

    #[error("device disconnected")]
    Disconnected,

    #[error("frame payload too long (max {max} bytes)", max = crate::protocol::MAX_PAYLOAD)]
    FrameTooLong,

    #[error(
        "lock scale {scale} is outside {min} to {max} (percent of the physical value kept: 0 blocks, \
         100 passes, above amplifies, negative reverses)"
    )]
    LockScaleRange { scale: i16, min: i16, max: i16 },

    #[error(
        "lock scale {scale} reverses, but class {class} carries one bit with nothing to reverse; use \
         0 to block or 100 to pass"
    )]
    LockScaleUsage { scale: i16, class: u8 },

    #[error("subscription needs {needed} catch entries; the box holds {limit}")]
    CatchTableFull { needed: usize, limit: usize },

    #[error("a catch subscription needs at least one filter")]
    EmptySubscription,

    #[error(
        "advanced control (§3.14) needs the imperfect-clone opt-in, which the box reports off; call \
         allow_imperfect_clones(true) first"
    )]
    ImperfectRequired,

    #[error("rewrite match and mask differ in length (match {match_len}, mask {mask_len})")]
    RewriteMaskLength { match_len: usize, mask_len: usize },

    #[error("rewrite match has {len} bytes; at most {limit} are compared")]
    RewriteMatchTooLong { len: usize, limit: usize },

    #[error("all {limit} rewrite rules are in use; remove one first")]
    RewriteTableFull { limit: usize },

    #[error(
        "rewrite payload of {len} bytes exceeds the {free} bytes left in the {limit}-byte pool; \
         remove a rule or shorten a payload first"
    )]
    RewritePoolFull {
        len: usize,
        free: usize,
        limit: usize,
    },

    #[error("{action:?} is not valid on a {class:?} rewrite rule")]
    RewriteActionClass {
        action: crate::types::RewriteAction,
        class: crate::types::RewriteClass,
    },

    #[error(
        "{action:?} rewrite payload of {len} bytes at offset {offset} exceeds the {cap}-byte head \
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
        "{op:?} transform cannot address {src:?} → {dst:?}: swap takes two axes; remap takes \
         axis→axis, button→button, button→key or button→media; neither takes one field as both \
         ends. To weigh a field, use scale"
    )]
    TransformOpFields {
        op: crate::types::TransformOp,
        src: crate::types::LockTarget,
        dst: crate::types::LockTarget,
    },

    #[error(
        "{limit} transforms already held for this device; remove one first. The count is host-side \
         and can include an entry the box refused for naming a field the clone does not declare; \
         query_transforms reports what the box has"
    )]
    TransformTableFull { limit: usize },

    #[error("{class:?} arrives decoded with no packet, so a capture on it does nothing")]
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
        "raw injection needs one endpoint flow, Direction::IN or Direction::OUT; {direction:?} is \
         neither"
    )]
    RawDirection { direction: crate::types::Direction },

    #[error("clip frame has {count} {what}, over the limit of {limit}")]
    ClipFrameCount {
        what: &'static str,
        count: usize,
        limit: usize,
    },

    #[error(
        "clip frame encodes to {len} bytes, over the {max} one append carries; split it across frames",
        max = crate::types::CLIP_ENTRY_MAX
    )]
    ClipFrameTooLong { len: usize },

    #[error(
        "clip transfer needs {want} data bytes and has {got}: wLength for an OUT request, none for \
         an IN one"
    )]
    ClipTransferData { want: usize, got: usize },

    #[error("a clip packet trigger {reason}")]
    ClipPacketTrigger { reason: &'static str },

    #[error(
        "id 0x{id:04X} is the wire's blanket sentinel; an exact {class:?} subscription to it would \
         address the whole class"
    )]
    ReservedId { class: CatchClass, id: u16 },

    #[error(
        "input subscription must cover both edges: without the release a fresh press looks like a \
         chord, and without the opposite sign an axis never returns to rest. Drop the direction and \
         match on Input::Press or the delta's sign"
    )]
    HalfEdgeInputFilter,

    /// Firmware update op refused (§4.16). `arg`: the slot size for `TOO_BIG`, the expected chunk for
    /// `SEQ_GAP`, an `esp_err_t` for a write or image failure.
    #[error("{} failed: {}", crate::types::update_doing(*op), crate::types::update_reason(*op, *status, *arg))]
    Update {
        op: u8,
        status: crate::types::UpdateStatus,
        arg: u32,
    },
}

/// Crate-wide [`Result`](core::result::Result) alias.
pub type Result<T> = core::result::Result<T, Error>;

impl From<FrameError> for Error {
    fn from(err: FrameError) -> Self {
        match err {
            FrameError::PayloadTooLong { .. } => Error::FrameTooLong,
        }
    }
}
