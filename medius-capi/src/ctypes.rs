//! `#[repr(C)]` mirror types. Flat PODs, sized so the wire protocol's bounds never truncate.

use std::os::raw::c_char;

/// Largest number of pressed keys / active media usages in one catch snapshot. The wire format
/// length-prefixes both lists with a `u8`, so 256 can never truncate.
pub const MEDIUS_MAX_KEYS: usize = 256;
/// See [`MEDIUS_MAX_KEYS`].
pub const MEDIUS_MAX_MEDIA_KEYS: usize = 256;
/// Capacity for a log line's text (the wire payload is at most 512 bytes).
pub const MEDIUS_MAX_LOG_TEXT: usize = 512;
/// Capacity for a discovered serial-port path.
pub const MEDIUS_MAX_PATH: usize = 512;

/// CATCH subscription class bits, OR them together (see `medius_device_catch_events`).
pub const MEDIUS_CATCH_MASK_MOTION: u8 = 0x01;
pub const MEDIUS_CATCH_MASK_WHEEL: u8 = 0x02;
pub const MEDIUS_CATCH_MASK_BUTTONS: u8 = 0x04;
pub const MEDIUS_CATCH_MASK_KEYS: u8 = 0x08;
pub const MEDIUS_CATCH_MASK_ALL: u8 = 0x0F;

/// A keyboard key, addressed by HID Keyboard/Keypad usage. Modifiers are `0xE0..=0xE7`.
pub type MediusKey = u8;
/// A media key, addressed by 16-bit HID Consumer usage.
pub type MediusMediaKey = u16;
/// A CATCH subscription mask, an OR of the `MEDIUS_CATCH_MASK_*` bits.
pub type MediusCatchMask = u8;

// --- parameter enums (repr(u8); discriminants are the wire bytes) ---

/// A mouse button. Values match the firmware button id.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediusButton {
    Left = 0,
    Right = 1,
    Middle = 2,
    Side1 = 3,
    Side2 = 4,
}

/// An injection override action.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediusAction {
    SoftRelease = 0,
    Press = 1,
    ForceRelease = 2,
}

/// A reboot target chip + mode.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediusRebootTarget {
    DeviceDownload = 0,
    HostDownload = 1,
    DeviceRun = 2,
    HostRun = 3,
}

/// What paces injected motion.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediusEmitMode {
    Learned = 0,
    Interval = 1,
    Fixed = 2,
}

/// Which status LED a command addresses.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediusLedTarget {
    Device = 0,
    Host = 1,
    Both = 2,
}

/// LED drive mode.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediusLedMode {
    Auto = 0,
    Off = 1,
    Solid = 2,
    Blink = 3,
}

/// Which edge of an axis/button a lock applies to.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediusLockDirection {
    Both = 0,
    Positive = 1,
    Negative = 2,
}

/// A whole input class for a blanket lock.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediusBlanket {
    Keys = 0,
    Media = 1,
    Buttons = 2,
}

/// A device log line's severity.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediusLogLevel {
    Error = 0,
    Warn = 1,
    Info = 2,
    Debug = 3,
    Verbose = 4,
}

/// A wire frame type (the `TYPE` byte). Used with the mock recorder.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediusFrameType {
    Move = 0x01,
    Inject = 0x03,
    Reset = 0x04,
    Query = 0x05,
    Resp = 0x06,
    RebootDl = 0x07,
    Log = 0x08,
    Led = 0x09,
    Lock = 0x0A,
    Catch = 0x0B,
    MouseEvent = 0x0C,
    KbEvent = 0x0F,
    ConsEvent = 0x10,
    Option = 0x11,
}

/// Which arm of a [`MediusCatchEvent`] is populated.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediusCatchEventKind {
    Mouse = 0,
    Keyboard = 1,
    Media = 2,
}

/// Which arm of a [`MediusInput`] is populated.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediusInputKind {
    Button = 0,
    Key = 1,
    Media = 2,
}

/// Which arm of a [`MediusMotion`] is populated.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediusMotionKind {
    Cursor = 0,
    Wheel = 1,
}

/// Which axis/button a lock targets.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediusLockTargetKind {
    X = 0,
    Y = 1,
    Wheel = 2,
    Button = 3,
}

// --- data-carrying parameter structs ---

/// A momentary usage for `medius_device_inject`. `value` holds the button id, key usage, or media
/// usage depending on `kind`. Build with the `medius_input_*` helpers.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediusInput {
    pub kind: MediusInputKind,
    pub value: u16,
}

/// A relative-axis drive for `medius_device_move_axis`. For `Cursor`, `dx`/`dy` apply; for `Wheel`,
/// `wheel` applies. Build with the `medius_motion_*` helpers.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediusMotion {
    pub kind: MediusMotionKind,
    pub dx: i16,
    pub dy: i16,
    pub wheel: i16,
}

/// A lock target. `button` is meaningful only when `kind` is `Button`.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediusLockTarget {
    pub kind: MediusLockTargetKind,
    pub button: MediusButton,
}

// --- value (query result) structs ---

/// Decoded firmware version.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediusVersion {
    pub proto_ver: u8,
    pub fw_major: u8,
    pub fw_minor: u8,
    pub fw_patch: u8,
}

/// Box health flags (each field is 0 or 1).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediusHealth {
    pub link_up: u8,
    pub mouse_attached: u8,
    pub clone_configured: u8,
    pub injection_active: u8,
    pub rate_confident: u8,
    pub lock_on: u8,
    pub catch_on: u8,
    pub kbd_attached: u8,
}

/// Mouse half of the cloned device's capabilities.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediusMouseCaps {
    pub n_buttons: u8,
    pub has_x: u8,
    pub has_y: u8,
    pub has_wheel: u8,
    pub has_report_id: u8,
    pub n_hid: u8,
}

/// Keyboard half of the cloned device's capabilities. `n_keys == 0xFF` signals an NKRO bitmap.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediusKbdCaps {
    pub n_keys: u8,
    pub nkro: u8,
    pub has_consumer: u8,
    pub has_system: u8,
    pub has_report_id: u8,
}

/// The whole cloned device's capabilities.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediusCaps {
    pub mouse: MediusMouseCaps,
    pub keyboard: MediusKbdCaps,
    pub mouse_change_driven: u8,
    pub kbd_change_driven: u8,
}

/// The cloned mouse's USB identity.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediusMouseInfo {
    pub vid: u16,
    pub pid: u16,
    pub bcd_device: u16,
    pub bcd_usb: u16,
    pub has_serial: u8,
    pub has_bos: u8,
}

/// The live native report rate and clone poll period.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediusRate {
    pub native_period_us: u16,
    pub poll_period_us: u16,
    pub confident: u8,
    pub change_driven: u8,
}

/// Box-side delivery/telemetry counters.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediusStats {
    pub inject_emits: u32,
    pub tx_drops: u16,
    pub tx_merges: u16,
    pub tx_maxdepth: u8,
    pub tx_wedges: u8,
    pub wakeups: u16,
    pub reset_count: u16,
    pub config_count: u16,
}

/// The active lock bitmask. Use `medius_locks_is_locked` to test a target/direction.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediusLocks {
    pub mask: u16,
}

/// The active catch subscription mask plus the box-side dropped-event count.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediusCatchState {
    pub mask: u8,
    pub dropped: u32,
}

/// Imperfect-clone opt-in and over-capacity status (each field is 0 or 1).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediusImperfectStatus {
    pub allowed: u8,
    pub over_capacity: u8,
    pub clone_imperfect: u8,
}

/// Emit-rate pacing mode plus the rate in effect. `fixed_hz` is the rate requested for `Fixed` (0
/// otherwise); `resolved_hz` is the ceiling actually in effect (0 = learnt/adaptive).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediusEmitPaceStatus {
    pub mode: MediusEmitMode,
    pub fixed_hz: u16,
    pub resolved_hz: u16,
}

/// Host-side always-on counters.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediusCountersSnapshot {
    pub frames_tx: u64,
    pub frames_rx: u64,
    pub crc_drops: u64,
    pub reconnects: u64,
}

/// A discovered medius serial port. `path` is NUL-terminated.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct MediusPortInfo {
    pub path: [c_char; MEDIUS_MAX_PATH],
    pub vid: u16,
    pub pid: u16,
}

// --- catch-stream snapshots ---

/// One physical mouse report. `buttons` is a bitmask by button id.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediusMouseEvent {
    pub buttons: u8,
    pub dx: i16,
    pub dy: i16,
    pub wheel: i16,
}

/// One physical keyboard snapshot: a modifier bitmap plus the pressed non-modifier keycodes in
/// `keys[0..n_keys]`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct MediusKeyboardEvent {
    pub modifiers: u8,
    pub n_keys: u8,
    pub keys: [u8; MEDIUS_MAX_KEYS],
}

/// One physical media snapshot: the active Consumer usages in `keys[0..n_keys]`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct MediusMediaEvent {
    pub n_keys: u8,
    pub keys: [u16; MEDIUS_MAX_MEDIA_KEYS],
}

/// The populated arm of a [`MediusCatchEvent`]; read the field matching the event's `kind`.
#[repr(C)]
#[derive(Clone, Copy)]
pub union MediusCatchEventData {
    pub mouse: MediusMouseEvent,
    pub keyboard: MediusKeyboardEvent,
    pub media: MediusMediaEvent,
}

/// One catch-stream event. Read `data.mouse` / `data.keyboard` / `data.media` per `kind`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct MediusCatchEvent {
    pub kind: MediusCatchEventKind,
    pub data: MediusCatchEventData,
}

/// One device log line. `text` is NUL-terminated.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct MediusLogLine {
    pub level: MediusLogLevel,
    pub text: [c_char; MEDIUS_MAX_LOG_TEXT],
}
