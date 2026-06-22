//! Typed response/event decoders (box → PC).

use super::opcode::{Q_CAPS, Q_HEALTH, Q_LOCKS, Q_MOUSE_INFO, Q_RATE, Q_STATS, Q_VERSION};
use crate::types::{Caps, Health, Locks, LogLevel, LogLine, MouseInfo, Rate, Stats, Version};

/// A decoded `RESP` (§4.1), keyed by the `what` selector at `payload[0]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resp {
    Version(Version),
    Health(Health),
    MouseInfo(MouseInfo),
    Caps(Caps),
    Rate(Rate),
    Stats(Stats),
    Locks(Locks),
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
        Q_MOUSE_INFO => MouseInfo::from_payload(payload).map(Resp::MouseInfo),
        Q_CAPS => Caps::from_payload(payload).map(Resp::Caps),
        Q_RATE => Rate::from_payload(payload).map(Resp::Rate),
        Q_STATS => Stats::from_payload(payload).map(Resp::Stats),
        Q_LOCKS => Locks::from_payload(payload).map(Resp::Locks),
        _ => None,
    }
}

/// Parse a `LOG` payload (§4.7): `[level u8][text UTF-8 (LEN−1)]`.
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
