//! Conversions between the safe `medius` types and the `#[repr(C)]` mirrors.

use std::os::raw::c_char;
use std::time::Duration;

use medius::{
    Action, Axis, Blanket, BoxInfo, Button, Caps, CatchClass, CatchEntry, CatchEvent, CatchFilter,
    CatchState, Class, ClipState, ClipStatus, ClockDomain, ClockEstimate, CountersSnapshot,
    DeviceInfo, DeviceKind, EmitPace, EmitPaceStatus, Health, ImperfectStatus, KbdCaps, Key,
    LedMode, LedTarget, LockDirection, LockEntry, LockScope, LockTarget, Locks, LogLevel, LogLine,
    MediaKey, Motion, MouseCaps, PortInfo, Rate, RebootTarget, Stats, Usage, Version,
};

use crate::ctypes::*;

#[inline]
fn b(v: bool) -> u8 {
    v as u8
}

fn fill_cstr(dst: &mut [c_char], s: &str) {
    let bytes = s.as_bytes();
    let n = bytes.len().min(dst.len().saturating_sub(1));
    for (slot, &byte) in dst.iter_mut().zip(bytes.iter()).take(n) {
        *slot = byte as c_char;
    }
    dst[n] = 0;
}

fn read_cstr(src: &[c_char]) -> String {
    let bytes: Vec<u8> = src
        .iter()
        .take_while(|&&c| c != 0)
        .map(|&c| c as u8)
        .collect();
    String::from_utf8_lossy(&bytes).into_owned()
}

fn kind_to_medius(k: DeviceKind) -> MediusDeviceKind {
    match k {
        DeviceKind::Unknown => MediusDeviceKind::Unknown,
        DeviceKind::Keyboard => MediusDeviceKind::Keyboard,
        DeviceKind::Mouse => MediusDeviceKind::Mouse,
    }
}

fn kind_from_medius(k: MediusDeviceKind) -> DeviceKind {
    match k {
        MediusDeviceKind::Unknown => DeviceKind::Unknown,
        MediusDeviceKind::Keyboard => DeviceKind::Keyboard,
        MediusDeviceKind::Mouse => DeviceKind::Mouse,
    }
}

impl From<MediusButton> for Button {
    fn from(v: MediusButton) -> Self {
        match v {
            MediusButton::Left => Button::Left,
            MediusButton::Right => Button::Right,
            MediusButton::Middle => Button::Middle,
            MediusButton::Side1 => Button::Side1,
            MediusButton::Side2 => Button::Side2,
        }
    }
}

impl From<MediusAction> for Action {
    fn from(v: MediusAction) -> Self {
        match v {
            MediusAction::SoftRelease => Action::SoftRelease,
            MediusAction::Press => Action::Press,
            MediusAction::ForceRelease => Action::ForceRelease,
        }
    }
}

impl From<MediusRebootTarget> for RebootTarget {
    fn from(v: MediusRebootTarget) -> Self {
        match v {
            MediusRebootTarget::DeviceDownload => RebootTarget::DeviceDownload,
            MediusRebootTarget::HostDownload => RebootTarget::HostDownload,
            MediusRebootTarget::DeviceRun => RebootTarget::DeviceRun,
            MediusRebootTarget::HostRun => RebootTarget::HostRun,
        }
    }
}

impl From<MediusLedTarget> for LedTarget {
    fn from(v: MediusLedTarget) -> Self {
        match v {
            MediusLedTarget::Device => LedTarget::Device,
            MediusLedTarget::Host => LedTarget::Host,
            MediusLedTarget::Both => LedTarget::Both,
        }
    }
}

impl From<MediusLedMode> for LedMode {
    fn from(v: MediusLedMode) -> Self {
        match v {
            MediusLedMode::Auto => LedMode::Auto,
            MediusLedMode::Off => LedMode::Off,
            MediusLedMode::Solid => LedMode::Solid,
            MediusLedMode::Blink => LedMode::Blink,
        }
    }
}

impl From<MediusLockDirection> for LockDirection {
    fn from(v: MediusLockDirection) -> Self {
        match v {
            MediusLockDirection::Both => LockDirection::Both,
            MediusLockDirection::Positive => LockDirection::Positive,
            MediusLockDirection::Negative => LockDirection::Negative,
        }
    }
}

fn lock_direction_to_c(d: LockDirection) -> MediusLockDirection {
    match d {
        LockDirection::Both => MediusLockDirection::Both,
        LockDirection::Positive => MediusLockDirection::Positive,
        LockDirection::Negative => MediusLockDirection::Negative,
    }
}

impl From<MediusBlanket> for Blanket {
    fn from(v: MediusBlanket) -> Self {
        match v {
            MediusBlanket::Keys => Blanket::Keys,
            MediusBlanket::Media => Blanket::Media,
            MediusBlanket::Buttons => Blanket::Buttons,
            MediusBlanket::Aim => Blanket::Aim,
            MediusBlanket::Wheel => Blanket::Wheel,
        }
    }
}

impl From<MediusLogLevel> for LogLevel {
    fn from(v: MediusLogLevel) -> Self {
        match v {
            MediusLogLevel::Error => LogLevel::Error,
            MediusLogLevel::Warn => LogLevel::Warn,
            MediusLogLevel::Info => LogLevel::Info,
            MediusLogLevel::Debug => LogLevel::Debug,
            MediusLogLevel::Verbose => LogLevel::Verbose,
        }
    }
}

/// `MediusLockTarget` to [`LockTarget`]; `None` when a `Usage` target has an out-of-range button id.
pub(crate) fn lock_target_to_medius(v: MediusLockTarget) -> Option<LockTarget> {
    Some(match v.kind {
        MediusLockTargetKind::X => LockTarget::Axis(Axis::X),
        MediusLockTargetKind::Y => LockTarget::Axis(Axis::Y),
        MediusLockTargetKind::Wheel => LockTarget::Axis(Axis::Wheel),
        MediusLockTargetKind::Usage => LockTarget::Usage(input_to_medius(v.usage)?),
    })
}

impl From<MediusMotion> for Motion {
    fn from(v: MediusMotion) -> Self {
        match v.kind {
            MediusMotionKind::Cursor => Motion::Cursor { dx: v.dx, dy: v.dy },
            MediusMotionKind::Wheel => Motion::Wheel(v.wheel),
        }
    }
}

/// `MediusUsage` to a [`Usage`]; `None` when a button/key id is out of range for its class.
pub(crate) fn input_to_medius(v: MediusUsage) -> Option<Usage> {
    Some(match v.kind {
        MediusClass::Button => Usage::from(Button::from_id(u8::try_from(v.id).ok()?)?),
        MediusClass::Key => Usage::from(Key::new(u8::try_from(v.id).ok()?)),
        MediusClass::Media => Usage::from(MediaKey::new(v.id)),
    })
}

fn class_kind(class: Class) -> MediusClass {
    match class {
        Class::Button => MediusClass::Button,
        Class::Key => MediusClass::Key,
        Class::Media => MediusClass::Media,
    }
}

fn kind_class(kind: MediusClass) -> Class {
    match kind {
        MediusClass::Button => Class::Button,
        MediusClass::Key => Class::Key,
        MediusClass::Media => Class::Media,
    }
}

fn usage_to_c(u: Usage) -> MediusUsage {
    MediusUsage {
        kind: class_kind(u.class),
        id: u.id,
    }
}

fn lock_target_to_c(t: LockTarget) -> MediusLockTarget {
    match t {
        LockTarget::Axis(Axis::X) => axis_target(MediusLockTargetKind::X),
        LockTarget::Axis(Axis::Y) => axis_target(MediusLockTargetKind::Y),
        LockTarget::Axis(Axis::Wheel) => axis_target(MediusLockTargetKind::Wheel),
        LockTarget::Usage(u) => MediusLockTarget {
            kind: MediusLockTargetKind::Usage,
            usage: usage_to_c(u),
        },
    }
}

fn axis_target(kind: MediusLockTargetKind) -> MediusLockTarget {
    MediusLockTarget {
        kind,
        usage: MediusUsage {
            kind: MediusClass::Button,
            id: 0,
        },
    }
}

/// `(MediusEmitMode, hz)` to `EmitPace`; `hz` matters only for `Fixed`.
pub(crate) fn emit_pace_to_medius(mode: MediusEmitMode, hz: u16) -> EmitPace {
    match mode {
        MediusEmitMode::Learned => EmitPace::Learned,
        MediusEmitMode::Interval => EmitPace::Interval,
        MediusEmitMode::Fixed => EmitPace::Fixed(hz),
    }
}

impl From<Version> for MediusVersion {
    fn from(v: Version) -> Self {
        let mut name = [0 as c_char; MEDIUS_MAX_NAME];
        fill_cstr(&mut name, &v.name);
        MediusVersion {
            proto_ver: v.proto_ver,
            fw_major: v.fw_major,
            fw_minor: v.fw_minor,
            fw_patch: v.fw_patch,
            mac: v.mac,
            name,
        }
    }
}

impl From<Health> for MediusHealth {
    fn from(h: Health) -> Self {
        MediusHealth {
            link_up: b(h.link_up),
            mouse_attached: b(h.mouse_attached),
            clone_configured: b(h.clone_configured),
            injection_active: b(h.injection_active),
            rate_confident: b(h.rate_confident),
            lock_on: b(h.lock_on),
            catch_on: b(h.catch_on),
            kbd_attached: b(h.kbd_attached),
        }
    }
}

impl From<MouseCaps> for MediusMouseCaps {
    fn from(c: MouseCaps) -> Self {
        MediusMouseCaps {
            n_buttons: c.n_buttons,
            has_x: b(c.has_x),
            has_y: b(c.has_y),
            has_wheel: b(c.has_wheel),
            has_report_id: b(c.has_report_id),
            n_hid: c.n_hid,
        }
    }
}

impl From<KbdCaps> for MediusKbdCaps {
    fn from(c: KbdCaps) -> Self {
        MediusKbdCaps {
            n_keys: c.n_keys,
            nkro: b(c.nkro),
            has_consumer: b(c.has_consumer),
            has_system: b(c.has_system),
            has_report_id: b(c.has_report_id),
        }
    }
}

impl From<Caps> for MediusCaps {
    fn from(c: Caps) -> Self {
        MediusCaps {
            mouse: c.mouse.into(),
            keyboard: c.keyboard.into(),
            mouse_change_driven: b(c.mouse_change_driven),
            kbd_change_driven: b(c.kbd_change_driven),
        }
    }
}

impl From<DeviceInfo> for MediusDeviceInfo {
    fn from(m: DeviceInfo) -> Self {
        let mut product = [0 as c_char; MEDIUS_MAX_PRODUCT];
        fill_cstr(&mut product, &m.product);
        MediusDeviceInfo {
            vid: m.vid,
            pid: m.pid,
            bcd_device: m.bcd_device,
            bcd_usb: m.bcd_usb,
            has_serial: b(m.has_serial),
            has_bos: b(m.has_bos),
            kind: kind_to_medius(m.kind),
            product,
        }
    }
}

impl From<Rate> for MediusRate {
    fn from(r: Rate) -> Self {
        MediusRate {
            native_period_us: r.native_period_us,
            poll_period_us: r.poll_period_us,
            confident: b(r.confident),
            change_driven: b(r.change_driven),
        }
    }
}

impl From<Stats> for MediusStats {
    fn from(s: Stats) -> Self {
        MediusStats {
            inject_emits: s.inject_emits,
            tx_drops: s.tx_drops,
            tx_merges: s.tx_merges,
            tx_maxdepth: s.tx_maxdepth,
            tx_wedges: s.tx_wedges,
            wakeups: s.wakeups,
            reset_count: s.reset_count,
            config_count: s.config_count,
        }
    }
}

impl From<Locks> for MediusLocks {
    fn from(l: Locks) -> Self {
        let blank = MediusLockEntry {
            target: axis_target(MediusLockTargetKind::X),
            is_blanket: false,
            positive: false,
            negative: false,
        };
        let mut out = MediusLocks {
            n: 0,
            entries: [blank; MEDIUS_MAX_LOCKS],
        };
        for e in l.entries() {
            if out.n as usize >= MEDIUS_MAX_LOCKS {
                break;
            }
            let (target, is_blanket) = match e.scope {
                LockScope::Blanket(class) => {
                    let target = MediusLockTarget {
                        kind: MediusLockTargetKind::Usage,
                        usage: MediusUsage {
                            kind: class_kind(class),
                            id: 0,
                        },
                    };
                    (target, true)
                }
                LockScope::Target(t) => (lock_target_to_c(t), false),
            };
            out.entries[out.n as usize] = MediusLockEntry {
                target,
                is_blanket,
                positive: e.positive,
                negative: e.negative,
            };
            out.n += 1;
        }
        out
    }
}

fn clock_domain_to_c(d: ClockDomain) -> MediusClockDomain {
    match d {
        ClockDomain::HostChip => MediusClockDomain::HostChip,
        ClockDomain::DeviceChip => MediusClockDomain::DeviceChip,
    }
}

#[cfg(feature = "mock")]
pub(crate) fn clock_domain_to_native(d: MediusClockDomain) -> ClockDomain {
    match d {
        MediusClockDomain::HostChip => ClockDomain::HostChip,
        MediusClockDomain::DeviceChip => ClockDomain::DeviceChip,
    }
}

/// A [`CatchFilter`] to the C struct, wildcards resolved to their sentinels.
pub(crate) fn catch_filter_to_c(f: CatchFilter) -> MediusCatchFilter {
    MediusCatchFilter {
        class: f.class.map_or(MEDIUS_CATCH_CLASS_ANY, CatchClass::as_u8),
        id: f.id.unwrap_or(MEDIUS_CATCH_ID_ANY),
        direction: lock_direction_to_c(f.direction),
        snaplen: f.snaplen,
    }
}

/// The C struct back to a [`CatchFilter`]; `None` when `class` names no known class.
pub(crate) fn catch_filter_from_c(f: MediusCatchFilter) -> Option<CatchFilter> {
    Some(CatchFilter {
        class: if f.class == MEDIUS_CATCH_CLASS_ANY {
            None
        } else {
            Some(CatchClass::from_u8(f.class)?)
        },
        id: (f.id != MEDIUS_CATCH_ID_ANY).then_some(f.id),
        direction: f.direction.into(),
        snaplen: f.snaplen,
    })
}

fn clock_estimate_to_c(c: ClockEstimate) -> MediusClockEstimate {
    MediusClockEstimate {
        offset_us: c.offset_us,
        rate_ppb: c.rate_ppb,
        delay_us: c.delay_us,
        // Saturate one short of the sentinel so a real age can never read as "no estimate".
        age_ms: c.age.map_or(MEDIUS_CLOCK_AGE_NONE, |d| {
            d.as_millis().min(MEDIUS_CLOCK_AGE_NONE as u128 - 1) as u32
        }),
    }
}

fn clock_estimate_from_c(c: MediusClockEstimate) -> ClockEstimate {
    ClockEstimate {
        offset_us: c.offset_us,
        rate_ppb: c.rate_ppb,
        delay_us: c.delay_us,
        age: (c.age_ms != MEDIUS_CLOCK_AGE_NONE).then(|| Duration::from_millis(c.age_ms as u64)),
    }
}

impl From<CatchState> for MediusCatchState {
    fn from(c: CatchState) -> Self {
        let blank = MediusCatchEntry {
            filter: catch_filter_to_c(CatchFilter::all()),
            dropped: 0,
        };
        let mut entries = [blank; MEDIUS_MAX_CATCH_ENTRIES];
        let n = c.entries.len().min(MEDIUS_MAX_CATCH_ENTRIES);
        for (slot, e) in entries.iter_mut().zip(c.entries.iter()).take(n) {
            *slot = MediusCatchEntry {
                filter: catch_filter_to_c(e.filter),
                dropped: e.dropped,
            };
        }
        MediusCatchState {
            table_full: b(c.table_full),
            dropped: c.dropped,
            clock: clock_estimate_to_c(c.clock),
            n: n as u16,
            entries,
        }
    }
}

impl From<ImperfectStatus> for MediusImperfectStatus {
    fn from(s: ImperfectStatus) -> Self {
        MediusImperfectStatus {
            allowed: b(s.allowed),
            over_capacity: b(s.over_capacity),
            clone_imperfect: b(s.clone_imperfect),
        }
    }
}

impl From<ClipState> for MediusClipState {
    fn from(s: ClipState) -> Self {
        match s {
            ClipState::Idle => MediusClipState::Idle,
            ClipState::Playing => MediusClipState::Playing,
            ClipState::Paused => MediusClipState::Paused,
            ClipState::Faulted => MediusClipState::Faulted,
        }
    }
}

impl From<ClipStatus> for MediusClipStatus {
    fn from(s: ClipStatus) -> Self {
        let mut held = [MediusUsage {
            kind: MediusClass::Button,
            id: 0,
        }; MEDIUS_MAX_USAGES];
        let n = s.held.len().min(MEDIUS_MAX_USAGES);
        for (slot, u) in held.iter_mut().zip(s.held.iter()).take(n) {
            *slot = usage_to_c(*u);
        }
        MediusClipStatus {
            state: s.state.into(),
            free: s.free,
            total: s.total,
            played: s.played,
            ticks: s.ticks,
            underruns: s.underruns,
            overruns: s.overruns,
            seq_gaps: s.seq_gaps,
            held_n: n as u16,
            held,
        }
    }
}

impl From<MediusClipState> for ClipState {
    fn from(s: MediusClipState) -> Self {
        match s {
            MediusClipState::Idle => ClipState::Idle,
            MediusClipState::Playing => ClipState::Playing,
            MediusClipState::Paused => ClipState::Paused,
            MediusClipState::Faulted => ClipState::Faulted,
        }
    }
}

/// Serialize clip settings to the C struct (autolock as a `CLIP_LOCK_*` bitmask, triggers into the fixed array).
pub(crate) fn clip_settings_to_c(s: &medius::ClipSettings) -> MediusClipSettings {
    let mut triggers = [MediusClipTrigger {
        on: MediusUsage {
            kind: MediusClass::Button,
            id: 0,
        },
        edge: MediusEdge::Both,
        action: MediusClipAction::Start,
        consume: 0,
    }; MEDIUS_CLIP_TRIG_MAX];
    let n = s.triggers.len().min(MEDIUS_CLIP_TRIG_MAX);
    for (slot, t) in triggers.iter_mut().zip(s.triggers.iter()).take(n) {
        *slot = MediusClipTrigger {
            on: usage_to_c(t.on),
            edge: match t.edge {
                medius::Edge::Both => MediusEdge::Both,
                medius::Edge::Press => MediusEdge::Press,
                medius::Edge::Release => MediusEdge::Release,
            },
            action: match t.action {
                medius::ClipAction::Start => MediusClipAction::Start,
                medius::ClipAction::Stop => MediusClipAction::Stop,
                medius::ClipAction::Pause => MediusClipAction::Pause,
                medius::ClipAction::Resume => MediusClipAction::Resume,
                medius::ClipAction::Restart => MediusClipAction::Restart,
                medius::ClipAction::Toggle => MediusClipAction::Toggle,
            },
            consume: t.consume as u8,
        };
    }
    MediusClipSettings {
        autolock_bits: s.autolock.iter().fold(0u8, |m, b| m | blanket_bit(*b)),
        loop_: s.loop_ as u8,
        retain: s.retain as u8,
        finalized: s.finalized as u8,
        triggers,
        n: n as u8,
    }
}

/// A blanket group's `CLIP_LOCK_*` scope bit.
fn blanket_bit(b: Blanket) -> u8 {
    match b {
        Blanket::Aim => 0x01,
        Blanket::Wheel => 0x02,
        Blanket::Buttons => 0x04,
        Blanket::Keys => 0x08,
        Blanket::Media => 0x10,
    }
}

/// Deserialize clip settings from the C struct (the inverse of [`clip_settings_to_c`]).
#[cfg(feature = "mock")]
pub(crate) fn clip_settings_from_c(c: &MediusClipSettings) -> medius::ClipSettings {
    let n = (c.n as usize).min(MEDIUS_CLIP_TRIG_MAX);
    let triggers = c.triggers[..n]
        .iter()
        .filter_map(|t| {
            input_to_medius(t.on).map(|on| medius::ClipTrigger {
                on,
                edge: match t.edge {
                    MediusEdge::Both => medius::Edge::Both,
                    MediusEdge::Press => medius::Edge::Press,
                    MediusEdge::Release => medius::Edge::Release,
                },
                action: match t.action {
                    MediusClipAction::Start => medius::ClipAction::Start,
                    MediusClipAction::Stop => medius::ClipAction::Stop,
                    MediusClipAction::Pause => medius::ClipAction::Pause,
                    MediusClipAction::Resume => medius::ClipAction::Resume,
                    MediusClipAction::Restart => medius::ClipAction::Restart,
                    MediusClipAction::Toggle => medius::ClipAction::Toggle,
                },
                consume: t.consume != 0,
            })
        })
        .collect();
    let autolock = [
        Blanket::Aim,
        Blanket::Wheel,
        Blanket::Buttons,
        Blanket::Keys,
        Blanket::Media,
    ]
    .into_iter()
    .filter(|&b| c.autolock_bits & blanket_bit(b) != 0)
    .collect();
    medius::ClipSettings {
        autolock,
        loop_: c.loop_ != 0,
        retain: c.retain != 0,
        finalized: c.finalized != 0,
        triggers,
    }
}

impl From<MediusClipStatus> for ClipStatus {
    fn from(s: MediusClipStatus) -> Self {
        let n = (s.held_n as usize).min(MEDIUS_MAX_USAGES);
        let held = s.held[..n]
            .iter()
            .filter_map(|&u| input_to_medius(u))
            .collect();
        ClipStatus {
            state: s.state.into(),
            free: s.free,
            total: s.total,
            played: s.played,
            ticks: s.ticks,
            underruns: s.underruns,
            overruns: s.overruns,
            seq_gaps: s.seq_gaps,
            held,
        }
    }
}

impl From<EmitPaceStatus> for MediusEmitPaceStatus {
    fn from(s: EmitPaceStatus) -> Self {
        let (mode, fixed_hz) = match s.mode {
            EmitPace::Learned => (MediusEmitMode::Learned, 0),
            EmitPace::Interval => (MediusEmitMode::Interval, 0),
            EmitPace::Fixed(hz) => (MediusEmitMode::Fixed, hz),
        };
        MediusEmitPaceStatus {
            mode,
            fixed_hz,
            resolved_hz: s.resolved_hz,
        }
    }
}

impl From<CountersSnapshot> for MediusCountersSnapshot {
    fn from(c: CountersSnapshot) -> Self {
        MediusCountersSnapshot {
            frames_tx: c.frames_tx,
            frames_rx: c.frames_rx,
            crc_drops: c.crc_drops,
            reconnects: c.reconnects,
        }
    }
}

impl From<LogLevel> for MediusLogLevel {
    fn from(l: LogLevel) -> Self {
        match l {
            LogLevel::Error => MediusLogLevel::Error,
            LogLevel::Warn => MediusLogLevel::Warn,
            LogLevel::Info => MediusLogLevel::Info,
            LogLevel::Debug => MediusLogLevel::Debug,
            LogLevel::Verbose => MediusLogLevel::Verbose,
        }
    }
}

impl From<CatchEvent> for MediusCatchEvent {
    fn from(e: CatchEvent) -> Self {
        match e {
            CatchEvent::Motion(m) => MediusCatchEvent {
                kind: MediusCatchEventKind::Motion,
                ts_us: m.ts_us,
                clock: clock_domain_to_c(m.clock),
                data: MediusCatchEventData {
                    motion: MediusMotionEvent {
                        dx: m.dx,
                        dy: m.dy,
                        dz: m.dz,
                    },
                },
            },
            CatchEvent::Usages(s) => {
                let mut usages = [MediusUsage {
                    kind: MediusClass::Button,
                    id: 0,
                }; MEDIUS_MAX_USAGES];
                let n = s.usages.len().min(MEDIUS_MAX_USAGES);
                for (slot, u) in usages.iter_mut().zip(s.usages.iter()).take(n) {
                    *slot = usage_to_c(*u);
                }
                MediusCatchEvent {
                    kind: MediusCatchEventKind::Usages,
                    ts_us: s.ts_us,
                    clock: clock_domain_to_c(s.clock),
                    data: MediusCatchEventData {
                        usages: MediusUsageEvent {
                            n: n as u16,
                            usages,
                        },
                    },
                }
            }
            CatchEvent::Traffic(t) => {
                let mut bytes = [0u8; MEDIUS_MAX_TRAFFIC_BYTES];
                let n = t.bytes.len().min(MEDIUS_MAX_TRAFFIC_BYTES);
                bytes[..n].copy_from_slice(&t.bytes[..n]);
                MediusCatchEvent {
                    kind: MediusCatchEventKind::Traffic,
                    ts_us: t.ts_us,
                    clock: clock_domain_to_c(t.clock),
                    data: MediusCatchEventData {
                        traffic: MediusTrafficEvent {
                            class: t.class.as_u8(),
                            id: t.id,
                            direction: lock_direction_to_c(t.direction),
                            flags: t.flags,
                            true_len: t.true_len,
                            len: n as u16,
                            bytes,
                        },
                    },
                }
            }
        }
    }
}

impl From<&LogLine> for MediusLogLine {
    fn from(l: &LogLine) -> Self {
        let mut text = [0 as c_char; MEDIUS_MAX_LOG_TEXT];
        fill_cstr(&mut text, &l.text);
        MediusLogLine {
            level: l.level.into(),
            text,
        }
    }
}

#[inline]
fn nz(v: u8) -> bool {
    v != 0
}

impl From<MediusVersion> for Version {
    fn from(v: MediusVersion) -> Self {
        Version {
            proto_ver: v.proto_ver,
            fw_major: v.fw_major,
            fw_minor: v.fw_minor,
            fw_patch: v.fw_patch,
            mac: v.mac,
            name: read_cstr(&v.name),
        }
    }
}

impl From<MediusHealth> for Health {
    fn from(h: MediusHealth) -> Self {
        Health {
            link_up: nz(h.link_up),
            mouse_attached: nz(h.mouse_attached),
            clone_configured: nz(h.clone_configured),
            injection_active: nz(h.injection_active),
            rate_confident: nz(h.rate_confident),
            lock_on: nz(h.lock_on),
            catch_on: nz(h.catch_on),
            kbd_attached: nz(h.kbd_attached),
        }
    }
}

impl From<MediusMouseCaps> for MouseCaps {
    fn from(c: MediusMouseCaps) -> Self {
        MouseCaps {
            n_buttons: c.n_buttons,
            has_x: nz(c.has_x),
            has_y: nz(c.has_y),
            has_wheel: nz(c.has_wheel),
            has_report_id: nz(c.has_report_id),
            n_hid: c.n_hid,
        }
    }
}

impl From<MediusKbdCaps> for KbdCaps {
    fn from(c: MediusKbdCaps) -> Self {
        KbdCaps {
            n_keys: c.n_keys,
            nkro: nz(c.nkro),
            has_consumer: nz(c.has_consumer),
            has_system: nz(c.has_system),
            has_report_id: nz(c.has_report_id),
        }
    }
}

impl From<MediusCaps> for Caps {
    fn from(c: MediusCaps) -> Self {
        Caps {
            mouse: c.mouse.into(),
            keyboard: c.keyboard.into(),
            mouse_change_driven: nz(c.mouse_change_driven),
            kbd_change_driven: nz(c.kbd_change_driven),
        }
    }
}

impl From<MediusDeviceInfo> for DeviceInfo {
    fn from(m: MediusDeviceInfo) -> Self {
        DeviceInfo {
            vid: m.vid,
            pid: m.pid,
            bcd_device: m.bcd_device,
            bcd_usb: m.bcd_usb,
            has_serial: nz(m.has_serial),
            has_bos: nz(m.has_bos),
            kind: kind_from_medius(m.kind),
            product: read_cstr(&m.product),
        }
    }
}

impl From<MediusRate> for Rate {
    fn from(r: MediusRate) -> Self {
        Rate {
            native_period_us: r.native_period_us,
            poll_period_us: r.poll_period_us,
            confident: nz(r.confident),
            change_driven: nz(r.change_driven),
        }
    }
}

impl From<MediusStats> for Stats {
    fn from(s: MediusStats) -> Self {
        Stats {
            inject_emits: s.inject_emits,
            tx_drops: s.tx_drops,
            tx_merges: s.tx_merges,
            tx_maxdepth: s.tx_maxdepth,
            tx_wedges: s.tx_wedges,
            wakeups: s.wakeups,
            reset_count: s.reset_count,
            config_count: s.config_count,
        }
    }
}

impl From<MediusLocks> for Locks {
    fn from(l: MediusLocks) -> Self {
        let n = (l.n as usize).min(MEDIUS_MAX_LOCKS);
        let entries = l.entries[..n]
            .iter()
            .filter_map(|e| {
                let scope = if e.is_blanket {
                    LockScope::Blanket(kind_class(e.target.usage.kind))
                } else {
                    LockScope::Target(lock_target_to_medius(e.target)?)
                };
                Some(LockEntry {
                    scope,
                    positive: e.positive,
                    negative: e.negative,
                })
            })
            .collect();
        Locks::from_entries(entries)
    }
}

impl From<MediusCatchState> for CatchState {
    fn from(c: MediusCatchState) -> Self {
        let n = (c.n as usize).min(MEDIUS_MAX_CATCH_ENTRIES);
        let entries = c.entries[..n]
            .iter()
            .filter_map(|e| {
                Some(CatchEntry {
                    filter: catch_filter_from_c(e.filter)?,
                    dropped: e.dropped,
                })
            })
            .collect();
        CatchState {
            table_full: nz(c.table_full),
            dropped: c.dropped,
            clock: clock_estimate_from_c(c.clock),
            entries,
        }
    }
}

impl From<MediusImperfectStatus> for ImperfectStatus {
    fn from(s: MediusImperfectStatus) -> Self {
        ImperfectStatus {
            allowed: nz(s.allowed),
            over_capacity: nz(s.over_capacity),
            clone_imperfect: nz(s.clone_imperfect),
        }
    }
}

/// The held usages of a `MediusUsageEvent` as a `Usage` list; invalid entries are dropped.
#[cfg(feature = "mock")]
pub(crate) fn usage_event_to_medius(e: &MediusUsageEvent) -> Vec<Usage> {
    let n = (e.n as usize).min(MEDIUS_MAX_USAGES);
    e.usages[..n]
        .iter()
        .filter_map(|&u| input_to_medius(u))
        .collect()
}

#[cfg(feature = "mock")]
impl From<medius::FrameType> for MediusFrameType {
    fn from(t: medius::FrameType) -> Self {
        use medius::FrameType as F;
        match t {
            F::Move => MediusFrameType::Move,
            F::Inject => MediusFrameType::Inject,
            F::Reset => MediusFrameType::Reset,
            F::Query => MediusFrameType::Query,
            F::Resp => MediusFrameType::Resp,
            F::RebootDl => MediusFrameType::RebootDl,
            F::Log => MediusFrameType::Log,
            F::Led => MediusFrameType::Led,
            F::Lock => MediusFrameType::Lock,
            F::Catch => MediusFrameType::Catch,
            F::MotionEvent => MediusFrameType::MotionEvent,
            F::UsageEvent => MediusFrameType::UsageEvent,
            F::TrafficEvent => MediusFrameType::TrafficEvent,
            F::Option => MediusFrameType::Option,
            F::ClipAppend => MediusFrameType::ClipAppend,
            F::ClipCtrl => MediusFrameType::ClipCtrl,
            F::ClipSet => MediusFrameType::ClipSet,
            F::ClipTrigger => MediusFrameType::ClipTrigger,
        }
    }
}

/// `MediusFrameType` to `medius::FrameType`; `None` if the value is out of range.
#[cfg(feature = "mock")]
pub(crate) fn frame_type_to_native(t: MediusFrameType) -> Option<medius::FrameType> {
    medius::FrameType::try_from(t as u8).ok()
}

/// `PortInfo` to `MediusPortInfo`; `None` if the path would not fit (never a half-written string).
pub(crate) fn port_to_medius(p: &PortInfo) -> Option<MediusPortInfo> {
    if p.path.len() >= MEDIUS_MAX_PATH {
        return None;
    }
    let mut path = [0 as c_char; MEDIUS_MAX_PATH];
    fill_cstr(&mut path, &p.path);
    let mut serial = [0 as c_char; MEDIUS_MAX_SERIAL];
    if let Some(s) = &p.serial {
        fill_cstr(&mut serial, s);
    }
    Some(MediusPortInfo {
        path,
        vid: p.vid,
        pid: p.pid,
        serial,
        has_serial: b(p.serial.is_some()),
    })
}

/// `BoxInfo` to `MediusBoxInfo`; `None` if the port path would not fit.
pub(crate) fn box_to_medius(b: &BoxInfo) -> Option<MediusBoxInfo> {
    Some(MediusBoxInfo {
        port: port_to_medius(&b.port)?,
        version: b.version.clone().into(),
        device: b.device.clone().into(),
    })
}
