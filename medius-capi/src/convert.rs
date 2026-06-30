//! Conversions between the safe `medius` types and the `#[repr(C)]` mirrors.
//!
//! `From<MediusX> for medius::X` handles command parameters; `From<medius::X> for MediusX` handles
//! query results and stream events. Both directions are concrete-to-concrete, so the orphan rule
//! permits the foreign-for-local impls.

use std::os::raw::c_char;

use medius::{
    Action, Blanket, Button, Caps, CatchEvent, CatchMask, CatchState, CountersSnapshot, Health,
    ImperfectStatus, Input, KbdCaps, Key, KeyboardEvent, LedMode, LedTarget, LockDirection,
    LockTarget, Locks, LogLevel, LogLine, MediaEvent, MediaKey, Motion, MouseCaps, MouseEvent,
    MouseInfo, PortInfo, Rate, RebootTarget, Stats, Version,
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

impl From<MediusLockTarget> for LockTarget {
    fn from(v: MediusLockTarget) -> Self {
        match v.kind {
            MediusLockTargetKind::X => LockTarget::X,
            MediusLockTargetKind::Y => LockTarget::Y,
            MediusLockTargetKind::Wheel => LockTarget::Wheel,
            MediusLockTargetKind::Button => LockTarget::Button(v.button.into()),
        }
    }
}

impl From<MediusMotion> for Motion {
    fn from(v: MediusMotion) -> Self {
        match v.kind {
            MediusMotionKind::Cursor => Motion::Cursor { dx: v.dx, dy: v.dy },
            MediusMotionKind::Wheel => Motion::Wheel(v.wheel),
        }
    }
}

/// `MediusInput` -> `Input`. `None` when an `Input::Button` carries an out-of-range button id.
pub(crate) fn input_to_medius(v: MediusInput) -> Option<Input> {
    Some(match v.kind {
        MediusInputKind::Button => Input::Button(Button::from_id(v.value as u8)?),
        MediusInputKind::Key => Input::Key(Key::new(v.value as u8)),
        MediusInputKind::Media => Input::Media(MediaKey::new(v.value)),
    })
}

// --- query results: medius -> Medius ---

impl From<Version> for MediusVersion {
    fn from(v: Version) -> Self {
        MediusVersion {
            proto_ver: v.proto_ver,
            fw_major: v.fw_major,
            fw_minor: v.fw_minor,
            fw_patch: v.fw_patch,
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

impl From<MouseInfo> for MediusMouseInfo {
    fn from(m: MouseInfo) -> Self {
        MediusMouseInfo {
            vid: m.vid,
            pid: m.pid,
            bcd_device: m.bcd_device,
            bcd_usb: m.bcd_usb,
            has_serial: b(m.has_serial),
            has_bos: b(m.has_bos),
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
        MediusLocks { mask: l.mask() }
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

impl From<MouseEvent> for MediusMouseEvent {
    fn from(e: MouseEvent) -> Self {
        MediusMouseEvent {
            buttons: e.buttons,
            dx: e.dx,
            dy: e.dy,
            wheel: e.wheel,
        }
    }
}

impl From<&KeyboardEvent> for MediusKeyboardEvent {
    fn from(e: &KeyboardEvent) -> Self {
        let mut keys = [0u8; MEDIUS_MAX_KEYS];
        // The count is a u8; cap at 255 so it can never wrap (the wire list is u8-prefixed anyway).
        let n = e.keys.len().min(u8::MAX as usize);
        for (slot, k) in keys.iter_mut().zip(e.keys.iter()).take(n) {
            *slot = k.usage();
        }
        MediusKeyboardEvent {
            modifiers: e.modifiers,
            n_keys: n as u8,
            keys,
        }
    }
}

impl From<&MediaEvent> for MediusMediaEvent {
    fn from(e: &MediaEvent) -> Self {
        let mut keys = [0u16; MEDIUS_MAX_MEDIA_KEYS];
        let n = e.keys.len().min(u8::MAX as usize);
        for (slot, k) in keys.iter_mut().zip(e.keys.iter()).take(n) {
            *slot = k.usage();
        }
        MediusMediaEvent {
            n_keys: n as u8,
            keys,
        }
    }
}

impl From<CatchEvent> for MediusCatchEvent {
    fn from(e: CatchEvent) -> Self {
        match e {
            CatchEvent::Mouse(m) => MediusCatchEvent {
                kind: MediusCatchEventKind::Mouse,
                data: MediusCatchEventData { mouse: m.into() },
            },
            CatchEvent::Keyboard(k) => MediusCatchEvent {
                kind: MediusCatchEventKind::Keyboard,
                data: MediusCatchEventData {
                    keyboard: (&k).into(),
                },
            },
            CatchEvent::Media(md) => MediusCatchEvent {
                kind: MediusCatchEventKind::Media,
                data: MediusCatchEventData {
                    media: (&md).into(),
                },
            },
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

impl From<MediusMouseInfo> for MouseInfo {
    fn from(m: MediusMouseInfo) -> Self {
        MouseInfo {
            vid: m.vid,
            pid: m.pid,
            bcd_device: m.bcd_device,
            bcd_usb: m.bcd_usb,
            has_serial: nz(m.has_serial),
            has_bos: nz(m.has_bos),
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
        Locks::from_mask(l.mask)
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

impl From<MediusMouseEvent> for MouseEvent {
    fn from(e: MediusMouseEvent) -> Self {
        MouseEvent {
            buttons: e.buttons,
            dx: e.dx,
            dy: e.dy,
            wheel: e.wheel,
        }
    }
}

impl From<&MediusKeyboardEvent> for KeyboardEvent {
    fn from(e: &MediusKeyboardEvent) -> Self {
        let n = (e.n_keys as usize).min(MEDIUS_MAX_KEYS);
        KeyboardEvent {
            modifiers: e.modifiers,
            keys: e.keys[..n].iter().map(|&u| Key::new(u)).collect(),
        }
    }
}

impl From<&MediusMediaEvent> for MediaEvent {
    fn from(e: &MediusMediaEvent) -> Self {
        let n = (e.n_keys as usize).min(MEDIUS_MAX_MEDIA_KEYS);
        MediaEvent {
            keys: e.keys[..n].iter().map(|&u| MediaKey::new(u)).collect(),
        }
    }
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
            F::MouseEvent => MediusFrameType::MouseEvent,
            F::KbEvent => MediusFrameType::KbEvent,
            F::ConsEvent => MediusFrameType::ConsEvent,
            F::Option => MediusFrameType::Option,
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
    Some(MediusPortInfo {
        path,
        vid: p.vid,
        pid: p.pid,
    })
}
