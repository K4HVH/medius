//! The medius wire protocol — pure, no-I/O codec.
//!
//! This module is the whole wire layer: frame encoding/decoding, opcodes and constants, value
//! types, typed command payload encoders, and response decoders. It performs **no** I/O, spawns no
//! threads, uses no `unsafe`, and is deterministic and panic-free on any malformed, truncated, or
//! oversized input — the foundation the transport and device layers build on.
//!
//! The byte-exact reference is `docs/protocol/control-protocol.md` (the source of truth);
//! `firmware/device/components/inject/ctrl_proto.h` is the authoritative constants header. Every
//! numeric constant here is pinned to those by [`opcode::tests::opcodes_match_firmware`].

pub mod crc;
pub mod frame;
pub mod opcode;
pub mod types;

pub use frame::{DecodedFrame, FrameDecoder, FrameError, encode};
pub use opcode::{FrameType, MAX_PAYLOAD, PROTO_VER, SOF, UnknownFrameType};
pub use types::{Button, ButtonAction, Health, LogLevel, LogLine, RebootTarget, Version};
