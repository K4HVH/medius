//! Conversions between the safe `medius` types and the `#[repr(C)]` mirrors.
//!
//! `From<MediusX> for medius::X` handles command parameters; `From<medius::X> for MediusX` handles
//! query results and stream events. Both directions are concrete-to-concrete, so the orphan rule
//! permits the foreign-for-local impls.

use std::os::raw::c_char;

use medius::{
    Action, Axis, Blanket, BoxInfo, Button, Caps, CatchEvent, CatchMask, CatchState, Class,
    ClipState, ClipStatus, CountersSnapshot, DeviceInfo, DeviceKind, EmitPace, EmitPaceStatus,
    Health, ImperfectStatus, KbdCaps, Key, LedMode, LedTarget, LockDirection, LockEntry,
    LockTarget, Locks, LogLevel, LogLine, MediaKey, Motion, MouseCaps, PortInfo, Rate,
    RebootTarget, Stats, Usage, Version,
};

use crate::ctypes::*;

#[inline]
fn b(v: bool) -> u8 {
    v as u8
}

/// Copy `s` into a fixed C buffer, NUL-terminated, truncating to fit.
fn fill_cstr(dst: &mut [c_char], s: &str) {
    let bytes = s.as_bytes();
    let n = bytes.len().min(dst.len().saturating_sub(1));
    for (slot, &byte) in dst.iter_mut().zip(bytes.iter()).take(n) {
        *slot = byte as c_char;
    }
    dst[n] = 0;
}

/// Read a NUL-terminated fixed C buffer back into a `String` (lossy).
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

// --- command parameters: Medius -> medius ---

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

/// `MediusLockTarget` -> [`LockTarget`]. `None` when a `Usage` target carries an out-of-range button id.
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

/// `MediusInput` -> a [`Usage`]. `None` when a button carries an out-of-range button id.
pub(crate) fn input_to_medius(v: MediusInput) -> Option<Usage> {
    Some(match v.kind {
        MediusInputKind::Button => Usage::from(Button::from_id(v.value as u8)?),
        MediusInputKind::Key => Usage::from(Key::new(v.value as u8)),
        MediusInputKind::Media => Usage::from(MediaKey::new(v.value)),
    })
}

/// A [`Class`] as the matching `MediusInputKind` arm.
fn class_kind(class: Class) -> MediusInputKind {
    match class {
        Class::Button => MediusInputKind::Button,
        Class::Key => MediusInputKind::Key,
        Class::Media => MediusInputKind::Media,
    }
}

/// A `MediusInputKind` as the matching [`Class`].
fn kind_class(kind: MediusInputKind) -> Class {
    match kind {
        MediusInputKind::Button => Class::Button,
        MediusInputKind::Key => Class::Key,
        MediusInputKind::Media => Class::Media,
    }
}

/// A [`Usage`] as its flat C mirror.
fn usage_to_c(u: Usage) -> MediusInput {
    MediusInput {
        kind: class_kind(u.class),
        value: u.id,
    }
}

/// A [`LockTarget`] as its flat C mirror.
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

/// An axis lock target of the given kind (its `usage` field is unused).
fn axis_target(kind: MediusLockTargetKind) -> MediusLockTarget {
    MediusLockTarget {
        kind,
        usage: MediusInput {
            kind: MediusInputKind::Button,
            value: 0,
        },
    }
}

/// `(MediusEmitMode, hz)` -> `EmitPace`. `hz` matters only for `Fixed`.
pub(crate) fn emit_pace_to_medius(mode: MediusEmitMode, hz: u16) -> EmitPace {
    match mode {
        MediusEmitMode::Learned => EmitPace::Learned,
        MediusEmitMode::Interval => EmitPace::Interval,
        MediusEmitMode::Fixed => EmitPace::Fixed(hz),
    }
}

// --- query results: medius -> Medius ---

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
            let (target, is_blanket) = if let Some(class) = e.blanket {
                let target = MediusLockTarget {
                    kind: MediusLockTargetKind::Usage,
                    usage: MediusInput {
                        kind: class_kind(class),
                        value: 0,
                    },
                };
                (target, true)
            } else if let Some(t) = e.target {
                (lock_target_to_c(t), false)
            } else {
                continue;
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

impl From<medius::CatchState> for MediusCatchState {
    fn from(c: medius::CatchState) -> Self {
        MediusCatchState {
            mask: c.mask.bits(),
            dropped: c.dropped,
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
            ClipState::Armed => MediusClipState::Armed,
            ClipState::Playing => MediusClipState::Playing,
            ClipState::Faulted => MediusClipState::Faulted,
        }
    }
}

impl From<ClipStatus> for MediusClipStatus {
    fn from(s: ClipStatus) -> Self {
        let mut held = [MediusInput {
            kind: MediusInputKind::Button,
            value: 0,
        }; MEDIUS_MAX_USAGES];
        let n = s.held.len().min(MEDIUS_MAX_USAGES);
        for (slot, u) in held.iter_mut().zip(s.held.iter()).take(n) {
            *slot = usage_to_c(*u);
        }
        MediusClipStatus {
            state: s.state.into(),
            free: s.free,
            used: s.used,
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
            MediusClipState::Armed => ClipState::Armed,
            MediusClipState::Playing => ClipState::Playing,
            MediusClipState::Faulted => ClipState::Faulted,
        }
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
            used: s.used,
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

// --- stream events: medius -> Medius ---

impl From<CatchEvent> for MediusCatchEvent {
    fn from(e: CatchEvent) -> Self {
        match e {
            CatchEvent::Motion(m) => MediusCatchEvent {
                kind: MediusCatchEventKind::Motion,
                data: MediusCatchEventData {
                    motion: MediusMotionEvent {
                        dx: m.dx,
                        dy: m.dy,
                        dz: m.dz,
                    },
                },
            },
            CatchEvent::Usages(s) => {
                let mut usages = [MediusInput {
                    kind: MediusInputKind::Button,
                    value: 0,
                }; MEDIUS_MAX_USAGES];
                let n = s.usages.len().min(MEDIUS_MAX_USAGES);
                for (slot, u) in usages.iter_mut().zip(s.usages.iter()).take(n) {
                    *slot = usage_to_c(*u);
                }
                MediusCatchEvent {
                    kind: MediusCatchEventKind::Usages,
                    data: MediusCatchEventData {
                        usages: MediusUsageEvent {
                            n: n as u16,
                            usages,
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

// --- mock config + pushed events: Medius -> medius ---

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
                let (target, blanket) = if e.is_blanket {
                    (None, Some(kind_class(e.target.usage.kind)))
                } else {
                    (Some(lock_target_to_medius(e.target)?), None)
                };
                Some(LockEntry {
                    target,
                    blanket,
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
        CatchState {
            mask: CatchMask::from_bits_truncate(c.mask),
            dropped: c.dropped,
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

/// The held usages of a `MediusUsageEvent` as a `Usage` list (for mock injection). Invalid entries (an
/// out-of-range button id) are dropped.
pub(crate) fn usage_event_to_medius(e: &MediusUsageEvent) -> Vec<Usage> {
    let n = (e.n as usize).min(MEDIUS_MAX_USAGES);
    e.usages[..n]
        .iter()
        .filter_map(|&u| input_to_medius(u))
        .collect()
}

// --- frame types (mock recorder) ---

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
            F::Option => MediusFrameType::Option,
            F::ClipAppend => MediusFrameType::ClipAppend,
            F::ClipCtrl => MediusFrameType::ClipCtrl,
        }
    }
}

/// `MediusFrameType` -> `medius::FrameType`; `None` if the value is out of range.
#[cfg(feature = "mock")]
pub(crate) fn frame_type_to_native(t: MediusFrameType) -> Option<medius::FrameType> {
    medius::FrameType::try_from(t as u8).ok()
}

/// `PortInfo` -> `MediusPortInfo`. `None` if the path would not fit (never a half-written string).
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

/// `BoxInfo` -> `MediusBoxInfo`. `None` if the port path would not fit.
pub(crate) fn box_to_medius(b: &BoxInfo) -> Option<MediusBoxInfo> {
    Some(MediusBoxInfo {
        port: port_to_medius(&b.port)?,
        version: b.version.clone().into(), // Version is no longer Copy (it carries the name String)
        device: b.device.clone().into(),
    })
}
