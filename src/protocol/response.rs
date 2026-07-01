//! Typed response/event decoders (box → PC).

use std::time::Duration;

use super::opcode::{
    OPT_EMIT, OPT_IMPERFECT, OPT_MOVE_RIDE, Q_CAPS, Q_CATCH, Q_DEVICE_INFO, Q_HEALTH, Q_LOCKS,
    Q_OPTIONS, Q_RATE, Q_STATS, Q_VERSION,
};
use crate::types::{
    Caps, CatchState, DeviceInfo, EmitPaceStatus, Health, ImperfectStatus, Locks, LogLevel,
    LogLine, Rate, Stats, Version,
};

/// A decoded `RESP` (§4.1), keyed by the `what` selector at `payload[0]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resp {
    Version(Version),
    Health(Health),
    DeviceInfo(DeviceInfo),
    Caps(Caps),
    Rate(Rate),
    Stats(Stats),
    Locks(Locks),
    Catch(CatchState),
    Imperfect(ImperfectStatus),
    /// `RESP(OPTIONS, MOVE_RIDE)` — the movement-riding window (`None` = off).
    MovementRiding(Option<Duration>),
    /// `RESP(OPTIONS, EMIT)` — the emit-rate pacing mode and the rate in effect.
    EmitPace(EmitPaceStatus),
}

/// Parse a `RESP` payload (§4.1): `[what u8][data..]`.
pub fn parse_resp(payload: &[u8]) -> Option<Resp> {
    let what = *payload.first()?;
    match what {
        Q_VERSION => {
            if payload.len() < 11 {
                return None;
            }
            let mut mac = [0u8; 6];
            mac.copy_from_slice(&payload[5..11]);
            Some(Resp::Version(Version {
                proto_ver: payload[1],
                fw_major: payload[2],
                fw_minor: payload[3],
                fw_patch: payload[4],
                mac,
            }))
        }
        Q_HEALTH => {
            if payload.len() < 2 {
                return None;
            }
            Some(Resp::Health(Health::from_flags(payload[1])))
        }
        Q_DEVICE_INFO => DeviceInfo::from_payload(payload).map(Resp::DeviceInfo),
        Q_CAPS => Caps::from_payload(payload).map(Resp::Caps),
        Q_RATE => Rate::from_payload(payload).map(Resp::Rate),
        Q_STATS => Stats::from_payload(payload).map(Resp::Stats),
        Q_LOCKS => Locks::from_payload(payload).map(Resp::Locks),
        Q_CATCH => CatchState::from_payload(payload).map(Resp::Catch),
        Q_OPTIONS => {
            let id = *payload.get(1)?;
            match id {
                OPT_IMPERFECT => ImperfectStatus::from_payload(payload).map(Resp::Imperfect),
                OPT_MOVE_RIDE => {
                    if payload.len() < 4 {
                        return None;
                    }
                    let ms = u16::from_le_bytes([payload[2], payload[3]]);
                    let dur = (ms != 0).then(|| Duration::from_millis(ms as u64));
                    Some(Resp::MovementRiding(dur))
                }
                OPT_EMIT => EmitPaceStatus::from_payload(payload).map(Resp::EmitPace),
                _ => None,
            }
        }
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
