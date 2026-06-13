//! Typed response/event decoders (box → PC).
//!
//! Decoders for the `RESP` (§4.1) and `LOG` (§4.3) payloads, operating on payload bytes only (the
//! frame layer stripped framing and verified the CRC). A truncated or malformed payload yields
//! `None` (or a safe default for `LOG`), never a panic.

use super::opcode::{Q_HEALTH, Q_VERSION};
use crate::types::{Health, LogLevel, LogLine, Version};

/// A decoded `RESP` (§4.1), keyed by the `what` selector at `payload[0]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resp {
    /// `RESP(VERSION)` — `what = 0`.
    Version(Version),
    /// `RESP(HEALTH)` — `what = 1`.
    Health(Health),
}

/// Parse a `RESP` payload (§4.1): `[what u8][data..]`.
///
/// Returns `None` if the payload is empty, the `what` selector is unknown, or the data is too short
/// for the selector. Never panics.
///
/// - `what = 0` VERSION → `[0, proto_ver, fw_major, fw_minor, fw_patch]` (needs ≥ 5 bytes).
/// - `what = 1` HEALTH  → `[1, flags]` (needs ≥ 2 bytes).
///
/// # Examples
/// ```ignore
/// # use medius::protocol::response::{parse_resp, Resp};
/// # use medius::types::Version;
/// assert_eq!(
///     parse_resp(&[0, 1, 0, 1, 0]),
///     Some(Resp::Version(Version { proto_ver: 1, fw_major: 0, fw_minor: 1, fw_patch: 0 })),
/// );
/// ```
pub fn parse_resp(payload: &[u8]) -> Option<Resp> {
    let what = *payload.first()?;
    match what {
        Q_VERSION => {
            if payload.len() < 5 {
                return None;
            }
            Some(Resp::Version(Version {
                proto_ver: payload[1],
                fw_major: payload[2],
                fw_minor: payload[3],
                fw_patch: payload[4],
            }))
        }
        Q_HEALTH => {
            if payload.len() < 2 {
                return None;
            }
            Some(Resp::Health(Health::from_flags(payload[1])))
        }
        _ => None,
    }
}

/// Parse a `LOG` payload (§4.3): `[level u8][text UTF-8 (LEN−1)]`.
///
/// Text is decoded lossily; an unknown level falls back to [`LogLevel::Info`], and an empty payload
/// yields an empty `Info` line.
///
/// # Examples
/// ```ignore
/// # use medius::protocol::response::parse_log;
/// # use medius::types::LogLevel;
/// let line = parse_log(&[1, b'h', b'i']);
/// assert_eq!(line.level, LogLevel::Warn);
/// assert_eq!(line.text, "hi");
/// ```
pub fn parse_log(payload: &[u8]) -> LogLine {
    match payload.split_first() {
        Some((&level, text)) => LogLine {
            level: LogLevel::from_u8(level),
            text: String::from_utf8_lossy(text).into_owned(),
        },
        None => LogLine {
            level: LogLevel::Info,
            text: String::new(),
        },
    }
}
