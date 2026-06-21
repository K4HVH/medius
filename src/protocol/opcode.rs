//! Frame opcodes and wire constants, pinned to `ctrl_proto.h` / `control-protocol.md`.

use core::fmt;

/// Start-of-frame byte; the receiver resyncs by scanning for it (§2).
pub const SOF: u8 = 0xA5;

/// Maximum payload length (§2); a larger `LEN` is rejected as bogus.
pub const MAX_PAYLOAD: usize = 512;

/// Protocol version in `RESP(VERSION)` (§4.1); the handshake requires this exact value.
pub const PROTO_VER: u8 = 1;

pub const Q_VERSION: u8 = 0;
pub const Q_HEALTH: u8 = 1;
/// Cloned mouse identity: vid/pid/bcd + serial/bos flags (§4.3).
pub const Q_MOUSE_INFO: u8 = 2;
/// Semantic capabilities: button count, axes, interface count (§4.4).
pub const Q_CAPS: u8 = 3;
/// Live native report rate + clone poll period + confidence (§4.5).
pub const Q_RATE: u8 = 4;
/// Delivery/telemetry counters (§4.6).
pub const Q_STATS: u8 = 5;

pub const BTN_LEFT: u8 = 0;
pub const BTN_RIGHT: u8 = 1;
pub const BTN_MIDDLE: u8 = 2;
pub const BTN_SIDE1: u8 = 3;
pub const BTN_SIDE2: u8 = 4;
pub const BTN_COUNT: u8 = 5;

/// Clear our injected press; defer to physical state.
pub const ACT_SOFTREL: u8 = 0;
/// Force the button down regardless of physical state.
pub const ACT_PRESS: u8 = 1;
/// Force the button up, masking a physical hold.
pub const ACT_FORCEREL: u8 = 2;

/// Inter-chip link to the host chip is up.
pub const H_LINK_UP: u8 = 0x01;
/// A real mouse is attached on the host chip.
pub const H_MOUSE_ATT: u8 = 0x02;
/// The clone has been configured by the game PC.
pub const H_CLONE_CFG: u8 = 0x04;
/// Injection is currently active.
pub const H_INJECT_ON: u8 = 0x08;
/// The native-rate estimator window is full, so the `RATE` value is trustworthy (§4.2, v1.4.0).
pub const H_RATE_CONFIDENT: u8 = 0x10;

/// `MOUSE_INFO` flag: the clone serves a serial string (§4.3).
pub const MI_HAS_SERIAL: u8 = 0x01;
/// `MOUSE_INFO` flag: the clone serves a BOS descriptor (§4.3).
pub const MI_HAS_BOS: u8 = 0x02;

/// `CAPS` axis flag: relative X present (§4.4).
pub const CAP_X: u8 = 0x01;
/// `CAPS` axis flag: relative Y present (§4.4).
pub const CAP_Y: u8 = 0x02;
/// `CAPS` axis flag: wheel present (§4.4).
pub const CAP_WHEEL: u8 = 0x04;
/// `CAPS` axis flag: the mouse report sits behind a HID report ID (§4.4).
pub const CAP_REPORT_ID: u8 = 0x08;

/// `RATE` flag: estimator window full (same source as [`H_RATE_CONFIDENT`], §4.5).
pub const RATE_CONFIDENT: u8 = 0x01;

pub const LOG_ERROR: u8 = 0;
pub const LOG_WARN: u8 = 1;
pub const LOG_INFO: u8 = 2;
pub const LOG_DEBUG: u8 = 3;
pub const LOG_VERBOSE: u8 = 4;

/// A frame opcode (the `TYPE` byte, §3 / §4).
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
    /// `LED` — status LED override (PC→box).
    Led = 0x09,
}

/// Error returned when a byte does not name a known [`FrameType`].
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
            0x09 => FrameType::Led,
            other => return Err(UnknownFrameType(other)),
        })
    }
}

impl From<FrameType> for u8 {
    fn from(t: FrameType) -> u8 {
        t as u8
    }
}
