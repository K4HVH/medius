//! The medius wire protocol: pure, no-I/O codec.

pub mod command;
pub mod crc;
pub mod frame;
pub mod opcode;
pub mod response;

pub use frame::{DecodedFrame, FrameDecoder, FrameError, encode};
pub use opcode::{FrameType, MAX_PAYLOAD, PROTO_VER, RST_F_NVS};
pub use response::{Resp, parse_log, parse_resp};
