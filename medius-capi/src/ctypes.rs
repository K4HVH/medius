//! `#[repr(C)]` mirror types. Flat PODs, sized so the wire protocol's bounds never truncate.

use std::os::raw::c_char;

/// Largest number of held usages in one catch snapshot.
pub const MEDIUS_MAX_USAGES: usize = 256;
/// Largest number of entries in a decoded `RESP(LOCKS)`.
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

/// Largest CATCH subscription table the box holds (the firmware `CTRL_CATCH_MAXN`).
pub const MEDIUS_MAX_CATCH_ENTRIES: usize = 32;
/// Largest traffic payload one event carries (the firmware `CTRL_TRAFFIC_DATA_MAX`).
pub const MEDIUS_MAX_TRAFFIC_BYTES: usize = 180;

/// CATCH classes, the `class` of a `MediusCatchFilter`. 0-3 are the classes `LOCK` and `INJECT`
/// address; 4-10 are the traffic the box relays.
pub const MEDIUS_CATCH_CLASS_BTN: u8 = 0;
pub const MEDIUS_CATCH_CLASS_KEY: u8 = 1;
pub const MEDIUS_CATCH_CLASS_MEDIA: u8 = 2;
pub const MEDIUS_CATCH_CLASS_AXIS: u8 = 3;
/// Raw HID input report bytes, keyed by interface number.
pub const MEDIUS_CATCH_CLASS_HID_IN: u8 = 4;
/// Interrupt-OUT report bytes the PC wrote, keyed by endpoint address.
pub const MEDIUS_CATCH_CLASS_HID_OUT: u8 = 5;
/// Vendor-interface interrupt traffic, keyed by endpoint address.
pub const MEDIUS_CATCH_CLASS_VENDOR_INTERRUPT: u8 = 6;
/// Vendor-interface bulk traffic, keyed by endpoint address.
pub const MEDIUS_CATCH_CLASS_VENDOR_BULK: u8 = 7;
/// A proxied control transaction, keyed by endpoint number (0 = EP0).
pub const MEDIUS_CATCH_CLASS_CONTROL: u8 = 8;
/// The bytes the clone put on the wire, keyed by endpoint address.
pub const MEDIUS_CATCH_CLASS_EMIT: u8 = 9;
/// Bus lifecycle: reset, suspend, configuration and interface changes, attach and detach.
pub const MEDIUS_CATCH_CLASS_BUS: u8 = 10;
/// Wildcard: every class.
pub const MEDIUS_CATCH_CLASS_ANY: u8 = 0xFF;
/// Wildcard: every id within a class.
pub const MEDIUS_CATCH_ID_ANY: u16 = 0xFFFF;

/// `MediusClockEstimate::age_ms` when the box has no estimate yet.
pub const MEDIUS_CLOCK_AGE_NONE: u32 = u32::MAX;

/// `MediusClockEstimate::rate_ppb` when the box has fitted no drift rate.
pub const MEDIUS_CLOCK_RATE_NONE: i32 = i32::MIN;

/// A keyboard key, addressed by HID Keyboard/Keypad usage. Modifiers are `0xE0..=0xE7`.
pub type MediusKey = u8;
/// A media key, addressed by 16-bit HID Consumer usage.
pub type MediusMediaKey = u16;
/// A CATCH class, one of the `MEDIUS_CATCH_CLASS_*` values.
pub type MediusCatchClass = u8;

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
pub enum MediusDirection {
    /// Both edges, both signs, or both flows; on a `LOCK` scale the two fixed signs, with the
    /// relative pair passing.
    Both = 0,
    Positive = 1,
    Negative = 2,
    /// The axis sign the box is currently injecting. Measured against the bearing, so the sign it covers follows the
    /// injection rather than the axis; inert while no bearing is live. Axes only.
    With = 3,
    /// The axis sign opposing the box's injection. Measured against the bearing; axes only.
    Against = 4,
}

/// How the box decides whether physical motion runs with or against its own injection.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediusBearingMode {
    /// Each axis compares its own sign against its own bearing, independently.
    PerAxis = 0,
    /// The physical delta is projected onto the injected XY vector. One
    /// relative scale governs both axes, the lower of X's and Y's, and that is what reads back.
    /// Each axis's absolute scale then applies to what the projection left, not to the sign the report
    /// carried: it governs what reaches the PC.
    Vector = 1,
}

/// The configured bearing: what `MEDIUS_DIRECTION_WITH` / `_AGAINST` are measured against.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediusBearing {
    /// How long the last injected delta's direction stays the bearing, in ms. 0 = never, which
    /// leaves the relative directions inert whatever their scale.
    pub window_ms: u16,
    pub mode: MediusBearingMode,
}

/// `LOCK` scale: percent of the physical value kept. 0 blocks, 100 passes it untouched, above 100
/// amplifies, to 255 (2.55x).
pub const MEDIUS_LOCK_SCALE_BLOCK: u8 = 0;
pub const MEDIUS_LOCK_SCALE_PASS: u8 = 100;
pub const MEDIUS_LOCK_SCALE_MAX: u8 = 255;
/// The bearing window the box holds before any host sets one, in ms.
pub const MEDIUS_BEARING_WINDOW_DEFAULT_MS: u16 = 20;

/// A whole input group for a blanket lock or a clip auto-lock scope.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediusBlanket {
    Aim = 0,
    Wheel = 1,
    Buttons = 2,
    Keys = 3,
    Media = 4,
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
    TrafficEvent = 0x16,
    Option = 0x11,
    ClipAppend = 0x12,
    ClipCtrl = 0x13,
    ClipSet = 0x14,
    ClipTrigger = 0x15,
    Update = 0x17,
    UpdateResp = 0x18,
}

/// Which arm of a [`MediusCatchEvent`] is populated.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediusCatchEventKind {
    /// A relative-axis event (`data.motion`).
    Motion = 0,
    /// A held-usage snapshot for one class (`data.usages`).
    Usages = 1,
    /// Byte-oriented traffic (`data.traffic`).
    Traffic = 2,
}

/// Which chip's clock stamped an event.
///
/// The two chips boot independently, so nothing relates their timers: a stamp is only meaningful
/// against another from the same domain. To place both on one timeline, apply
/// `MediusClockEstimate::offset_us` and respect its error bound.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediusClockDomain {
    /// The device-facing chip: everything the real device produced.
    HostChip = 0,
    /// The PC-facing chip: everything the PC produced and everything the clone emitted.
    DeviceChip = 1,
}

/// The class of a [`MediusUsage`] (button / key / media).
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediusClass {
    Button = 0,
    Key = 1,
    Media = 2,
}

/// A relative axis. Values match the wire axis id a `CATCH` or `LOCK` entry carries.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediusAxis {
    X = 0,
    Y = 1,
    Wheel = 2,
}

/// When a delta reaches the game PC, against movement riding.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediusMoveTiming {
    Ride = 0,
    Now = 1,
}

/// What a move does to the motion already held for a ride.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediusPendingMotion {
    Keep = 0,
    Flush = 1,
    Discard = 2,
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

/// A momentary usage for `medius_device_inject`; build with the `medius_usage_*` helpers.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediusUsage {
    /// A `MEDIUS_CLASS_*` value.
    pub kind: u8,
    pub id: u16,
}

/// Which edge of a trigger usage fires its binding (matches the lock direction wire values).
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediusEdge {
    Both = 0,
    Press = 1,
    Release = 2,
}

/// The engine action a trigger binding drives.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediusClipAction {
    Start = 0,
    Stop = 1,
    Pause = 2,
    Resume = 3,
    Restart = 4,
    Toggle = 5,
}

/// One clip trigger binding: `on`'s `edge` drives `action`; `consume` suppresses the input from the game.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediusClipTrigger {
    pub on: MediusUsage,
    /// A `MEDIUS_EDGE_*` value.
    pub edge: u8,
    /// A `MEDIUS_CLIP_ACTION_*` value.
    pub action: u8,
    pub consume: u8,
}

/// A relative-axis drive for `medius_device_move_axis`; build with the `medius_motion_*` helpers.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediusMotion {
    /// A `MEDIUS_MOTION_KIND_*` value.
    pub kind: u8,
    pub dx: i16,
    pub dy: i16,
    pub wheel: i16,
}

/// A lock target: an axis (`kind` is `X`/`Y`/`Wheel`) or a momentary usage (`kind` is `Usage`, read `usage`).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediusLockTarget {
    /// A `MEDIUS_LOCK_TARGET_KIND_*` value.
    pub kind: u8,
    pub usage: MediusUsage,
}

/// The cloned device's primary kind, from its Boot-interface protocol.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediusDeviceKind {
    Unknown = 0,
    Keyboard = 1,
    Mouse = 2,
}

/// Decoded firmware version; `mac` is the device chip's base MAC, a stable per-box identity.
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

/// One chip's firmware version and which of its two app slots it booted.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediusChipFirmware {
    pub major: u8,
    pub minor: u8,
    pub patch: u8,
    /// 0 = ota_0, 1 = ota_1.
    pub slot: u8,
    /// 0 new, 1 pending-verify, 2 valid, 3 invalid, 4 aborted, 0xFF unknown.
    pub state: u8,
}

/// Both chips' firmware state (`RESP(FIRMWARE)`).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediusFirmwareInfo {
    pub device: MediusChipFirmware,
    /// 0 when the host chip has not answered over the inter-chip link; `host` is then meaningless.
    pub host_present: u8,
    pub host: MediusChipFirmware,
    /// Usable bytes in a spare slot; the same on both chips.
    pub slot_size: u32,
    pub device_staged: u8,
    pub host_staged: u8,
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

/// The cloned device's USB identity, primary kind, and product string.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct MediusDeviceInfo {
    pub vid: u16,
    pub pid: u16,
    pub bcd_device: u16,
    pub bcd_usb: u16,
    pub has_serial: u8,
    pub has_bos: u8,
    /// A `MEDIUS_DEVICE_KIND_*` value.
    pub kind: u8,
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

/// One entry in a decoded `RESP(LOCKS)`: the locked target and which edges are locked.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediusLockEntry {
    pub target: MediusLockTarget,
    pub is_blanket: bool,
    /// A `MEDIUS_DIRECTION_*` value: which direction of the target this entry weighs.
    /// A byte rather than `MediusDirection`, so the boundary can validate it before anything reads it
    /// as one; C++ renders the enum as `enum : uint8_t`, so assigning this to a `MediusDirection`
    /// there needs a cast.
    pub direction: u8,
    /// Percent of the physical value kept: 0 blocks, 100 passes, above 100 amplifies. A momentary
    /// usage carries one bit, so the box stores the block or pass it renders and one never reports a
    /// value in between.
    ///
    /// This is the figure the box applies, not the byte it was sent: in `MEDIUS_BEARING_MODE_VECTOR`
    /// one relative scale governs the whole aim, the lower of X's and Y's, and both relative entries
    /// carry that number.
    pub scale: u8,
}

/// The active locks: `entries[0..n]`. Use `medius_locks_is_locked` to test a target/direction.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct MediusLocks {
    pub n: u16,
    pub entries: [MediusLockEntry; MEDIUS_MAX_LOCKS],
}

/// One CATCH subscription entry: what to observe, in which direction, and how much of each packet to
/// keep. Build one with a `medius_catch_filter_*` helper.
///
/// The box resolves each event to its most specific matching entry -- an exact `(class, id)` beats a
/// class blanket, which beats the everything filter, and a named direction beats `Both` -- and that
/// entry supplies `capture`. The wildcards are sentinels rather than a separate flag:
/// `class = MEDIUS_CATCH_CLASS_ANY` matches every class and `id = MEDIUS_CATCH_ID_ANY` every id
/// within one. The wildcard class with a real id addresses nothing and is refused.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediusCatchFilter {
    /// One of `MEDIUS_CATCH_CLASS_*`, or `MEDIUS_CATCH_CLASS_ANY`.
    pub class: MediusCatchClass,
    /// The class-specific id, or `MEDIUS_CATCH_ID_ANY`.
    pub id: u16,
    /// A `MEDIUS_DIRECTION_*` value: the press/release edge on the momentary classes, the sign of
    /// the delta on axes, and IN (`Positive`) / OUT (`Negative`) on the traffic classes. A byte no
    /// constant names is refused at subscribe time.
    /// A byte rather than `MediusDirection`, so the boundary can validate it before anything reads it
    /// as one; C++ renders the enum as `enum : uint8_t`, so assigning this to a `MediusDirection`
    /// there needs a cast.
    pub direction: u8,
    /// Bytes kept per event; 0 keeps the whole packet. Traffic classes only -- an input class carries
    /// no packet, and naming one with a non-zero capture is refused at subscribe time.
    pub capture: u8,
}

/// One row of a `MediusCatchState`: a live subscription and what it has lost.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediusCatchEntry {
    pub filter: MediusCatchFilter,
    /// Events this entry could not queue. Per entry, because a box-wide count says you are losing
    /// events but not which ones, and those are different problems.
    pub dropped: u16,
}

/// The measured difference between the two chips' clocks, from `RESP(CATCH)`.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediusClockEstimate {
    /// The host chip's clock minus the device chip's, in microseconds.
    pub offset_us: i32,
    /// Relative drift between the two crystals in parts per billion, or `MEDIUS_CLOCK_RATE_NONE`
    /// when the box has fitted none. That is a different answer from a fitted 0, which says the two
    /// crystals match: on a link too busy for enough clean exchanges no fit is made at all, which is
    /// exactly when assuming no drift is least safe.
    pub rate_ppb: i32,
    /// Best measured round trip in the window. The offset is good to about half of this.
    pub delay_us: u16,
    /// Age of the estimate, or `MEDIUS_CLOCK_AGE_NONE` when the box has no estimate yet. The
    /// sentinel is load-bearing: an offset that was never measured also reads as zero, and applying
    /// it would silently shift every cross-domain stamp.
    pub age_ms: u32,
}

/// Decoded `RESP(CATCH)`: the live subscription table in `entries[0..n]`, its drop counts, and the
/// measured inter-chip clock estimate.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediusCatchState {
    /// The box refused an entry because its table is full.
    pub table_full: u8,
    /// Box-wide events dropped under back-pressure.
    pub dropped: u32,
    pub clock: MediusClockEstimate,
    /// The number of valid entries in `entries`.
    pub n: u16,
    pub entries: [MediusCatchEntry; MEDIUS_MAX_CATCH_ENTRIES],
}

/// Imperfect-clone opt-in and over-capacity status (each field is 0 or 1).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediusImperfectStatus {
    pub allowed: u8,
    pub over_capacity: u8,
    pub clone_imperfect: u8,
}

/// Emit-rate pacing mode plus the rate in effect and the rate the clone advertises.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediusEmitPaceStatus {
    pub mode: MediusEmitMode,
    /// 1 when the renderer composes onto `mode`, gating every 1 ms frame tick.
    pub rendered: u8,
    pub fixed_hz: u16,
    pub resolved_hz: u16,
    /// The forced wire rate requested, in Hz; 0 leaves the device's own.
    pub force_hz: u16,
    /// What the clone's input endpoints advertise now, in Hz; 0 = no clone.
    pub advertised_hz: u16,
    /// 1 when a forced interval is written into the descriptor being served.
    pub force_active: u8,
}

/// The device-side clip lifecycle state (`medius_clip_query_status`).
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediusClipState {
    /// No clip playing (empty, or a loaded clip parked at its start).
    Idle = 0,
    /// Draining the ring, one entry per native frame.
    Playing = 1,
    /// Halted mid-clip; the cursor and any held usages are retained.
    Paused = 2,
    /// An append was dropped or the ring overflowed; recover with `medius_clip_clear`.
    Faulted = 3,
}

/// A snapshot of the device-side clip ring and playback counters (the runtime view of `RESP(CLIP)`).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediusClipStatus {
    /// A `MEDIUS_CLIP_STATE_*` value.
    pub state: u8,
    pub free: u32,
    /// The retained clip size in bytes (streaming: buffered-but-undrained bytes).
    pub total: u32,
    /// Bytes played from the clip start (retained progress; ~0 while streaming).
    pub played: u32,
    pub ticks: u32,
    pub underruns: u16,
    pub overruns: u16,
    pub seq_gaps: u16,
    pub held_n: u16,
    pub held: [MediusUsage; MEDIUS_MAX_USAGES],
}

/// The max clip trigger bindings in a `MediusClipSettings` (matches the firmware `CLIP_TRIG_MAX`).
pub const MEDIUS_CLIP_TRIG_MAX: usize = 8;

/// The clip configuration read back from `RESP(CLIP)`: autolock scope, loop/retain scalars, and triggers.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct MediusClipSettings {
    /// The autolock scope as `CLIP_LOCK_*` bits (`medius_clip_set_autolock`).
    pub autolock_bits: u8,
    pub loop_: u8,
    pub retain: u8,
    pub finalized: u8,
    /// Whether the clip's motion waits to ride a native report (`medius_clip_set_ride`).
    pub ride: u8,
    pub triggers: [MediusClipTrigger; MEDIUS_CLIP_TRIG_MAX],
    /// The number of valid entries in `triggers`.
    pub n: u8,
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

/// One relative-axis catch event: the user's real motion at the merge point, before lock suppression or injection.
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

/// One held-usage snapshot: every held usage of one class in `usages[0..n]`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct MediusUsageEvent {
    /// Which class this snapshot is of, one of `MEDIUS_CLASS_*`. Carried here rather than read off
    /// the first entry, because the snapshot that most needs it is the one with `n == 0`: releasing
    /// the last held usage is the edge a caller waits for, and it lists nothing to read a class from.
    pub class: u8,
    /// A `MEDIUS_DIRECTION_*` value: the edge that produced this snapshot, the subscribed set having
    /// grown (`Positive`) or shrunk (`Negative`). Without it a direction on an input filter cannot be
    /// honoured at all.
    /// A byte rather than `MediusDirection`, so the boundary can validate it before anything reads it
    /// as one; C++ renders the enum as `enum : uint8_t`, so assigning this to a `MediusDirection`
    /// there needs a cast.
    pub direction: u8,
    pub n: u16,
    pub usages: [MediusUsage; MEDIUS_MAX_USAGES],
}

/// One byte-oriented catch event: HID reports, vendor endpoints, control transactions, the bytes the
/// clone emitted, or bus lifecycle. `bytes[0..len]` is as much of the packet as `capture` kept.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediusTrafficEvent {
    /// One of `MEDIUS_CATCH_CLASS_*`.
    pub class: MediusCatchClass,
    /// Endpoint address, interface number, or endpoint number, per the class.
    pub id: u16,
    /// A `MEDIUS_DIRECTION_*` value: `Positive` is IN (device to PC), `Negative` is OUT.
    /// A byte rather than `MediusDirection`, so the boundary can validate it before anything reads it
    /// as one; C++ renders the enum as `enum : uint8_t`, so assigning this to a `MediusDirection`
    /// there needs a cast.
    pub direction: u8,
    /// Class-specific; read it with `medius_traffic_event_control_status` or `..._bus_event`.
    pub flags: u8,
    /// The packet's length before `capture` truncated it.
    pub true_len: u16,
    /// Valid bytes in `bytes`.
    pub len: u16,
    pub bytes: [u8; MEDIUS_MAX_TRAFFIC_BYTES],
}

/// What the real device answered a proxied control transaction with.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediusControlStatus {
    Ok = 0,
    Stalled = 1,
    Naked = 2,
    /// A status byte this build does not know. Read `MediusTrafficEvent::flags` for its value. Kept
    /// distinct rather than folded into the nearest known one: a catch-all arm reported a future
    /// firmware's new status as a timeout, which reads as a device fault that never happened.
    Other = 3,
}

/// What a `MEDIUS_CATCH_CLASS_BUS` event describes.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediusBusEventKind {
    Reset = 0,
    Suspend = 1,
    Resume = 2,
    /// `SET_CONFIGURATION` selected `configuration`.
    Configured = 3,
    Deconfigured = 4,
    /// `SET_INTERFACE` selected `alt` on `interface`.
    SetInterface = 5,
    DeviceAttached = 6,
    DeviceDetached = 7,
    CloneUp = 8,
    CloneDown = 9,
}

/// A decoded bus lifecycle event; the payload fields are 0 for the kinds that carry none.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediusBusEvent {
    pub kind: MediusBusEventKind,
    pub configuration: u8,
    pub interface: u8,
    pub alt: u8,
}

/// The populated arm of a [`MediusCatchEvent`]; read the field matching the event's `kind`.
#[repr(C)]
#[derive(Clone, Copy)]
pub union MediusCatchEventData {
    pub motion: MediusMotionEvent,
    pub usages: MediusUsageEvent,
    pub traffic: MediusTrafficEvent,
}

/// One catch-stream event. Read `data.motion` / `data.usages` / `data.traffic` per `kind`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct MediusCatchEvent {
    pub kind: MediusCatchEventKind,
    /// When the event was stamped, in the `clock` chip's microseconds. A box-local clock, unrelated
    /// to any clock on this machine, so only meaningful compared against other events of the same
    /// domain. It wraps every ~71.6 minutes and restarts at zero if that chip reboots, so a value
    /// below the previous one is a wrap, a reboot, or a domain change, and the delta is meaningless.
    pub ts_us: u32,
    /// Which chip's clock stamped `ts_us`.
    pub clock: MediusClockDomain,
    pub data: MediusCatchEventData,
}

/// Which arm of a [`MediusInputEvent`] is populated.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediusInputKind {
    /// A momentary usage went down; read `usage`.
    Press = 0,
    /// A momentary usage came up; read `usage`.
    Release = 1,
    /// A relative-motion report; read `dx`/`dy`/`dz`.
    Motion = 2,
}

/// One decoded input event: a press or release edge, or a motion report. The held-usage snapshots the
/// box sends are diffed into these by `medius_device_input_events`.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediusInputEvent {
    pub kind: MediusInputKind,
    /// The report's arrival stamp, in the `clock` chip's microseconds.
    pub ts_us: u32,
    /// Which chip's clock stamped it; always `HostChip` for physical input.
    pub clock: MediusClockDomain,
    /// The usage this is an edge on; unset for `Motion`.
    pub usage: MediusUsage,
    /// Relative X this report (right positive); 0 unless `kind` is `Motion`.
    pub dx: i16,
    /// Relative Y this report (down positive); 0 unless `kind` is `Motion`.
    pub dy: i16,
    /// Wheel delta this report (up positive); 0 unless `kind` is `Motion`.
    pub dz: i16,
}

/// One event placed on this machine's clock by a `MediusTimeline`.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediusStamped {
    /// When the event happened, on the same monotonic clock the caller passed as `now_ns`.
    pub host_ns: u64,
    /// The event's own stamp, unwrapped past the 32-bit rollover.
    pub box_us: u64,
    /// How much later than the measured floor this event reached the caller. Jitter, not latency.
    pub excess_ns: u64,
}

/// One device log line. `text` is NUL-terminated.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct MediusLogLine {
    pub level: MediusLogLevel,
    pub text: [c_char; MEDIUS_MAX_LOG_TEXT],
}
