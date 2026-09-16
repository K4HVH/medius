//! Typed response/event decoders (box → PC).

use std::time::Duration;

use super::opcode::{
    OPT_BEARING, OPT_EMIT, OPT_IMPERFECT, OPT_MOVE_RIDE, OPT_RENDER, OPT_SPREAD, Q_CAPS, Q_CATCH,
    Q_CLIP, Q_DEVICE_INFO, Q_FIRMWARE, Q_HEALTH, Q_LOCKS, Q_OPTIONS, Q_PATCHES, Q_RATE, Q_REWRITE,
    Q_STATS, Q_TRANSFORMS, Q_VERSION,
};
use crate::types::{
    Bearing, Caps, CatchState, ClipStatus, DeviceInfo, EmitPaceStatus, FirmwareInfo, Health,
    ImperfectStatus, Locks, LogLevel, LogLine, PatchSet, Rate, RenderStatus, RewriteTable,
    SpreadStatus, Stats, Transforms, Version,
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
    /// `RESP(OPTIONS, MOVE_RIDE)`: the movement-riding window (`None` = off).
    MovementRiding(Option<Duration>),
    /// `RESP(OPTIONS, EMIT)`: the emit-rate pacing mode and the rate in effect.
    EmitPace(EmitPaceStatus),
    /// `RESP(OPTIONS, RENDER)`: what motion is rendered with, and whether a profile has armed.
    Render(RenderStatus),
    /// `RESP(OPTIONS, SPREAD)`: how far an injected delta is spread, and the interval in effect.
    Spread(SpreadStatus),
    /// `RESP(OPTIONS, BEARING)`: the bearing window and how it is read.
    Bearing(Bearing),
    /// `RESP(CLIP)`: the device-side clip ring and playback status.
    Clip(ClipStatus),
    /// `RESP(FIRMWARE)`: both chips' versions and slot state (§4.16).
    Firmware(FirmwareInfo),
    /// `RESP(REWRITE)`: the rewrite-rule table summary (§4.17).
    Rewrite(RewriteTable),
    /// `RESP(PATCHES)`: the descriptor-patch set summary (§4.17).
    Patches(PatchSet),
    /// `RESP(TRANSFORMS)`: the field-transform table summary (§4.18).
    Transforms(Transforms),
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
                // Variable ASCII name tail after the MAC, LEN-delimited like DEVICE_INFO's product; an
                // older box with no tail decodes to an empty name (the 11-byte header still parses).
                name: String::from_utf8_lossy(&payload[11..]).into_owned(),
            }))
        }
        Q_HEALTH => {
            // `u16` LE since proto 7 (§4.2). The frame `LEN` delimits it; a one-byte payload from an
            // older box still decodes its low byte with the high byte read clear.
            let flags = match payload.get(1..3) {
                Some(w) => u16::from_le_bytes([w[0], w[1]]),
                None => u16::from(*payload.get(1)?),
            };
            Some(Resp::Health(Health::from_flags(flags)))
        }
        Q_DEVICE_INFO => DeviceInfo::from_payload(payload).map(Resp::DeviceInfo),
        Q_CAPS => Caps::from_payload(payload).map(Resp::Caps),
        Q_RATE => Rate::from_payload(payload).map(Resp::Rate),
        Q_STATS => Stats::from_payload(payload).map(Resp::Stats),
        Q_LOCKS => Locks::from_payload(payload).map(Resp::Locks),
        Q_CATCH => CatchState::from_payload(payload).map(Resp::Catch),
        Q_CLIP => ClipStatus::from_payload(payload).map(Resp::Clip),
        Q_FIRMWARE => FirmwareInfo::from_payload(payload).map(Resp::Firmware),
        Q_REWRITE => RewriteTable::from_payload(payload).map(Resp::Rewrite),
        Q_PATCHES => PatchSet::from_payload(payload).map(Resp::Patches),
        Q_TRANSFORMS => Transforms::from_payload(payload).map(Resp::Transforms),
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
                OPT_RENDER => RenderStatus::from_payload(payload).map(Resp::Render),
                OPT_SPREAD => SpreadStatus::from_payload(payload).map(Resp::Spread),
                OPT_BEARING => Bearing::from_payload(payload).map(Resp::Bearing),
                _ => None,
            }
        }
        _ => None,
    }
}

/// Parse a `LOG` payload (§4.7): `[level u8][text UTF-8 (LEN-1)]`.
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
