//! Frame opcodes and wire constants — pinned to the documented wire values.
//!
//! Every value here mirrors `firmware/device/components/inject/ctrl_proto.h` (the authoritative
//! constants header) and `docs/protocol/control-protocol.md` (the byte-exact reference).
//!
//! Two test guards back this up (in the `tests` submodule):
//! - `opcodes_match_firmware` pins the in-crate constants to their documented numeric values. It is a
//!   Rust-literal-vs-Rust-literal check, so it catches an accidental in-crate edit — it does **not**
//!   by itself detect firmware drift (both sides would have to be edited in lockstep).
//! - `opcodes_match_ctrl_proto_header` provides the real drift guard: when the firmware header is
//!   reachable (the monorepo dev env) it parses the `#define CTRL_*` values and asserts the Rust
//!   constants equal them; when the header is absent (a published-crate build) it skips.

use core::fmt;

/// Start-of-frame byte. A receiver resynchronizes by scanning for this (§2).
pub const SOF: u8 = 0xA5;

/// Maximum payload length in bytes (§2). A `LEN` greater than this is rejected as bogus.
pub const MAX_PAYLOAD: usize = 512;

/// Protocol version reported in `RESP(VERSION)` (§4.1). The handshake requires this exact value.
pub const PROTO_VER: u8 = 1;

// ---- QUERY selectors (§3.5 / ctrl_proto.h `CTRL_Q_*`) ----

/// `QUERY` selector: request a `RESP(VERSION)`.
pub const Q_VERSION: u8 = 0;
/// `QUERY` selector: request a `RESP(HEALTH)`.
pub const Q_HEALTH: u8 = 1;

// ---- BUTTON ids (§3.3 / ctrl_proto.h `CTRL_BTN_*`) ----

/// `BUTTON` id: left button.
pub const BTN_LEFT: u8 = 0;
/// `BUTTON` id: right button.
pub const BTN_RIGHT: u8 = 1;
/// `BUTTON` id: middle button.
pub const BTN_MIDDLE: u8 = 2;
/// `BUTTON` id: side button 1.
pub const BTN_SIDE1: u8 = 3;
/// `BUTTON` id: side button 2.
pub const BTN_SIDE2: u8 = 4;
/// Number of standard buttons.
pub const BTN_COUNT: u8 = 5;

// ---- BUTTON actions (§3.3 / ctrl_proto.h `CTRL_ACT_*`) ----

/// `BUTTON` action: soft-release (clear our injected press; defer to physical).
pub const ACT_SOFTREL: u8 = 0;
/// `BUTTON` action: press (force the button down regardless of physical).
pub const ACT_PRESS: u8 = 1;
/// `BUTTON` action: force-release (force the button up, masking a physical hold).
pub const ACT_FORCEREL: u8 = 2;

// ---- HEALTH flag bits (§4.2 / ctrl_proto.h `CTRL_H_*`) ----

/// HEALTH flag: inter-chip link to the host chip is up.
pub const H_LINK_UP: u8 = 0x01;
/// HEALTH flag: a real mouse is attached on the host chip.
pub const H_MOUSE_ATT: u8 = 0x02;
/// HEALTH flag: the clone has been configured by the game PC.
pub const H_CLONE_CFG: u8 = 0x04;
/// HEALTH flag: injection is currently active.
pub const H_INJECT_ON: u8 = 0x08;

// ---- LOG levels (§4.3 / ctrl_proto.h `CTRL_LOG_*`) ----

/// LOG level: error.
pub const LOG_ERROR: u8 = 0;
/// LOG level: warn.
pub const LOG_WARN: u8 = 1;
/// LOG level: info.
pub const LOG_INFO: u8 = 2;
/// LOG level: debug.
pub const LOG_DEBUG: u8 = 3;
/// LOG level: verbose.
pub const LOG_VERBOSE: u8 = 4;

/// A frame opcode (the `TYPE` byte, §3 / §4).
///
/// Values are pinned to `ctrl_proto.h` `CTRL_*`. [`FrameType::try_from`] returns `Err` for any
/// unknown byte so the decoder can consume-and-ignore unrecognized frames (the forward-compat
/// mechanism, §2).
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
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

/// Error returned when a byte does not name a known [`FrameType`].
///
/// The decoder treats this as "unknown opcode → ignore the frame" (§2), so an unknown type never
/// breaks compatibility.
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

    /// In-crate constant guard: pins every numeric value to its **documented** wire value
    /// (`ctrl_proto.h` / `control-protocol.md`). This is a Rust-literal vs Rust-literal assertion, so
    /// it catches an accidental in-crate edit but does **not**, on its own, detect firmware drift —
    /// see `opcodes_match_ctrl_proto_header` for the best-effort header-parsing drift guard.
    #[test]
    fn opcodes_match_firmware() {
        // Opcodes (CTRL_* frame TYPE).
        assert_eq!(FrameType::Move as u8, 0x01);
        assert_eq!(FrameType::Wheel as u8, 0x02);
        assert_eq!(FrameType::Button as u8, 0x03);
        assert_eq!(FrameType::Reset as u8, 0x04);
        assert_eq!(FrameType::Query as u8, 0x05);
        assert_eq!(FrameType::Resp as u8, 0x06);
        assert_eq!(FrameType::RebootDl as u8, 0x07);
        assert_eq!(FrameType::Log as u8, 0x08);

        // Framing constants.
        assert_eq!(SOF, 0xA5);
        assert_eq!(MAX_PAYLOAD, 512);
        assert_eq!(PROTO_VER, 1);

        // QUERY selectors.
        assert_eq!(Q_VERSION, 0);
        assert_eq!(Q_HEALTH, 1);

        // BUTTON ids.
        assert_eq!(BTN_LEFT, 0);
        assert_eq!(BTN_RIGHT, 1);
        assert_eq!(BTN_MIDDLE, 2);
        assert_eq!(BTN_SIDE1, 3);
        assert_eq!(BTN_SIDE2, 4);
        assert_eq!(BTN_COUNT, 5);

        // BUTTON actions.
        assert_eq!(ACT_SOFTREL, 0);
        assert_eq!(ACT_PRESS, 1);
        assert_eq!(ACT_FORCEREL, 2);

        // HEALTH flag bits.
        assert_eq!(H_LINK_UP, 0x01);
        assert_eq!(H_MOUSE_ATT, 0x02);
        assert_eq!(H_CLONE_CFG, 0x04);
        assert_eq!(H_INJECT_ON, 0x08);

        // LOG levels.
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

    /// Best-effort **firmware drift** guard (FIX 7). Unlike [`opcodes_match_firmware`] (Rust-vs-Rust),
    /// this parses the real `ctrl_proto.h` `#define CTRL_* <num>` lines and asserts the Rust constants
    /// equal the firmware's values — true drift detection in the monorepo dev env.
    ///
    /// The header is located via the `MEDIUS_CTRL_PROTO_H` env var, else the sibling monorepo path
    /// `<CARGO_MANIFEST_DIR>/../medius-fw/firmware/device/components/inject/ctrl_proto.h`. If neither
    /// exists (a published-crate build), the test **passes** (skips) with an eprintln note, so it never
    /// breaks a standalone build.
    #[test]
    fn opcodes_match_ctrl_proto_header() {
        use std::path::PathBuf;

        // Locate the header: env override first, then the sibling firmware-repo path.
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

        // Parse every `#define CTRL_<NAME> <number>` (decimal or 0x-hex), ignoring trailing comments
        // and function-like macros (a define whose token after the name starts with `(`).
        let mut defs: std::collections::HashMap<String, u64> = std::collections::HashMap::new();
        for line in src.lines() {
            let line = line.trim();
            let Some(rest) = line.strip_prefix("#define ") else {
                continue;
            };
            let mut it = rest.split_whitespace();
            let Some(name) = it.next() else { continue };
            if !name.starts_with("CTRL_") || name.contains('(') {
                continue; // not a plain CTRL_* value define
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

        // (firmware #define name, Rust constant value) — assert each present define matches.
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
