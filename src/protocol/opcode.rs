//! Frame opcodes and wire constants, pinned to `ctrl_proto.h` / `control-protocol.md`.
//!
//! Two `tests` guards back these up: `opcodes_match_firmware` (Rust-vs-Rust, catches an in-crate
//! edit) and `opcodes_match_ctrl_proto_header` (the real drift guard — parses the firmware header
//! when reachable, skips otherwise).

use core::fmt;

/// Start-of-frame byte; the receiver resyncs by scanning for it (§2).
pub const SOF: u8 = 0xA5;

/// Maximum payload length (§2); a larger `LEN` is rejected as bogus.
pub const MAX_PAYLOAD: usize = 512;

/// Protocol version in `RESP(VERSION)` (§4.1); the handshake requires this exact value.
pub const PROTO_VER: u8 = 1;

// ---- QUERY selectors (§3.5 / ctrl_proto.h `CTRL_Q_*`) ----

pub const Q_VERSION: u8 = 0;
pub const Q_HEALTH: u8 = 1;

// ---- BUTTON ids (§3.3 / ctrl_proto.h `CTRL_BTN_*`) ----

pub const BTN_LEFT: u8 = 0;
pub const BTN_RIGHT: u8 = 1;
pub const BTN_MIDDLE: u8 = 2;
pub const BTN_SIDE1: u8 = 3;
pub const BTN_SIDE2: u8 = 4;
pub const BTN_COUNT: u8 = 5;

// ---- BUTTON actions (§3.3 / ctrl_proto.h `CTRL_ACT_*`) ----

/// Clear our injected press; defer to physical state.
pub const ACT_SOFTREL: u8 = 0;
/// Force the button down regardless of physical state.
pub const ACT_PRESS: u8 = 1;
/// Force the button up, masking a physical hold.
pub const ACT_FORCEREL: u8 = 2;

// ---- HEALTH flag bits (§4.2 / ctrl_proto.h `CTRL_H_*`) ----

/// Inter-chip link to the host chip is up.
pub const H_LINK_UP: u8 = 0x01;
/// A real mouse is attached on the host chip.
pub const H_MOUSE_ATT: u8 = 0x02;
/// The clone has been configured by the game PC.
pub const H_CLONE_CFG: u8 = 0x04;
/// Injection is currently active.
pub const H_INJECT_ON: u8 = 0x08;

// ---- LOG levels (§4.3 / ctrl_proto.h `CTRL_LOG_*`) ----

pub const LOG_ERROR: u8 = 0;
pub const LOG_WARN: u8 = 1;
pub const LOG_INFO: u8 = 2;
pub const LOG_DEBUG: u8 = 3;
pub const LOG_VERBOSE: u8 = 4;

/// A frame opcode (the `TYPE` byte, §3 / §4).
///
/// `try_from` returns `Err` for an unknown byte so the decoder can consume-and-ignore it (§2
/// forward-compat).
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FrameType {
    /// `MOVE` — relative cursor movement (PC→box).
    Move = 0x01,
    /// `WHEEL` — vertical scroll (PC→box).
    Wheel = 0x02,
    /// `BUTTON` — set a button injection override (PC→box).
    Button = 0x03,
    /// `RESET` — clear all injection (PC→box).
    Reset = 0x04,
    /// `QUERY` — request a state snapshot, elicits `RESP` (PC→box).
    Query = 0x05,
    /// `RESP` — reply to a `QUERY`, `SEQ` echoes the request (box→PC).
    Resp = 0x06,
    /// `REBOOT_DL` — reboot a chip to ROM download or to run (PC→box).
    RebootDl = 0x07,
    /// `LOG` — unsolicited device diagnostics (box→PC).
    Log = 0x08,
}

/// Error returned when a byte does not name a known [`FrameType`]; the decoder ignores the frame (§2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnknownFrameType(pub u8);

impl fmt::Display for UnknownFrameType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown frame type 0x{:02X}", self.0)
    }
}

impl core::error::Error for UnknownFrameType {}

impl TryFrom<u8> for FrameType {
    type Error = UnknownFrameType;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        Ok(match value {
            0x01 => FrameType::Move,
            0x02 => FrameType::Wheel,
            0x03 => FrameType::Button,
            0x04 => FrameType::Reset,
            0x05 => FrameType::Query,
            0x06 => FrameType::Resp,
            0x07 => FrameType::RebootDl,
            0x08 => FrameType::Log,
            other => return Err(UnknownFrameType(other)),
        })
    }
}

impl From<FrameType> for u8 {
    fn from(t: FrameType) -> u8 {
        t as u8
    }
}
