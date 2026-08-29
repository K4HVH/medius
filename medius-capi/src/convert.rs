//! Conversions between the safe `medius` types and the `#[repr(C)]` mirrors.

use std::os::raw::c_char;
use std::time::Duration;

use medius::{
    Action, Axis, Bearing, BearingMode, Blanket, BoxInfo, Button, Caps, CatchEntry, CatchEvent,
    CatchFilter, CatchState, ChipFirmware, Class, ClipState, ClipStatus, ClockDomain,
    ClockEstimate, CountersSnapshot, DeviceInfo, DeviceKind, Direction, EmitPace, EmitPaceStatus,
    FirmwareInfo, Health, ImageState, ImperfectStatus, Input, InputEvent, KbdCaps, Key, LedMode,
    LedTarget, LockEntry, LockScope, LockTarget, Locks, LogLevel, LogLine, MediaKey, Motion,
    MouseCaps, MoveTiming, PendingMotion, PortInfo, Rate, RebootTarget, RenderMode, Stats, Usage, Version,
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

// Every enum crosses this boundary as a byte, because materializing a `#[repr(u8)]` enum from one a
// caller chose is undefined behaviour before any check can run. These are the total maps back, keyed
// on the C ABI's own discriminants; each answers `None` for a byte no constant names.

fn kind_to_c(k: DeviceKind) -> u8 {
    let k = match k {
        DeviceKind::Unknown => MediusDeviceKind::Unknown,
        DeviceKind::Keyboard => MediusDeviceKind::Keyboard,
        DeviceKind::Mouse => MediusDeviceKind::Mouse,
    };
    k as u8
}

#[cfg(feature = "mock")]
pub(crate) fn device_kind_from_c(v: u8) -> Option<DeviceKind> {
    Some(match v {
        0 => DeviceKind::Unknown,
        1 => DeviceKind::Keyboard,
        2 => DeviceKind::Mouse,
        _ => return None,
    })
}

pub(crate) fn action_from_c(v: u8) -> Option<Action> {
    Some(match v {
        0 => Action::SoftRelease,
        1 => Action::Press,
        2 => Action::ForceRelease,
        _ => return None,
    })
}

pub(crate) fn reboot_target_from_c(v: u8) -> Option<RebootTarget> {
    Some(match v {
        0 => RebootTarget::DeviceDownload,
        1 => RebootTarget::HostDownload,
        2 => RebootTarget::DeviceRun,
        3 => RebootTarget::HostRun,
        _ => return None,
    })
}

pub(crate) fn led_target_from_c(v: u8) -> Option<LedTarget> {
    Some(match v {
        0 => LedTarget::Device,
        1 => LedTarget::Host,
        2 => LedTarget::Both,
        _ => return None,
    })
}

pub(crate) fn led_mode_from_c(v: u8) -> Option<LedMode> {
    Some(match v {
        0 => LedMode::Auto,
        1 => LedMode::Off,
        2 => LedMode::Solid,
        3 => LedMode::Blink,
        _ => return None,
    })
}

pub(crate) fn move_timing_from_c(v: u8) -> Option<MoveTiming> {
    Some(match v {
        0 => MoveTiming::Ride,
        1 => MoveTiming::Now,
        _ => return None,
    })
}

pub(crate) fn pending_motion_from_c(v: u8) -> Option<PendingMotion> {
    Some(match v {
        0 => PendingMotion::Keep,
        1 => PendingMotion::Flush,
        2 => PendingMotion::Discard,
        _ => return None,
    })
}

pub(crate) fn axis_from_c(v: u8) -> Option<Axis> {
    Some(match v {
        0 => Axis::X,
        1 => Axis::Y,
        2 => Axis::Wheel,
        _ => return None,
    })
}

pub(crate) fn edge_from_c(v: u8) -> Option<medius::Edge> {
    Some(match v {
        0 => medius::Edge::Both,
        1 => medius::Edge::Press,
        2 => medius::Edge::Release,
        _ => return None,
    })
}

pub(crate) fn clip_action_from_c(v: u8) -> Option<medius::ClipAction> {
    Some(match v {
        0 => medius::ClipAction::Start,
        1 => medius::ClipAction::Stop,
        2 => medius::ClipAction::Pause,
        3 => medius::ClipAction::Resume,
        4 => medius::ClipAction::Restart,
        5 => medius::ClipAction::Toggle,
        _ => return None,
    })
}

#[cfg(feature = "mock")]
pub(crate) fn clip_state_from_c(v: u8) -> Option<ClipState> {
    Some(match v {
        0 => ClipState::Idle,
        1 => ClipState::Playing,
        2 => ClipState::Paused,
        3 => ClipState::Faulted,
        _ => return None,
    })
}

pub(crate) fn clock_domain_from_c(v: u8) -> Option<ClockDomain> {
    Some(match v {
        0 => ClockDomain::HostChip,
        1 => ClockDomain::DeviceChip,
        _ => return None,
    })
}

/// A `MediusRenderMode` to the crate [`RenderMode`].
pub(crate) fn render_from_c(r: MediusRenderMode) -> RenderMode {
    match r {
        MediusRenderMode::Off => RenderMode::Off,
        MediusRenderMode::Stock => RenderMode::Stock,
        MediusRenderMode::Despiked => RenderMode::Despiked,
        MediusRenderMode::Unsmoothed => RenderMode::Unsmoothed,
    }
}

/// A crate [`RenderMode`] to its `MediusRenderMode`.
pub(crate) fn render_to_c(r: RenderMode) -> MediusRenderMode {
    match r {
        RenderMode::Off => MediusRenderMode::Off,
        RenderMode::Stock => MediusRenderMode::Stock,
        RenderMode::Despiked => MediusRenderMode::Despiked,
        RenderMode::Unsmoothed => MediusRenderMode::Unsmoothed,
    }
}

/// `(mode, hz)` to an [`EmitPace`]; `hz` matters only for `Fixed`.
pub(crate) fn emit_pace_from_c(mode: u8, hz: u16) -> Option<EmitPace> {
    Some(match mode {
        0 => EmitPace::Learned,
        1 => EmitPace::Interval,
        2 => EmitPace::Fixed(hz),
        _ => return None,
    })
}

pub(crate) fn motion_from_c(v: MediusMotion) -> Option<Motion> {
    Some(match v.kind {
        0 => Motion::Cursor { dx: v.dx, dy: v.dy },
        1 => Motion::Wheel(v.wheel),
        _ => return None,
    })
}

impl From<Bearing> for MediusBearing {
    fn from(b: Bearing) -> Self {
        MediusBearing {
            window_ms: b
                .window
                .map_or(0, |d| d.as_millis().min(u16::MAX as u128) as u16),
            mode: match b.mode {
                BearingMode::PerAxis => MediusBearingMode::PerAxis,
                BearingMode::Vector => MediusBearingMode::Vector,
            },
        }
    }
}

/// A `MEDIUS_BLANKET_*` byte to a [`Blanket`], or `None` for a value the enum does not name.
pub(crate) fn blanket_from_c(v: u8) -> Option<Blanket> {
    Some(match v {
        0 => Blanket::Aim,
        1 => Blanket::Wheel,
        2 => Blanket::Buttons,
        3 => Blanket::Keys,
        4 => Blanket::Media,
        _ => return None,
    })
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

/// `MediusLockTarget` to [`LockTarget`]; `None` for a `kind` no constant names or a `Usage` target
/// with an out-of-range button id.
pub(crate) fn lock_target_to_medius(v: MediusLockTarget) -> Option<LockTarget> {
    Some(match v.kind {
        0 => LockTarget::Axis(Axis::X),
        1 => LockTarget::Axis(Axis::Y),
        2 => LockTarget::Axis(Axis::Wheel),
        3 => LockTarget::Usage(input_to_medius(v.usage)?),
        _ => return None,
    })
}

/// `MediusUsage` to a [`Usage`]; `None` for a `kind` no constant names, or a button/key id out of
/// range for its class.
pub(crate) fn input_to_medius(v: MediusUsage) -> Option<Usage> {
    Some(match Class::from_u8(v.kind)? {
        Class::Button => Usage::from(Button::from_id(u8::try_from(v.id).ok()?)?),
        Class::Key => Usage::from(Key::new(u8::try_from(v.id).ok()?)),
        Class::Media => Usage::from(MediaKey::new(v.id)),
    })
}

pub(crate) fn usage_to_c(u: Usage) -> MediusUsage {
    MediusUsage {
        kind: u.class.as_u8(),
        id: u.id,
    }
}

fn lock_target_to_c(t: LockTarget) -> MediusLockTarget {
    match t {
        LockTarget::Axis(Axis::X) => axis_target(MediusLockTargetKind::X),
        LockTarget::Axis(Axis::Y) => axis_target(MediusLockTargetKind::Y),
        LockTarget::Axis(Axis::Wheel) => axis_target(MediusLockTargetKind::Wheel),
        LockTarget::Usage(u) => MediusLockTarget {
            kind: MediusLockTargetKind::Usage as u8,
            usage: usage_to_c(u),
        },
    }
}

pub(crate) fn blank_usage() -> MediusUsage {
    MediusUsage {
        kind: MediusClass::Button as u8,
        id: 0,
    }
}

fn axis_target(kind: MediusLockTargetKind) -> MediusLockTarget {
    MediusLockTarget {
        kind: kind as u8,
        usage: blank_usage(),
    }
}

impl From<ChipFirmware> for MediusChipFirmware {
    fn from(c: ChipFirmware) -> Self {
        MediusChipFirmware {
            major: c.major,
            minor: c.minor,
            patch: c.patch,
            slot: c.slot,
            state: match c.state {
                ImageState::New => 0,
                ImageState::PendingVerify => 1,
                ImageState::Valid => 2,
                ImageState::Invalid => 3,
                ImageState::Aborted => 4,
                ImageState::Unknown(v) => v,
            },
        }
    }
}

impl From<FirmwareInfo> for MediusFirmwareInfo {
    fn from(f: FirmwareInfo) -> Self {
        MediusFirmwareInfo {
            device: f.device.into(),
            host_present: u8::from(f.host.is_some()),
            host: f
                .host
                .unwrap_or(ChipFirmware {
                    major: 0,
                    minor: 0,
                    patch: 0,
                    slot: 0xFF,
                    state: ImageState::Unknown(0xFF),
                })
                .into(),
            slot_size: f.slot_size,
            device_staged: u8::from(f.device_staged),
            host_staged: u8::from(f.host_staged),
        }
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
            kind: kind_to_c(m.kind),
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
            direction: MediusDirection::Both as u8,
            scale: MEDIUS_LOCK_SCALE_PASS,
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
                        kind: MediusLockTargetKind::Usage as u8,
                        usage: MediusUsage {
                            kind: class.as_u8(),
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
                direction: e.direction.as_u8(),
                scale: e.scale,
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

/// A [`CatchFilter`] to the C struct, wildcards resolved to their sentinels.
pub(crate) fn catch_filter_to_c(f: CatchFilter) -> MediusCatchFilter {
    let (class, id) = f.wire();
    MediusCatchFilter {
        class,
        id,
        direction: f.direction().as_u8(),
        capture: f.capture().as_u8(),
    }
}

/// The C struct back to a [`CatchFilter`]; `None` when the four values address nothing the box would
/// accept -- an unknown class, an unknown direction, or the wildcard class carrying a real id.
pub(crate) fn catch_filter_from_c(f: MediusCatchFilter) -> Option<CatchFilter> {
    CatchFilter::from_wire(f.class, f.id, f.direction, f.capture)
}

/// A decoded [`InputEvent`] to the C struct. The unused arms are zeroed rather than left undefined:
/// a C caller reading `dx` on a press must see 0, not whatever was on the stack.
pub(crate) fn input_event_to_c(e: InputEvent) -> MediusInputEvent {
    let blank = blank_usage();
    let (kind, usage, dx, dy, dz) = match e.input {
        Input::Press(u) => (MediusInputKind::Press, usage_to_c(u), 0, 0, 0),
        Input::Release(u) => (MediusInputKind::Release, usage_to_c(u), 0, 0, 0),
        Input::Motion { dx, dy, dz } => (MediusInputKind::Motion, blank, dx, dy, dz),
    };
    MediusInputEvent {
        kind,
        ts_us: e.ts_us,
        clock: clock_domain_to_c(e.clock),
        usage,
        dx,
        dy,
        dz,
    }
}

fn clock_estimate_to_c(c: ClockEstimate) -> MediusClockEstimate {
    MediusClockEstimate {
        offset_us: c.offset_us,
        rate_ppb: c.rate_ppb.unwrap_or(MEDIUS_CLOCK_RATE_NONE),
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
        rate_ppb: (c.rate_ppb != MEDIUS_CLOCK_RATE_NONE).then_some(c.rate_ppb),
        delay_us: c.delay_us,
        age: (c.age_ms != MEDIUS_CLOCK_AGE_NONE).then(|| Duration::from_millis(c.age_ms as u64)),
    }
}

impl From<CatchState> for MediusCatchState {
    fn from(c: CatchState) -> Self {
        let blank = MediusCatchEntry {
            filter: catch_filter_to_c(CatchFilter::everything()),
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

fn clip_state_to_c(s: ClipState) -> u8 {
    let s = match s {
        ClipState::Idle => MediusClipState::Idle,
        ClipState::Playing => MediusClipState::Playing,
        ClipState::Paused => MediusClipState::Paused,
        ClipState::Faulted => MediusClipState::Faulted,
    };
    s as u8
}

impl From<ClipStatus> for MediusClipStatus {
    fn from(s: ClipStatus) -> Self {
        let mut held = [blank_usage(); MEDIUS_MAX_USAGES];
        let n = s.held.len().min(MEDIUS_MAX_USAGES);
        for (slot, u) in held.iter_mut().zip(s.held.iter()).take(n) {
            *slot = usage_to_c(*u);
        }
        MediusClipStatus {
            state: clip_state_to_c(s.state),
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

/// Serialize clip settings to the C struct (autolock as a `CLIP_LOCK_*` bitmask, triggers into the fixed array).
pub(crate) fn clip_settings_to_c(s: &medius::ClipSettings) -> MediusClipSettings {
    let mut triggers = [MediusClipTrigger {
        on: blank_usage(),
        edge: MediusEdge::Both as u8,
        action: MediusClipAction::Start as u8,
        consume: 0,
    }; MEDIUS_CLIP_TRIG_MAX];
    let n = s.triggers.len().min(MEDIUS_CLIP_TRIG_MAX);
    for (slot, t) in triggers.iter_mut().zip(s.triggers.iter()).take(n) {
        *slot = MediusClipTrigger {
            on: usage_to_c(t.on),
            edge: edge_to_c(t.edge),
            action: clip_action_to_c(t.action),
            consume: t.consume as u8,
        };
    }
    MediusClipSettings {
        autolock_bits: s.autolock.iter().fold(0u8, |m, b| m | blanket_bit(*b)),
        loop_: s.loop_ as u8,
        retain: s.retain as u8,
        finalized: s.finalized as u8,
        ride: s.ride as u8,
        triggers,
        n: n as u8,
    }
}

fn edge_to_c(e: medius::Edge) -> u8 {
    let e = match e {
        medius::Edge::Both => MediusEdge::Both,
        medius::Edge::Press => MediusEdge::Press,
        medius::Edge::Release => MediusEdge::Release,
    };
    e as u8
}

fn clip_action_to_c(a: medius::ClipAction) -> u8 {
    let a = match a {
        medius::ClipAction::Start => MediusClipAction::Start,
        medius::ClipAction::Stop => MediusClipAction::Stop,
        medius::ClipAction::Pause => MediusClipAction::Pause,
        medius::ClipAction::Resume => MediusClipAction::Resume,
        medius::ClipAction::Restart => MediusClipAction::Restart,
        medius::ClipAction::Toggle => MediusClipAction::Toggle,
    };
    a as u8
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
            Some(medius::ClipTrigger {
                on: input_to_medius(t.on)?,
                edge: edge_from_c(t.edge)?,
                action: clip_action_from_c(t.action)?,
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
        ride: c.ride != 0,
        triggers,
    }
}

/// The C struct back to a [`ClipStatus`]; `None` for a `state` no constant names.
#[cfg(feature = "mock")]
pub(crate) fn clip_status_from_c(s: MediusClipStatus) -> Option<ClipStatus> {
    let n = (s.held_n as usize).min(MEDIUS_MAX_USAGES);
    let held = s.held[..n]
        .iter()
        .filter_map(|&u| input_to_medius(u))
        .collect();
    Some(ClipStatus {
        state: clip_state_from_c(s.state)?,
        free: s.free,
        total: s.total,
        played: s.played,
        ticks: s.ticks,
        underruns: s.underruns,
        overruns: s.overruns,
        seq_gaps: s.seq_gaps,
        held,
    })
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
            render: render_to_c(s.render),
            fixed_hz,
            resolved_hz: s.resolved_hz,
            force_hz: s.force_hz.unwrap_or(0),
            advertised_hz: s.advertised_hz,
            force_active: s.force_active as u8,
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
                let mut usages = [blank_usage(); MEDIUS_MAX_USAGES];
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
                            class: s.class.as_u8(),
                            direction: s.direction.as_u8(),
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
                            direction: t.direction.as_u8(),
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

/// The C struct back to a [`DeviceInfo`]; `None` for a `kind` no constant names.
#[cfg(feature = "mock")]
pub(crate) fn device_info_from_c(m: MediusDeviceInfo) -> Option<DeviceInfo> {
    Some(DeviceInfo {
        vid: m.vid,
        pid: m.pid,
        bcd_device: m.bcd_device,
        bcd_usb: m.bcd_usb,
        has_serial: nz(m.has_serial),
        has_bos: nz(m.has_bos),
        kind: device_kind_from_c(m.kind)?,
        product: read_cstr(&m.product),
    })
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
                    LockScope::Blanket(Class::from_u8(e.target.usage.kind)?)
                } else {
                    LockScope::Target(lock_target_to_medius(e.target)?)
                };
                Some(LockEntry {
                    scope,
                    direction: Direction::from_u8(e.direction)?,
                    scale: e.scale,
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
            F::Update => MediusFrameType::Update,
            F::UpdateResp => MediusFrameType::UpdateResp,
        }
    }
}

/// A `MEDIUS_FRAME_TYPE_*` byte to a `medius::FrameType`; `None` if no constant names it.
#[cfg(feature = "mock")]
pub(crate) fn frame_type_from_c(t: u8) -> Option<medius::FrameType> {
    medius::FrameType::try_from(t).ok()
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
