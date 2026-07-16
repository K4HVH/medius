//! `#[repr(C)]` mirror types. Flat PODs, sized so the wire protocol's bounds never truncate.

use std::os::raw::c_char;

/// Largest number of held usages in one catch snapshot. The wire format length-prefixes the list with a
/// `u8`, so 256 can never truncate.
pub const MEDIUS_MAX_USAGES: usize = 256;
/// Largest number of entries in a decoded `RESP(LOCKS)`. Every distinct axis, usage, and whole-class
/// blanket lock across all classes fits well within this.
pub const MEDIUS_MAX_LOCKS: usize = 256;
/// Capacity for a log line's text (the wire payload is at most 512 bytes).
pub const MEDIUS_MAX_LOG_TEXT: usize = 512;
/// Capacity for a discovered serial-port path.
pub const MEDIUS_MAX_PATH: usize = 512;
/// Capacity for a cloned device's product string (the wire caps it at 127 bytes).
pub const MEDIUS_MAX_PRODUCT: usize = 128;
/// Capacity for the box name string (the wire caps it at 32 bytes; +1 for the NUL terminator).
pub const MEDIUS_MAX_NAME: usize = 33;
/// Capacity for a control adapter's serial string.
pub const MEDIUS_MAX_SERIAL: usize = 128;

/// CATCH subscription class bits, OR them together (see `medius_device_catch_events`).
pub const MEDIUS_CATCH_MASK_MOTION: u8 = 0x01;
pub const MEDIUS_CATCH_MASK_WHEEL: u8 = 0x02;
pub const MEDIUS_CATCH_MASK_BUTTONS: u8 = 0x04;
pub const MEDIUS_CATCH_MASK_KEYS: u8 = 0x08;
pub const MEDIUS_CATCH_MASK_MEDIA: u8 = 0x10;
pub const MEDIUS_CATCH_MASK_ALL: u8 = 0x1F;

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

/// A whole input group for a blanket lock or a clip auto-lock scope.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediusBlanket {
    Keys = 0,
    Media = 1,
    Buttons = 2,
    Aim = 3,
    Wheel = 4,
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
    MotionEvent = 0x0C,
    UsageEvent = 0x0F,
    Option = 0x11,
    ClipAppend = 0x12,
    ClipCtrl = 0x13,
}

/// Which arm of a [`MediusCatchEvent`] is populated.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediusCatchEventKind {
    /// A relative-axis event (`data.motion`).
    Motion = 0,
    /// A held-usage snapshot for one class (`data.usages`).
    Usages = 1,
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

/// What a lock addresses: a relative axis, or a momentary usage (button/key/media).
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediusLockTargetKind {
    /// The X cursor axis.
    X = 0,
    /// The Y cursor axis.
    Y = 1,
    /// The wheel.
    Wheel = 2,
    /// A momentary usage; read `usage`.
    Usage = 3,
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

/// Playback options for a clip start or catch trigger (`medius_clip_start` / `_arm_catch`). The single
/// place clip settings live; extensible as more are added. `autolock` points at `autolock_len`
/// `MediusBlanket` groups the clip auto-locks while playing (NULL / 0 = no auto-lock).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct MediusClipConfig {
    pub autolock: *const MediusBlanket,
    pub autolock_len: usize,
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

/// A lock target: an axis (`kind` is `X`/`Y`/`Wheel`) or a momentary usage (`kind` is `Usage`, read
/// `usage`). A button, key, and media usage all lock the same way. Build with the `medius_lock_target_*`
/// helpers.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediusLockTarget {
    pub kind: MediusLockTargetKind,
    pub usage: MediusInput,
}

// --- value (query result) structs ---

/// The cloned device's primary kind, from its Boot-interface protocol.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediusDeviceKind {
    Unknown = 0,
    Keyboard = 1,
    Mouse = 2,
}

/// Decoded firmware version. `mac` is the device chip's base MAC — a stable per-box identity. `name` is
/// the box's NUL-terminated human-readable name (its readable partner to `mac`), never empty (the
/// firmware synthesizes a `Medius-XXXX` default when no custom name is set).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediusVersion {
    pub proto_ver: u8,
    pub fw_major: u8,
    pub fw_minor: u8,
    pub fw_patch: u8,
    pub mac: [u8; 6],
    pub name: [c_char; MEDIUS_MAX_NAME],
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

/// The cloned device's USB identity, primary kind, and product string. `product` is a NUL-terminated
/// UTF-8 C string (empty when the device serves none).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct MediusDeviceInfo {
    pub vid: u16,
    pub pid: u16,
    pub bcd_device: u16,
    pub bcd_usb: u16,
    pub has_serial: u8,
    pub has_bos: u8,
    pub kind: MediusDeviceKind,
    pub product: [c_char; MEDIUS_MAX_PRODUCT],
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

/// One entry in a decoded `RESP(LOCKS)`: the locked target and which edges are locked. When `is_blanket`
/// is set the lock covers a whole class (every button / key / media usage) — `target.usage.kind` names
/// the class and its `value` is unused.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediusLockEntry {
    pub target: MediusLockTarget,
    pub is_blanket: bool,
    pub positive: bool,
    pub negative: bool,
}

/// The active locks: `entries[0..n]`. Use `medius_locks_is_locked` to test a target/direction.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct MediusLocks {
    pub n: u16,
    pub entries: [MediusLockEntry; MEDIUS_MAX_LOCKS],
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

/// The device-side clip lifecycle state (`medius_clip_status`).
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediusClipState {
    /// No clip active.
    Idle = 0,
    /// A catch-trigger is armed; playback starts on the physical button edge.
    Armed = 1,
    /// Draining the ring, one entry per native frame.
    Playing = 2,
    /// An append was dropped or the ring overflowed; stop and re-preload.
    Faulted = 3,
}

/// A snapshot of the device-side clip ring and playback counters. `free`/`used` pace top-ups;
/// `state == Faulted` means re-sync (stop + rebuild).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediusClipStatus {
    pub state: MediusClipState,
    pub free: u32,
    pub used: u32,
    pub ticks: u32,
    pub underruns: u16,
    pub overruns: u16,
    pub seq_gaps: u16,
    pub held: u8,
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
    /// The control adapter's serial (NUL-terminated); empty and `has_serial == 0` when it serves none.
    pub serial: [c_char; MEDIUS_MAX_SERIAL],
    pub has_serial: u8,
}

/// One discovered box: its control port, firmware version (with the box MAC), and the device it clones.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct MediusBoxInfo {
    pub port: MediusPortInfo,
    pub version: MediusVersion,
    pub device: MediusDeviceInfo,
}

// --- catch-stream snapshots ---

/// One relative-axis catch event: the user's real motion at the merge point, before any lock suppression
/// or injection.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediusMotionEvent {
    /// Relative X this report (right positive).
    pub dx: i16,
    /// Relative Y this report (down positive).
    pub dy: i16,
    /// Wheel delta this report (up positive).
    pub dz: i16,
}

/// One held-usage snapshot: every held usage of one class (button / key / media; modifiers are key
/// usages `0xE0..=0xE7`) in `usages[0..n]`. A mouse-button press and a key press have the same shape.
/// Diff successive snapshots for edges, or use `medius_usage_event_is_held`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct MediusUsageEvent {
    pub n: u16,
    pub usages: [MediusInput; MEDIUS_MAX_USAGES],
}

/// The populated arm of a [`MediusCatchEvent`]; read the field matching the event's `kind`.
#[repr(C)]
#[derive(Clone, Copy)]
pub union MediusCatchEventData {
    pub motion: MediusMotionEvent,
    pub usages: MediusUsageEvent,
}

/// One catch-stream event. Read `data.motion` / `data.usages` per `kind`.
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
