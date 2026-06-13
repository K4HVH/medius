//! Typed response/event decoders (box → PC).

use super::opcode::{Q_HEALTH, Q_VERSION};
use crate::types::{Health, LogLevel, LogLine, Version};

/// A decoded `RESP` (§4.1), keyed by the `what` selector at `payload[0]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resp {
    Version(Version),
    Health(Health),
}

/// Parse a `RESP` payload (§4.1): `[what u8][data..]`.
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
