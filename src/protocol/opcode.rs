//! Frame opcodes and wire constants, pinned to `ctrl_proto.h` / `control-protocol.md`.

use core::fmt;

/// Start-of-frame byte; the receiver resyncs by scanning for it (§2).
pub const SOF: u8 = 0xA5;

/// Maximum payload length (§2); a larger `LEN` is rejected as bogus.
pub const MAX_PAYLOAD: usize = 512;

/// Protocol version in `RESP(VERSION)` (§4.1); the handshake requires this exact value.
pub const PROTO_VER: u8 = 2; // v2: the unified-input-core redesign (generic INJECT/MOVE/LOCK, class-aware RATE)

/// `INJECT` class byte: the momentary-usage field kind.
pub const INJ_BTN: u8 = 0;
pub const INJ_KEY: u8 = 1;
pub const INJ_MEDIA: u8 = 2;
/// `MOVE` motion byte: the relative-axis field kind.
pub const INJ_MOTION_CURSOR: u8 = 0;
pub const INJ_MOTION_WHEEL: u8 = 1;

pub const Q_VERSION: u8 = 0;
pub const Q_HEALTH: u8 = 1;
/// Cloned mouse identity: vid/pid/bcd + serial/bos flags (§4.3).
pub const Q_MOUSE_INFO: u8 = 2;
/// Unified device capabilities: mouse (buttons/axes/ifaces) + keyboard (keys/NKRO/media/system) +
/// per-class change_driven. One query describes the whole cloned device (§4.4).
pub const Q_CAPS: u8 = 3;
/// Live native report rate + clone poll period + confidence (§4.5).
pub const Q_RATE: u8 = 4;
/// Delivery/telemetry counters (§4.6).
pub const Q_STATS: u8 = 5;
/// Active lock bitmask (§4.8, v1.5.0).
pub const Q_LOCKS: u8 = 6;
/// Active catch subscription mask + dropped-event count (§4.9, v1.6.0).
pub const Q_CATCH: u8 = 7;
// selector 8 retired (was Q_KBD_CAPS; keyboard caps folded into the unified Q_CAPS = 3).
/// Persistent box options, read one at a time by id: `QUERY [Q_OPTIONS][id]` → `RESP [Q_OPTIONS][id][value..]` (§4.14).
pub const Q_OPTIONS: u8 = 9;

/// `OPTION` id: imperfect-clone opt-in. Set value `[allow u8]`; readback adds `over_capacity`/`clone_imperfect`.
pub const OPT_IMPERFECT: u8 = 0;
/// `OPTION` id: movement riding. Value `[timeout u16 LE ms]` — 0 = off, N = ride window in milliseconds.
pub const OPT_MOVE_RIDE: u8 = 1;
/// `OPTION` id: emit-rate pacing. Value `[mode u8][rate_hz u16 LE]` — 0 learnt / 1 bInterval / 2 fixed.
pub const OPT_EMIT: u8 = 2;

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
/// At least one lock is active (§4.2, v1.5.0).
pub const H_LOCK_ON: u8 = 0x20;
/// A catch subscription is active — physical-input events are streaming (§4.2, v1.6.0).
pub const H_CATCH_ON: u8 = 0x40;
/// A keyboard is attached on the host chip — cloned and injectable (§4.2, v2.0.0).
pub const H_KBD_ATT: u8 = 0x80;

/// `CATCH` mask: stream reports whose X or Y delta is non-zero (§3.9).
pub const CATCH_MOTION: u8 = 0x01;
/// `CATCH` mask: stream reports whose wheel delta is non-zero (§3.9).
pub const CATCH_WHEEL: u8 = 0x02;
/// `CATCH` mask: stream reports with a button edge (§3.9).
pub const CATCH_BUTTONS: u8 = 0x04;
/// `CATCH` mask: stream keyboard + media changes (`KB_EVENT` / `CONS_EVENT`, v2.0.0).
pub const CATCH_KEYS: u8 = 0x08;
/// `CATCH` mask: every class (§3.9).
pub const CATCH_ALL: u8 = 0x0F;
/// Valid `CATCH` mask bits; the firmware ignores any others (§3.9).
pub const CATCH_MASK: u8 = 0x0F;

/// `CAPS` kbd_flags: keys are an NKRO bitmap (`n_keys` = 0xFF), else a keycode array (§4.4).
pub const KBC_NKRO: u8 = 0x01;
/// `CAPS` kbd_flags: a Consumer (media-key) collection is present and injectable/catchable.
pub const KBC_CONSUMER: u8 = 0x02;
/// `CAPS` kbd_flags: a System-control collection is present (passthrough-only, not injectable).
pub const KBC_SYSTEM: u8 = 0x04;
/// `CAPS` kbd_flags: the keyboard report sits behind a HID report ID.
pub const KBC_REPORT_ID: u8 = 0x08;

/// `CAPS` change_driven flag: the mouse class is change-driven (never set — mouse motion is continuous).
pub const CAPS_CD_MOUSE: u8 = 0x01;
/// `CAPS` change_driven flag: the keyboard/media class is change-driven (set when a keyboard is bound).
pub const CAPS_CD_KBD: u8 = 0x02;

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
/// `RATE` flag: the active input is change-driven (keyboard/media) — no continuous cadence, poll floor only.
pub const RATE_CHANGE_DRIVEN: u8 = 0x02;

pub const LOG_ERROR: u8 = 0;
pub const LOG_WARN: u8 = 1;
pub const LOG_INFO: u8 = 2;
pub const LOG_DEBUG: u8 = 3;
pub const LOG_VERBOSE: u8 = 4;

/// A frame opcode (the `TYPE` byte, §3 / §4).
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FrameType {
    /// `MOVE` — relative-axis movement, motion-tagged (cursor dx/dy or wheel dz) (PC→box).
    Move = 0x01,
    /// `INJECT` — set a momentary-usage override (button/key/media), class-tagged (PC→box).
    Inject = 0x03,
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
    /// `LOCK` — lock/unlock an axis or button edge (PC→box).
    Lock = 0x0A,
    /// `CATCH` — subscribe to the physical-input event stream (PC→box).
    Catch = 0x0B,
    /// `MOUSE_EVENT` — one unsolicited mouse snapshot; `SEQ` is a rolling counter (box→PC).
    MouseEvent = 0x0C,
    /// `KB_EVENT` — one unsolicited keyboard snapshot (modifiers + pressed keys); box→PC (v2.0.0).
    KbEvent = 0x0F,
    /// `CONS_EVENT` — one unsolicited media snapshot (active Consumer usages); box→PC (v2.0.0).
    ConsEvent = 0x10,
    /// `OPTION` — set a persistent box option by id (imperfect-clone opt-in, movement riding) (PC→box).
    Option = 0x11,
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
            0x03 => FrameType::Inject,
            0x04 => FrameType::Reset,
            0x05 => FrameType::Query,
            0x06 => FrameType::Resp,
            0x07 => FrameType::RebootDl,
            0x08 => FrameType::Log,
            0x09 => FrameType::Led,
            0x0A => FrameType::Lock,
            0x0B => FrameType::Catch,
            0x0C => FrameType::MouseEvent,
            0x0F => FrameType::KbEvent,
            0x10 => FrameType::ConsEvent,
            0x11 => FrameType::Option,
            other => return Err(UnknownFrameType(other)),
        })
    }
}

impl From<FrameType> for u8 {
    fn from(t: FrameType) -> u8 {
        t as u8
    }
}
