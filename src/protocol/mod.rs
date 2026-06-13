//! The medius wire protocol — pure, no-I/O codec.
//!
//! The whole wire layer: frame encode/decode, opcodes and constants, value types, command payload
//! encoders, and response decoders. No I/O, no threads, no `unsafe`, and panic-free on any
//! malformed, truncated, or oversized input.
//!
//! Source of truth: `control-protocol.md` (byte-exact reference) and `ctrl_proto.h` (constants).
//! Constants are pinned by the `opcode` module's `opcodes_match_firmware` /
//! `opcodes_match_ctrl_proto_header` tests.

pub mod command;
pub mod crc;
pub mod frame;
pub mod opcode;
pub mod response;
pub mod types;

pub use frame::{DecodedFrame, FrameDecoder, FrameError, encode};
pub use opcode::{FrameType, MAX_PAYLOAD, PROTO_VER};
pub use response::{Resp, parse_log, parse_resp};
pub use types::{Button, ButtonAction, Health, LogLevel, LogLine, RebootTarget, Version};
