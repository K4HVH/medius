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

#[cfg(test)]
mod tests {
    use super::*;

    /// In-crate guard (Rust-vs-Rust): pins every value to its documented wire byte. Catches an
    /// in-crate edit; firmware drift is `opcodes_match_ctrl_proto_header`'s job.
    #[test]
    fn opcodes_match_firmware() {
        assert_eq!(FrameType::Move as u8, 0x01);
        assert_eq!(FrameType::Wheel as u8, 0x02);
        assert_eq!(FrameType::Button as u8, 0x03);
        assert_eq!(FrameType::Reset as u8, 0x04);
        assert_eq!(FrameType::Query as u8, 0x05);
        assert_eq!(FrameType::Resp as u8, 0x06);
        assert_eq!(FrameType::RebootDl as u8, 0x07);
        assert_eq!(FrameType::Log as u8, 0x08);

        assert_eq!(SOF, 0xA5);
        assert_eq!(MAX_PAYLOAD, 512);
        assert_eq!(PROTO_VER, 1);

        assert_eq!(Q_VERSION, 0);
        assert_eq!(Q_HEALTH, 1);

        assert_eq!(BTN_LEFT, 0);
        assert_eq!(BTN_RIGHT, 1);
        assert_eq!(BTN_MIDDLE, 2);
        assert_eq!(BTN_SIDE1, 3);
        assert_eq!(BTN_SIDE2, 4);
        assert_eq!(BTN_COUNT, 5);

        assert_eq!(ACT_SOFTREL, 0);
        assert_eq!(ACT_PRESS, 1);
        assert_eq!(ACT_FORCEREL, 2);

        assert_eq!(H_LINK_UP, 0x01);
        assert_eq!(H_MOUSE_ATT, 0x02);
        assert_eq!(H_CLONE_CFG, 0x04);
        assert_eq!(H_INJECT_ON, 0x08);

        assert_eq!(LOG_ERROR, 0);
        assert_eq!(LOG_WARN, 1);
        assert_eq!(LOG_INFO, 2);
        assert_eq!(LOG_DEBUG, 3);
        assert_eq!(LOG_VERBOSE, 4);
    }

    /// Known bytes round-trip; unknown bytes are rejected so the decoder ignores them (§2).
    #[test]
    fn frame_type_try_from() {
        for (byte, ty) in [
            (0x01, FrameType::Move),
            (0x02, FrameType::Wheel),
            (0x03, FrameType::Button),
            (0x04, FrameType::Reset),
            (0x05, FrameType::Query),
            (0x06, FrameType::Resp),
            (0x07, FrameType::RebootDl),
            (0x08, FrameType::Log),
        ] {
            assert_eq!(FrameType::try_from(byte), Ok(ty));
            assert_eq!(u8::from(ty), byte);
        }
        assert_eq!(FrameType::try_from(0x00), Err(UnknownFrameType(0x00)));
        assert_eq!(FrameType::try_from(0x09), Err(UnknownFrameType(0x09)));
        assert_eq!(FrameType::try_from(0xFF), Err(UnknownFrameType(0xFF)));
    }

    /// Best-effort firmware-drift guard: parses the real `ctrl_proto.h` `#define CTRL_* <num>` lines
    /// and asserts the Rust constants equal them. Header located via `MEDIUS_CTRL_PROTO_H`, else the
    /// sibling `../medius-fw/.../ctrl_proto.h`; absent (published-crate build) → skip and pass.
    #[test]
    fn opcodes_match_ctrl_proto_header() {
        use std::path::PathBuf;

        let path = match std::env::var_os("MEDIUS_CTRL_PROTO_H") {
            Some(p) => PathBuf::from(p),
            None => {
                let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
                p.push("../medius-fw/firmware/device/components/inject/ctrl_proto.h");
                p
            }
        };
        let Ok(src) = std::fs::read_to_string(&path) else {
            eprintln!(
                "opcodes_match_ctrl_proto_header: ctrl_proto.h not found at {} \
                 (set MEDIUS_CTRL_PROTO_H to enable the firmware drift check); skipping.",
                path.display()
            );
            return;
        };

        // Parse `#define CTRL_<NAME> <number>` (decimal or 0x-hex), skipping function-like macros.
        let mut defs: std::collections::HashMap<String, u64> = std::collections::HashMap::new();
        for line in src.lines() {
            let line = line.trim();
            let Some(rest) = line.strip_prefix("#define ") else {
                continue;
            };
            let mut it = rest.split_whitespace();
            let Some(name) = it.next() else { continue };
            if !name.starts_with("CTRL_") || name.contains('(') {
                continue;
            }
            let Some(tok) = it.next() else { continue };
            let val = if let Some(hex) = tok.strip_prefix("0x").or_else(|| tok.strip_prefix("0X")) {
                u64::from_str_radix(hex, 16).ok()
            } else {
                tok.parse::<u64>().ok()
            };
            if let Some(v) = val {
                defs.insert(name.to_string(), v);
            }
        }
        assert!(
            !defs.is_empty(),
            "parsed no CTRL_* defines from {} — header format changed?",
            path.display()
        );

        let expected: &[(&str, u64)] = &[
            ("CTRL_MOVE", FrameType::Move as u64),
            ("CTRL_WHEEL", FrameType::Wheel as u64),
            ("CTRL_BUTTON", FrameType::Button as u64),
            ("CTRL_RESET", FrameType::Reset as u64),
            ("CTRL_QUERY", FrameType::Query as u64),
            ("CTRL_RESP", FrameType::Resp as u64),
            ("CTRL_REBOOT_DL", FrameType::RebootDl as u64),
            ("CTRL_LOG", FrameType::Log as u64),
            ("CTRL_BTN_LEFT", BTN_LEFT as u64),
            ("CTRL_BTN_RIGHT", BTN_RIGHT as u64),
            ("CTRL_BTN_MIDDLE", BTN_MIDDLE as u64),
            ("CTRL_BTN_SIDE1", BTN_SIDE1 as u64),
            ("CTRL_BTN_SIDE2", BTN_SIDE2 as u64),
            ("CTRL_BTN_COUNT", BTN_COUNT as u64),
            ("CTRL_ACT_SOFTREL", ACT_SOFTREL as u64),
            ("CTRL_ACT_PRESS", ACT_PRESS as u64),
            ("CTRL_ACT_FORCEREL", ACT_FORCEREL as u64),
            ("CTRL_Q_VERSION", Q_VERSION as u64),
            ("CTRL_Q_HEALTH", Q_HEALTH as u64),
            ("CTRL_H_LINK_UP", H_LINK_UP as u64),
            ("CTRL_H_MOUSE_ATT", H_MOUSE_ATT as u64),
            ("CTRL_H_CLONE_CFG", H_CLONE_CFG as u64),
            ("CTRL_H_INJECT_ON", H_INJECT_ON as u64),
            ("CTRL_LOG_ERROR", LOG_ERROR as u64),
            ("CTRL_LOG_WARN", LOG_WARN as u64),
            ("CTRL_LOG_INFO", LOG_INFO as u64),
            ("CTRL_LOG_DEBUG", LOG_DEBUG as u64),
            ("CTRL_LOG_VERBOSE", LOG_VERBOSE as u64),
            ("CTRL_PROTO_VER", PROTO_VER as u64),
        ];
        for (name, rust_val) in expected {
            if let Some(&fw_val) = defs.get(*name) {
                assert_eq!(
                    fw_val, *rust_val,
                    "firmware drift: {name} = {fw_val} in ctrl_proto.h but {rust_val} in the Rust crate"
                );
            } else {
                panic!("ctrl_proto.h is missing expected define {name}");
            }
        }
    }
}
