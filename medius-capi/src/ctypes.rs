//! `#[repr(C)]` mirror types: flat PODs, sized so nothing the wire carries is truncated.

use std::os::raw::c_char;

/// Most held usages in one catch snapshot.
pub const MEDIUS_MAX_USAGES: usize = 256;
/// Most entries in a decoded `RESP(LOCKS)`.
pub const MEDIUS_MAX_LOCKS: usize = 256;
/// Log line text capacity (the wire payload is <=512 bytes).
pub const MEDIUS_MAX_LOG_TEXT: usize = 512;
/// Discovered serial-port path capacity.
pub const MEDIUS_MAX_PATH: usize = 512;
/// Cloned device product string capacity (the wire caps it at 127 bytes).
pub const MEDIUS_MAX_PRODUCT: usize = 128;
/// Box name capacity: the wire's 32 bytes plus the NUL.
pub const MEDIUS_MAX_NAME: usize = 33;
/// Control adapter serial string capacity.
pub const MEDIUS_MAX_SERIAL: usize = 128;

/// Box CATCH subscription table size (firmware `CTRL_CATCH_MAXN`).
pub const MEDIUS_MAX_CATCH_ENTRIES: usize = 32;
/// Most traffic payload bytes per event (firmware `CTRL_TRAFFIC_DATA_MAX`).
pub const MEDIUS_MAX_TRAFFIC_BYTES: usize = 180;

/// Most rows in a decoded `RESP(REWRITE)` (firmware `REWRITE_TAB_MAX`).
pub const MEDIUS_MAX_REWRITE_ENTRIES: usize = 32;
/// Most rows in a decoded `RESP(PATCHES)` (firmware `PATCH_MAX`).
pub const MEDIUS_MAX_PATCH_ENTRIES: usize = 16;
/// Most entries in a decoded `RESP(TRANSFORMS)` (firmware `CTRL_TRANSFORM_MAXN`).
pub const MEDIUS_MAX_TRANSFORM_ENTRIES: usize = 32;
/// Most `match`/`mask` bytes one rewrite rule compares (firmware `REWRITE_MATCH_MAX`).
pub const MEDIUS_MAX_REWRITE_MATCH: usize = 16;
/// Rewrite payload bytes the box holds across all rules (firmware `REWRITE_POOL`).
pub const MEDIUS_REWRITE_PAYLOAD_POOL: usize = 2048;

// Literals: cbindgen cannot constant-fold another crate's value into a `#define`.
const _: () = {
    assert!(MEDIUS_MAX_CATCH_ENTRIES == medius::CATCH_MAX_ENTRIES);
    assert!(MEDIUS_MAX_REWRITE_ENTRIES == medius::REWRITE_MAX_ENTRIES);
    assert!(MEDIUS_MAX_PATCH_ENTRIES == medius::PATCH_MAX_ENTRIES);
    assert!(MEDIUS_MAX_TRANSFORM_ENTRIES == medius::TRANSFORM_MAX_ENTRIES);
    assert!(MEDIUS_MAX_REWRITE_MATCH == medius::REWRITE_MATCH_MAX);
    assert!(MEDIUS_REWRITE_PAYLOAD_POOL == medius::REWRITE_PAYLOAD_POOL);
    assert!(MEDIUS_MAX_DEV_PAYLOAD == medius::MAX_PAYLOAD);
    assert!(MEDIUS_MAX_TRANSFER_OUT == medius::TRANSFER_OUT_MAX);
};
/// Largest byte payload one control-link frame carries (`MAX_PAYLOAD`): the bound on a
/// `medius_device_raw` write, a rewrite payload and a descriptor patch.
pub const MEDIUS_MAX_DEV_PAYLOAD: usize = 512;
/// Most OUT data one `medius_device_transfer` carries: one frame less its header.
pub const MEDIUS_MAX_TRANSFER_OUT: usize = 501;
/// Longest reply a control transfer brings back: wLength 65535 rounded up to whole 64-byte packets.
pub const MEDIUS_MAX_TRANSFER_REPLY: usize = 65536;

/// CATCH classes (`MediusCatchFilter::class`). 0-3 are the `LOCK` and `INJECT` classes; 4-11 are
/// byte-oriented traffic.
pub const MEDIUS_CATCH_CLASS_BTN: u8 = 0;
pub const MEDIUS_CATCH_CLASS_KEY: u8 = 1;
pub const MEDIUS_CATCH_CLASS_MEDIA: u8 = 2;
pub const MEDIUS_CATCH_CLASS_AXIS: u8 = 3;
/// Raw HID input report bytes, keyed by interface number.
pub const MEDIUS_CATCH_CLASS_HID_IN: u8 = 4;
/// Interrupt-OUT report bytes the PC wrote, keyed by endpoint number, direction OUT.
pub const MEDIUS_CATCH_CLASS_HID_OUT: u8 = 5;
/// Vendor-interface interrupt traffic, keyed by endpoint number and direction.
pub const MEDIUS_CATCH_CLASS_VENDOR_INTERRUPT: u8 = 6;
/// Vendor-interface bulk traffic, keyed by endpoint number and direction.
pub const MEDIUS_CATCH_CLASS_VENDOR_BULK: u8 = 7;
/// A control transaction the game PC received, keyed by endpoint number (0 = EP0); on EP0, the
/// requests relayed to the real device, not the ones the clone serves itself.
pub const MEDIUS_CATCH_CLASS_CONTROL: u8 = 8;
/// Bytes the clone emitted, keyed by endpoint number, direction IN.
pub const MEDIUS_CATCH_CLASS_EMIT: u8 = 9;
/// Bus lifecycle: reset, suspend, configuration and interface changes, attach and detach.
pub const MEDIUS_CATCH_CLASS_BUS: u8 = 10;
/// A control transfer a clip ran against the real device, keyed by endpoint number (0 = EP0).
pub const MEDIUS_CATCH_CLASS_CLIP_TRANSFER: u8 = 11;
/// Wildcard: every class.
pub const MEDIUS_CATCH_CLASS_ANY: u8 = 0xFF;
/// Wildcard: every id within a class.
pub const MEDIUS_CATCH_ID_ANY: u16 = 0xFFFF;

/// `MediusClockEstimate::age_ms` when the box has no estimate yet.
pub const MEDIUS_CLOCK_AGE_NONE: u32 = u32::MAX;

/// `MediusClockEstimate::rate_ppb` when the box has fitted no drift rate.
pub const MEDIUS_CLOCK_RATE_NONE: i32 = i32::MIN;

/// A keyboard key as a HID Keyboard/Keypad usage; modifiers are `0xE0..=0xE7`.
pub type MediusKey = u8;
/// A media key as a 16-bit HID Consumer usage.
pub type MediusMediaKey = u16;
/// A CATCH class: a `MEDIUS_CATCH_CLASS_*` value.
pub type MediusCatchClass = u8;

/// A mouse button; values are the firmware button ids.
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

/// Reboot target: chip and mode.
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

/// Motion render texture: `Off` is the paced fill; the others render the learned native report
/// texture and differ only in the path smoother.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediusRenderMode {
    Off = 0,
    Stock = 1,
    Despiked = 2,
    Unsmoothed = 3,
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
    /// Both edges, signs or flows. A `LOCK` scale on `Both` goes to the two fixed signs, and the
    /// relative pair gets a full pass.
    Both = 0,
    Positive = 1,
    Negative = 2,
    /// Axis sign along the bearing (the injected direction), not a fixed axis sign. Inert while no
    /// bearing is live. Axes only.
    With = 3,
    /// Axis sign opposing the bearing (the injected direction), not a fixed axis sign. Inert while
    /// no bearing is live. Axes only.
    Against = 4,
}

/// How the box reads whether native motion runs with or against injected motion.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediusBearingMode {
    /// Each axis compares its sign against its bearing.
    PerAxis = 0,
    /// Projects the native delta onto the injected XY vector. One relative scale governs both
    /// axes: the lower of X's and Y's, which is also what reads back. Each axis's absolute scale
    /// then applies to what the projection left on that axis, not to the sign the report carried
    /// (an absolute scale governs what is emitted).
    Vector = 1,
}

/// The configured bearing: what `MEDIUS_DIRECTION_WITH` / `_AGAINST` are measured against.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediusBearing {
    /// How long, in ms, the last injected delta's direction stays the bearing. 0 = off: the
    /// relative directions are inert whatever their scale.
    pub window_ms: u16,
    pub mode: MediusBearingMode,
}

/// `LOCK` scale: percent of the native value kept. 0 blocks, 100 passes, >100 amplifies up to 255
/// (2.55x), and a negative scale reverses what it keeps, down to `MEDIUS_LOCK_SCALE_MIN`.
pub const MEDIUS_LOCK_SCALE_BLOCK: i16 = 0;
pub const MEDIUS_LOCK_SCALE_PASS: i16 = 100;
pub const MEDIUS_LOCK_SCALE_MAX: i16 = 255;
/// Most negative `LOCK` scale: `-100` inverts, `-50` keeps half, reversed. Axes only, since a
/// momentary usage carries one bit.
// A literal, not a negation, so the header's `#define` needs no parentheses in a C expression.
pub const MEDIUS_LOCK_SCALE_MIN: i16 = -255;
const _: () = assert!(MEDIUS_LOCK_SCALE_MIN == -MEDIUS_LOCK_SCALE_MAX);
/// Bearing window before any host sets one, in ms.
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

/// A wire frame type (the `TYPE` byte), for the mock recorder.
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
    // v3.4.0 advanced control layer (§3.14) and field transforms (§3.15).
    Raw = 0x19,
    Transfer = 0x1A,
    TransferResp = 0x1B,
    Rewrite = 0x1C,
    Patch = 0x1D,
    Transform = 0x1E,
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
/// The chips boot independently and their timers are unrelated: compare a stamp only with one from
/// the same domain. For one timeline, apply `MediusClockEstimate::offset_us` within its error
/// bound.
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

/// A relative axis; values are the wire axis ids in `CATCH` and `LOCK` entries.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediusAxis {
    X = 0,
    Y = 1,
    Wheel = 2,
    /// AC Pan (horizontal scroll), a full peer of the wheel.
    Pan = 3,
}

/// Whether a delta obeys movement riding or bypasses it.
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
    /// AC Pan (horizontal scroll); read `pan`.
    Pan = 2,
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
    /// AC Pan (horizontal scroll).
    Pan = 3,
    /// A momentary usage; read `usage`.
    Usage = 4,
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

/// The clip action a `MediusClipTrigger` or `MediusClipPacketTrigger` drives.
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

/// One clip input trigger: `on`'s `edge` drives `action`; `consume` suppresses the input from the
/// game. The other trigger kind is `MediusClipPacketTrigger`.
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
    pub pan: i16,
}

/// A lock target: an axis (`kind` is `X`/`Y`/`Wheel`/`Pan`) or a momentary usage (`kind` is `Usage`,
/// read `usage`).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediusLockTarget {
    /// A `MEDIUS_LOCK_TARGET_KIND_*` value.
    pub kind: u8,
    pub usage: MediusUsage,
}

/// The cloned device's primary kind, from its HID report descriptors and Boot declarations.
///
/// A device with both kinds reads keyboard if it has a full key bitmap (unless it declares Boot for
/// both) or declares Boot for its keyboard only. Otherwise it reads mouse.
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

/// One chip's firmware version and booted app slot.
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

/// The detail of an `ErrUpdate`: the update op the box refused and its status. If
/// `medius_device_wait_firmware_confirmed` runs out of time, it gives op 0 and `0x1B` (on
/// probation): the refusal the next update would meet.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediusUpdateError {
    /// The op: 0 begin, 1 data, 2 end, 3 abort, 4 activate.
    pub op: u8,
    /// The update status byte (protocol §4.16), such as 0x1B for a chip on probation.
    pub status: u8,
    /// The status argument; what it holds depends on the status (§4.16).
    pub arg: u32,
}

/// Both chips' firmware state (`RESP(FIRMWARE)`).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediusFirmwareInfo {
    pub device: MediusChipFirmware,
    /// 0 when the host chip has not replied over the inter-chip link; `host` is then meaningless.
    pub host_present: u8,
    pub host: MediusChipFirmware,
    /// Usable bytes per spare slot, the same on both chips.
    pub slot_size: u32,
    pub device_staged: u8,
    pub host_staged: u8,
}

/// Box health flags, each 0 or 1.
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
    /// The rewrite-rule table (§3.14) is non-empty (v3.4.0).
    pub rewrite_on: u8,
    /// The clone serves a patched descriptor set (§3.14) (v3.4.0).
    pub patch_on: u8,
    /// A field transform is active (v3.4.0).
    pub transform_on: u8,
}

/// Cloned device mouse capabilities.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediusMouseCaps {
    pub n_buttons: u8,
    pub has_x: u8,
    pub has_y: u8,
    pub has_wheel: u8,
    /// AC Pan (horizontal scroll) present.
    pub pan: u8,
    pub has_report_id: u8,
    pub n_hid: u8,
}

/// Cloned device keyboard capabilities; `n_keys == 0xFF` means an NKRO bitmap.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediusKbdCaps {
    pub n_keys: u8,
    pub nkro: u8,
    pub has_consumer: u8,
    pub has_system: u8,
    pub has_report_id: u8,
}

/// Cloned device capabilities.
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

/// Native report rate measured off the live report stream, and the clone's poll period.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediusRate {
    pub native_period_us: u16,
    pub poll_period_us: u16,
    pub confident: u8,
    pub change_driven: u8,
}

/// Box-side delivery and telemetry counters.
///
/// Narrowed fields saturate rather than wrap. The three drop counters are full width and do not
/// saturate, so a count keeps rising while loss continues.
///
/// `tx_drops`, `link_rx_drops` and `host_rx_drops` count lost input and should read 0.
/// `relay_drops` counts relayed traffic and commands that went no further, never native input. A
/// rise across a burst of commands can mean some never reached the box. A lost release can leave a
/// key held. `medius_device_reapply` re-sends only what is held, so send the releases again or call
/// `medius_device_reset`.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediusStats {
    pub inject_emits: u32,
    /// Reports the clone's TX queue could not hold, never emitted to the game PC. Should stay 0.
    pub tx_drops: u16,
    pub tx_merges: u16,
    pub tx_maxdepth: u8,
    pub tx_wedges: u8,
    pub wakeups: u16,
    pub reset_count: u16,
    pub config_count: u16,
    /// Input-carrying frames the device chip could not take off the inter-chip link: each a mouse
    /// report or injected delta never emitted. Should stay 0.
    pub link_rx_drops: u32,
    /// The same count for the host chip, relayed over the link.
    pub host_rx_drops: u32,
    /// Relayed traffic and commands that went no further. Counted apart from `tx_drops` because no
    /// native input goes missing with it. Each of these counts:
    ///
    /// - a vendor IN packet that found its endpoint's queue full of the control PC's RAW packets
    /// - a native zero-length packet answering a poll the PC did not make (the one poll the box
    ///   runs ahead of a suspended PC)
    /// - an OUT packet the box could not queue, that failed 3 times on the real device's bus, or
    ///   that a bus reset or SET_INTERFACE overtook
    /// - a RAW OUT packet with no room to wait
    /// - a relayed request or a flagged MOVE the box could not queue
    /// - any frame other than native input that either chip's link receive ring dropped
    /// - a frame from the control PC the box's receive ring had no room for (1 per frame)
    /// - an overrun of the control port's UART (1 per overrun, however many frames it cost)
    pub relay_drops: u32,
    /// Times the box released host-set session state. 0 at boot; wraps, so compare for inequality.
    pub session: u16,
}

/// One entry in a decoded `RESP(LOCKS)`: the locked target and which edges are locked.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediusLockEntry {
    pub target: MediusLockTarget,
    pub is_blanket: bool,
    /// A `MEDIUS_DIRECTION_*` value: the direction of the target this entry weighs. A byte, not
    /// `MediusDirection`, so the boundary can validate it; C++ (`enum : uint8_t`) needs a cast to
    /// assign it to one.
    pub direction: u8,
    /// Percent of the native value kept: 0 blocks, 100 passes, >100 amplifies, and a negative scale
    /// reverses what it keeps. A momentary usage carries one bit, so the box stores a block or a
    /// pass and never reports a value between.
    ///
    /// This is the figure the box applies, which can differ from the one sent. In
    /// `MEDIUS_BEARING_MODE_VECTOR` one relative scale (the lower of X's and Y's) governs both
    /// axes, and both relative entries carry it.
    pub scale: i16,
}

/// The active locks: `entries[0..n]`. Use `medius_locks_is_locked` to test a target/direction.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct MediusLocks {
    pub n: u16,
    pub entries: [MediusLockEntry; MEDIUS_MAX_LOCKS],
}

/// One CATCH subscription entry: what to observe, which direction, and how much of each packet to
/// keep. Build with a `medius_catch_filter_*` helper.
///
/// Each event resolves to its most specific matching entry, which supplies `capture`: an exact
/// `(class, id)` outranks a class blanket, which outranks the everything filter, and a named
/// direction outranks `Both`. The wildcards are sentinels: `class = MEDIUS_CATCH_CLASS_ANY` matches
/// every class and `id = MEDIUS_CATCH_ID_ANY` every id within one. The wildcard class with a real
/// id addresses nothing and is refused.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediusCatchFilter {
    /// One of `MEDIUS_CATCH_CLASS_*`, or `MEDIUS_CATCH_CLASS_ANY`.
    pub class: MediusCatchClass,
    /// The class-specific id, or `MEDIUS_CATCH_ID_ANY`.
    pub id: u16,
    /// A `MEDIUS_DIRECTION_*` value: press/release edge on momentary classes, delta sign on axes,
    /// IN (`Positive`) / OUT (`Negative`) on traffic classes. An unnamed byte is refused at
    /// subscribe time. A byte, not `MediusDirection`, so the boundary can validate it; C++ (`enum :
    /// uint8_t`) needs a cast to assign it to one.
    pub direction: u8,
    /// Bytes kept per event; 0 keeps the whole packet. Traffic classes only: an input class carries
    /// no packet, so a non-zero capture on one is refused at subscribe time.
    pub capture: u8,
}

/// One `MediusCatchState` row: a live subscription and its drop count.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediusCatchEntry {
    pub filter: MediusCatchFilter,
    /// Events this entry could not queue.
    pub dropped: u16,
}

/// Measured offset between the two chips' clocks, from `RESP(CATCH)`.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediusClockEstimate {
    /// The host chip's clock minus the device chip's, in microseconds.
    pub offset_us: i32,
    /// Relative crystal drift in parts per billion, or `MEDIUS_CLOCK_RATE_NONE` when the box has
    /// fitted none (as on a link too busy for enough clean exchanges). A fitted 0 means the
    /// crystals match.
    pub rate_ppb: i32,
    /// Best round trip in the window; the offset is good to ~half of it.
    pub delay_us: u16,
    /// Estimate age, or `MEDIUS_CLOCK_AGE_NONE` before the first estimate. An unmeasured offset
    /// reads 0, and applying it would shift every cross-domain stamp.
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
    /// Valid entries in `entries`.
    pub n: u16,
    pub entries: [MediusCatchEntry; MEDIUS_MAX_CATCH_ENTRIES],
}

/// Imperfect-clone opt-in and over-capacity status, each 0 or 1.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediusImperfectStatus {
    pub allowed: u8,
    pub over_capacity: u8,
    pub clone_imperfect: u8,
}

// Advanced control layer (§3.14): raw injection, control transfers, rewrite rules, descriptor
// patches. Gated on `medius_device_allow_imperfect_clones`.

/// A traffic class a rewrite rule addresses (§3.14): the write-direction `CATCH` classes the box
/// rewrites, as the `class` byte of a `MediusRewriteRule`/`MediusRewriteEntry`. `Any` is the wire
/// wildcard `0xFF`: `Pass`/`Patch`/`Replace` only, applied at every surface a packet crosses.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediusRewriteClass {
    HidIn = 4,
    HidOut = 5,
    VendorInterrupt = 6,
    VendorBulk = 7,
    Control = 8,
    Emit = 9,
    Any = 0xFF,
}

/// What the top-ranked matching rewrite rule does to the packet (§3.14), as the `action` byte.
/// `Drop` is report surfaces only; `Answer`/`Stall`/`Nak` and the two reply rewrites are
/// control-only, matching the box's check.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediusRewriteAction {
    Pass = 0,
    Drop = 1,
    Patch = 2,
    Replace = 3,
    Answer = 4,
    Stall = 5,
    Nak = 6,
    ReplyPatch = 7,
    ReplyReplace = 8,
}

/// Which descriptor a patch overwrites (§3.14), as the `section` byte of a
/// `MediusPatch`/`MediusPatchEntry`.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediusPatchSection {
    Device = 0,
    Config = 1,
    Report = 2,
    String = 3,
    Bos = 4,
}

/// How a control transfer ended (§3.14): the `status` byte of a `MediusTransferOutcome`. An unnamed
/// byte is a status this build does not know, carried through verbatim.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediusTransferStatus {
    /// Completed.
    Ok = 0x00,
    /// The game PC's transfer held the pipe until the timeout and nothing was sent (stage
    /// `SETUP`); or the game PC's next request on that endpoint, or a bus event it caused, took the
    /// pipe in the stage named.
    Busy = 0xF9,
    /// The game PC has suspended the clone, and with it the real device: nothing was sent.
    Suspended = 0xFA,
    /// The real device's packets were corrupt past three tries, or ran past the buffer the host
    /// asked for; or the host chip had no channel free to send it on.
    Error = 0xFB,
    /// Never sent: the opt-in is off, `ep` is a control endpoint the real device does not declare,
    /// or another transfer is outstanding.
    Refused = 0xFC,
    /// The real device STALLed, in the stage named.
    Stall = 0xFD,
    /// The real device NAKed until the timeout, in the stage named; or the host chip's reply was
    /// lost on the inter-chip link.
    NoAnswer = 0xFE,
    /// No device attached.
    NoDevice = 0xFF,
}

/// Where a control transfer ended (§3.14): the `stage` byte of a `MediusTransferOutcome`. An
/// unnamed byte is a stage this build does not know, carried through verbatim.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediusTransferStage {
    Setup = 0,
    Data = 1,
    /// Where a completed transfer ends.
    Status = 2,
}

/// A USB setup packet: the eight `<BBHHH>` little-endian bytes of `bmRequestType`, `bRequest`,
/// `wValue`, `wIndex`, `wLength` (USB spec §9.3). `length` is the data stage: bytes to read for IN,
/// the OUT data length otherwise.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediusSetup {
    pub request_type: u8,
    pub request: u8,
    pub value: u16,
    pub index: u16,
    pub length: u16,
}

/// How `medius_device_transfer` ended. The reply goes to the caller's buffer: the IN data stage as
/// the box received it, partial data before a STALL included, and past wLength where the real
/// device sent more than was asked for in the packets a host asks for; for an OUT transfer or one
/// with wLength 0, what the real device sent where the status stage was due (at most one packet,
/// usually none). It is meaningful on any status. A non-OK `status` is a protocol outcome, not a
/// link error.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediusTransferOutcome {
    /// One of `MEDIUS_TRANSFER_STATUS_*`, or an unknown status carried through. A byte, not
    /// `MediusTransferStatus`, so it can carry unnamed values; C++ (`enum : uint8_t`) needs a cast
    /// to compare it to one.
    pub status: u8,
    /// One of `MEDIUS_TRANSFER_STAGE_*`, or an unknown stage carried through, as `status` is.
    pub stage: u8,
    /// The reply's length. Bytes past the buffer's capacity were not written.
    pub len: usize,
}

/// A rewrite rule (§3.14), keyed by `(class, id, direction, match, mask)`.
///
/// `match_bytes[0..match_len]` and `mask[0..mask_len]` are one length; the box compares the packet
/// head byte for byte under `mask`. An empty match takes every packet on the address.
/// `payload[0..payload_len]` is the payload for actions that carry one; `offset` is where a
/// `Patch`/`ReplyPatch` writes.
/// `medius_device_query_rewrite_entry` reads back this shape, so a read rule replays as a set.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct MediusRewriteRule {
    /// One of `MEDIUS_REWRITE_CLASS_*`. A byte, not `MediusRewriteClass`, so the boundary can
    /// validate it; C++ (`enum : uint8_t`) needs a cast to assign it to one.
    pub class: u8,
    /// Address within the class: interface or endpoint number.
    pub id: u16,
    /// A `MEDIUS_DIRECTION_*` value: `BOTH`, `POSITIVE` or `NEGATIVE`. A byte, not
    /// `MediusDirection`, so the boundary can validate it; C++ needs a cast to assign it to one.
    pub direction: u8,
    /// One of `MEDIUS_REWRITE_ACTION_*`. A byte, not `MediusRewriteAction`, so the boundary can
    /// validate it; C++ needs a cast to assign it to one.
    pub action: u8,
    /// Where a `Patch`/`ReplyPatch` writes; other actions ignore it.
    pub offset: u16,
    /// Valid bytes in `match_bytes` (must equal `mask_len`).
    pub match_len: u16,
    /// Valid bytes in `mask` (must equal `match_len`).
    pub mask_len: u16,
    /// Valid bytes in `payload`.
    pub payload_len: u16,
    pub match_bytes: [u8; MEDIUS_MAX_REWRITE_MATCH],
    pub mask: [u8; MEDIUS_MAX_REWRITE_MATCH],
    pub payload: [u8; MEDIUS_MAX_DEV_PAYLOAD],
}

/// One decoded `RESP(REWRITE)` row (§4.17): a rule's address, action and live counters, without its
/// match/mask/payload bytes (read those with `medius_device_query_rewrite_entry`).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediusRewriteEntry {
    /// One of `MEDIUS_REWRITE_CLASS_*`. A byte, not `MediusRewriteClass`; C++ needs a cast.
    pub class: u8,
    /// Address within the class.
    pub id: u16,
    /// A `MEDIUS_DIRECTION_*` value. A byte, not `MediusDirection`; C++ needs a cast.
    pub direction: u8,
    /// One of `MEDIUS_REWRITE_ACTION_*`. A byte, not `MediusRewriteAction`; C++ needs a cast.
    pub action: u8,
    /// `match`/`mask` bytes the rule compares.
    pub match_len: u8,
    /// Write offset for a patching action.
    pub offset: u16,
    /// Payload bytes the rule carries.
    pub payload_len: u16,
    /// Packets matched as the top-ranked rule since install or last overwrite (saturating). A
    /// `MEDIUS_REWRITE_ACTION_PASS` rule counts too.
    pub hits: u16,
}

/// Decoded `RESP(REWRITE)` (§4.17): the rewrite table summary in `entries[0..n]`, in installation
/// order. The box picks the most specific matching rule, whatever its order here.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct MediusRewriteTable {
    /// The box refused the last new rule or overwrite for lack of room (all 32 entries in use, or
    /// the 2048-byte payload pool full). The next table change or a clear resets it.
    pub table_full: u8,
    /// Bumps on a table change. A reset, detach, link loss, re-clone or the opt-in going off
    /// returns it to 0.
    pub generation: u8,
    /// Valid entries in `entries`.
    pub n: u16,
    pub entries: [MediusRewriteEntry; MEDIUS_MAX_REWRITE_ENTRIES],
}

/// A descriptor patch (§3.14), keyed by `(section, cfg, index, offset)`.
///
/// `bytes[0..len]` overwrites the descriptor from `offset`; `len` 0 removes the patch at that key.
/// Setting an existing key overwrites it and moves it to the end of the set; setting the bytes it
/// already holds changes nothing. Every section but `String` keeps the descriptor's byte count.
/// `medius_device_query_patch_entry` reads back this shape, so a read patch replays as a set.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct MediusPatch {
    /// One of `MEDIUS_PATCH_SECTION_*`. A byte, not `MediusPatchSection`, so the boundary can
    /// validate it; C++ (`enum : uint8_t`) needs a cast to assign it to one.
    pub section: u8,
    /// Configuration index for `Config`/`Report`: 0 is the first configuration, not
    /// `bConfigurationValue`.
    pub cfg: u8,
    /// Interface or string index, for `Report`/`String`.
    pub index: u8,
    /// Byte offset in the descriptor where the overwrite starts.
    pub offset: u16,
    /// Valid bytes in `bytes`; 0 removes the patch at this key.
    pub len: u16,
    pub bytes: [u8; MEDIUS_MAX_DEV_PAYLOAD],
}

/// One decoded `RESP(PATCHES)` row (§4.17): a stored patch's key and length, without its bytes
/// (read those with `medius_device_query_patch_entry`).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediusPatchEntry {
    /// One of `MEDIUS_PATCH_SECTION_*`. A byte, not `MediusPatchSection`; C++ needs a cast.
    pub section: u8,
    /// Configuration index.
    pub cfg: u8,
    /// Interface or string index.
    pub index: u8,
    /// Byte offset in the descriptor.
    pub offset: u16,
    /// Bytes the patch overwrites.
    pub len: u16,
}

/// Decoded `RESP(PATCHES)` (§4.17): the stored patch set in `entries[0..n]` plus its apply state.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct MediusPatchSet {
    /// The clone serves a non-empty patched set: the one it was presented with. A later store
    /// leaves it alone until the next presentation.
    pub applied: u8,
    /// The stored set differs from the served one in patches, bytes or order: not applied yet,
    /// changed or emptied since, refused, or held back with the opt-in off.
    pub pending: u8,
    /// The stored set failed a check at the last presentation and is unchanged since, so the clone
    /// serves the descriptors unpatched. The failed check is a clone check, or a consistency check
    /// the unpatched descriptors pass (a descriptor's length or type fields, `bcdUSB` >=0x0201 with
    /// no BOS, a HID `wDescriptorLength`, an interrupt-IN `wMaxPacketSize` smaller than the
    /// report).
    pub refused: u8,
    /// The box refused the last new patch or overwrite for lack of room (16 entries in use, or the
    /// 1024-byte pool full). The next set change or a clear resets it.
    pub table_full: u8,
    /// Valid entries in `entries`.
    pub n: u16,
    pub entries: [MediusPatchEntry; MEDIUS_MAX_PATCH_ENTRIES],
}

// Field transforms (§3.15): faithful swaps and remaps of fields the clone already declares, on the
// semantic path. Not gated on the imperfect-clone opt-in.

/// What a `MediusTransform` does to its fields (§3.15), as its `op` byte.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediusTransformOp {
    /// Move a source field's value into a destination, clearing the source.
    Remap = 0,
    /// Exchange two axes: both are read before either is written, unlike two remaps.
    Swap = 1,
}

// `op` crosses as a `u8` the crate's `TransformOp` decodes: a mismatch is a wrong transform on the
// wire, not a type error.
const _: () = {
    assert!(MediusTransformOp::Remap as u8 == medius::TransformOp::Remap.as_u8());
    assert!(MediusTransformOp::Swap as u8 == medius::TransformOp::Swap.as_u8());
};

/// One field transform (§3.15): an `op`, the `source` field it reads and the `dest` field it
/// writes.
///
/// `source` and `dest` are `MediusLockTarget`s (an axis `kind`, or `Usage` with `usage` read):
/// transforms address the lock-target field space. To weigh or reverse a field, use
/// `medius_device_scale`, whose percent is signed. `medius_device_query_transforms` reads back this
/// shape, so a read entry replays as a set.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediusTransform {
    /// One of `MEDIUS_TRANSFORM_OP_*`. A byte, not `MediusTransformOp`, so the boundary can
    /// validate it; C++ (`enum : uint8_t`) needs a cast to assign it to one.
    pub op: u8,
    /// The field read.
    pub source: MediusLockTarget,
    /// The field written.
    pub dest: MediusLockTarget,
}

/// Decoded `RESP(TRANSFORMS)` (§4.18): the transform table in `entries[0..n]`, in installation
/// order, each in the shape `medius_device_transform` takes.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct MediusTransforms {
    /// The table is full: a further entry was, or would be, refused.
    pub table_full: u8,
    /// Valid entries in `entries`.
    pub n: u16,
    pub entries: [MediusTransform; MEDIUS_MAX_TRANSFORM_ENTRIES],
}

/// Emit pacing mode, the rate in effect, and the rate the clone advertises.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediusEmitPaceStatus {
    pub mode: MediusEmitMode,
    pub fixed_hz: u16,
    pub resolved_hz: u16,
    /// Requested forced poll rate, in Hz; 0 keeps the native interval.
    pub force_hz: u16,
    /// What the clone's input endpoints advertise now, in Hz; 0 = no clone.
    pub advertised_hz: u16,
    /// 1 when the served descriptor carries a forced interval.
    pub force_active: u8,
}

/// Render mode, whether native motion is rendered, and whether the real device's profile is
/// learned.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediusRenderStatus {
    pub mode: MediusRenderMode,
    /// 1 when the model renders native motion, 0 when it is relayed.
    pub full: u8,
    /// 1 once the box has learned the real device's profile; nothing renders before that.
    pub ready: u8,
}

/// Injection spread: percent of the host's command interval, and the span the box releases across.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediusSpreadStatus {
    /// Percent of the learned command interval. 0 is off; >100 overlaps.
    pub percent: u16,
    /// Release span in microseconds. 0 while `percent` is 0, until the box learns the host's
    /// command period, and until the two chips agree which one holds the motion. While 0, the whole
    /// delta is emitted on the next report.
    pub span_us: u32,
}

/// Box-side clip lifecycle state (`medius_clip_query_status`).
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediusClipState {
    /// No clip playing: empty, or a loaded clip at its start.
    Idle = 0,
    /// Draining the ring, one entry per native frame.
    Playing = 1,
    /// Halted mid-clip; cursor and held usages kept.
    Paused = 2,
    /// An append was dropped or the ring overflowed; recover with `medius_clip_clear`.
    Faulted = 3,
}

/// Box-side clip ring and playback counters (the `RESP(CLIP)` runtime view).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediusClipStatus {
    /// A `MEDIUS_CLIP_STATE_*` value.
    pub state: u8,
    pub free: u32,
    /// Retained clip size in bytes (streaming: undrained bytes).
    pub total: u32,
    /// Bytes played from the clip start (retained progress; ~0 while streaming).
    pub played: u32,
    pub ticks: u32,
    pub underruns: u16,
    pub overruns: u16,
    pub seq_gaps: u16,
    /// Clip transfers the real device completed.
    pub xfers: u16,
    /// Clip transfers that ended otherwise: refused, no reply, no room in the box's queue, or
    /// dropped behind one the real device did not reply to.
    pub xfer_errs: u16,
    /// Raw reports and transfers discarded with the imperfect-clone opt-in off.
    pub gated: u16,
    pub held_n: u16,
    pub held: [MediusUsage; MEDIUS_MAX_USAGES],
}

/// Most input triggers in a `MediusClipSettings` (firmware `CLIP_TRIG_MAX`).
pub const MEDIUS_CLIP_TRIG_MAX: usize = 8;
/// Most packet triggers in a `MediusClipSettings` (firmware `CLIP_PKT_TRIG_MAX`).
pub const MEDIUS_CLIP_PKT_TRIG_MAX: usize = 8;
/// Match bytes the box holds across all clip packet triggers (firmware `CLIP_PKT_MATCH_POOL`).
pub const MEDIUS_CLIP_PKT_MATCH_POOL: usize = 112;
/// Most `match`/`mask` bytes one clip packet trigger compares (firmware `PKT_MATCH_MAX`).
pub const MEDIUS_MAX_PKT_MATCH: usize = 16;
/// Most edges per `MediusClipFrame` (firmware `CLIP_EDGES_MAX`).
pub const MEDIUS_CLIP_EDGES_MAX: usize = 8;
/// Most raw reports per `MediusClipFrame` (firmware `CLIP_RAW_MAX`).
pub const MEDIUS_CLIP_RAW_MAX: usize = 8;
/// Most bytes a `MediusClipFrame` encodes to: one `CLIP_APPEND` payload.
pub const MEDIUS_CLIP_ENTRY_MAX: usize = 512;

const _: () = {
    assert!(MEDIUS_CLIP_EDGES_MAX == medius::CLIP_EDGES_MAX);
    assert!(MEDIUS_CLIP_RAW_MAX == medius::CLIP_RAW_MAX);
    assert!(MEDIUS_CLIP_ENTRY_MAX == medius::CLIP_ENTRY_MAX);
    assert!(MEDIUS_CLIP_PKT_TRIG_MAX == medius::CLIP_PKT_TRIG_MAX);
    assert!(MEDIUS_CLIP_PKT_MATCH_POOL == medius::CLIP_PKT_MATCH_POOL);
    assert!(MEDIUS_MAX_PKT_MATCH == medius::PKT_MATCH_MAX);
};

/// One clip packet trigger, keyed by `(class, id, direction, match, mask)`. A traffic packet whose
/// head matches under the mask drives `action` on the box's next tick, with no host round trip. The
/// other trigger kind is `MediusClipTrigger`.
///
/// `match_bytes[0..match_len]` and `mask[0..mask_len]` are one length. A packet matches when
/// `head[i] & mask[i] == match_bytes[i]` for every match byte; an empty match takes every packet on
/// the address. For `MEDIUS_CATCH_CLASS_CONTROL` the head is the 8 setup bytes, then the first 8
/// bytes of OUT data. A `MEDIUS_CATCH_CLASS_EMIT` trigger matches the clip's frames as well as
/// native and injected ones, but not the clip's raw reports.
///
/// `medius_clip_bind_packet` and the box refuse a trigger no packet can match: a match bit outside
/// its mask (packet bytes are masked before the compare), or a direction the class never carries.
/// `HID_IN` and `EMIT` flow `POSITIVE` (IN), `HID_OUT` flows `NEGATIVE` (OUT), the vendor classes
/// and `CONTROL` carry either, and every class takes `MEDIUS_DIRECTION_BOTH`.
///
/// The box checks triggers against the packet as it arrived, ahead of the rewrite table and
/// independently of it: one packet can fire a trigger and have a rewrite rule act on it. Of the
/// triggers a packet matches, only the most specific acts: an exact `id` over
/// `MEDIUS_CATCH_ID_ANY`, more masked bits over fewer, `POSITIVE` or `NEGATIVE` over
/// `MEDIUS_DIRECTION_BOTH`, then the earlier-bound trigger.
///
/// `medius_clip_query_config` reads back this shape, so a read trigger replays as a bind.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediusClipPacketTrigger {
    /// Traffic surface: `MEDIUS_CATCH_CLASS_HID_IN`, `_HID_OUT`, `_VENDOR_INTERRUPT`,
    /// `_VENDOR_BULK`, `_CONTROL` or `_EMIT`.
    pub class: u8,
    /// Address within the class: interface number for `HID_IN`, endpoint number for the rest, or
    /// `MEDIUS_CATCH_ID_ANY`.
    pub id: u16,
    /// A `MEDIUS_DIRECTION_*` value: `BOTH`, or whichever of `POSITIVE` (IN) and `NEGATIVE` (OUT)
    /// the class carries.
    pub direction: u8,
    /// A `MEDIUS_CLIP_ACTION_*` value.
    pub action: u8,
    /// Drop each packet the trigger matches as the top-ranked trigger, ahead of the rewrite table.
    /// Dropping makes the clone imperfect, so the box holds a consuming trigger only under
    /// `medius_device_allow_imperfect_clones`, on any class but `CONTROL`.
    pub consume: u8,
    /// Drive `action` on the first of a run of matching packets, so a device repeating a held state
    /// every poll fires once per hold; 0 drives it on each packet. A run needs one stream: a class
    /// other than `CONTROL`, a concrete `id`, and `POSITIVE` or `NEGATIVE`.
    pub once_per_run: u8,
    /// With `once_per_run`, how many leading match bytes select the run's stream within the address
    /// (a report ID); 0 without. The rest are the condition. So `selector_len` is below
    /// `match_len`, and the mask past it has >=1 bit set: a condition every packet of the stream
    /// meets would make a run that never ends.
    pub selector_len: u8,
    /// Valid bytes in `match_bytes` (must equal `mask_len`).
    pub match_len: u16,
    /// Valid bytes in `mask` (must equal `match_len`).
    pub mask_len: u16,
    /// Every set bit of `match_bytes[0..match_len]` is set in `mask`; both are sent as given.
    pub match_bytes: [u8; MEDIUS_MAX_PKT_MATCH],
    pub mask: [u8; MEDIUS_MAX_PKT_MATCH],
    /// Packets matched as the top-ranked trigger since bind or overwrite (saturating). A
    /// `once_per_run` trigger counts every packet of a run. Filled by `medius_clip_query_config`,
    /// read by `medius_mock_set_clip_settings`, not sent by `medius_clip_bind_packet`.
    pub hits: u16,
}

/// Clip configuration from `RESP(CLIP)`: autolock scope, loop/retain scalars, both trigger kinds.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct MediusClipSettings {
    /// Autolock scope as `CLIP_LOCK_*` bits (`medius_clip_set_autolock`).
    pub autolock_bits: u8,
    pub loop_: u8,
    pub retain: u8,
    pub finalized: u8,
    /// Whether clip motion rides native reports (`medius_clip_set_ride`). 0 (the default) emits it
    /// at once.
    pub ride: u8,
    /// Input triggers.
    pub triggers: [MediusClipTrigger; MEDIUS_CLIP_TRIG_MAX],
    /// Valid entries in `triggers`.
    pub n: u8,
    /// Packet triggers in box order, each with its `hits`.
    pub packet_triggers: [MediusClipPacketTrigger; MEDIUS_CLIP_PKT_TRIG_MAX],
    /// Valid entries in `packet_triggers`.
    pub packet_n: u8,
}

/// Host-side always-on counters.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediusCountersSnapshot {
    pub frames_tx: u64,
    pub frames_rx: u64,
    pub crc_drops: u64,
    pub reconnects: u64,
    /// Device-chip restarts the library recovered from by re-sending its held state.
    pub restarts: u64,
}

/// A discovered medius serial port. `path` is NUL-terminated.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct MediusPortInfo {
    pub path: [c_char; MEDIUS_MAX_PATH],
    pub vid: u16,
    pub pid: u16,
    /// Control adapter serial, NUL-terminated; empty with `has_serial == 0` if the adapter has
    /// none.
    pub serial: [c_char; MEDIUS_MAX_SERIAL],
    pub has_serial: u8,
}

/// One discovered box: control port, firmware version (with the box MAC), and cloned device.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct MediusBoxInfo {
    pub port: MediusPortInfo,
    pub version: MediusVersion,
    /// Zeroed when `has_device` is 0.
    pub device: MediusDeviceInfo,
    /// 0 for a box on another control protocol (`version.proto_ver`); opening it returns
    /// `MEDIUS_STATUS_ERR_BAD_PROTO_VER`.
    pub has_device: u8,
}

/// One relative-axis catch event: native motion at the merge point, before lock suppression or
/// injection.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediusMotionEvent {
    /// Relative X this report (right positive).
    pub dx: i16,
    /// Relative Y this report (down positive).
    pub dy: i16,
    /// Wheel delta this report (up positive).
    pub dz: i16,
    /// AC Pan (horizontal-scroll) delta this report (right positive).
    pub pan: i16,
}

/// One held-usage snapshot: every held usage of one class in `usages[0..n]`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct MediusUsageEvent {
    /// The snapshot's class, a `MEDIUS_CLASS_*` value; set even when `n == 0` (the snapshot for the
    /// release of the last held usage).
    pub class: u8,
    /// A `MEDIUS_DIRECTION_*` value: the edge that produced this snapshot. The subscribed set grew
    /// (`Positive`) or shrank (`Negative`). A byte, not `MediusDirection`, so the
    /// boundary can validate it; C++ (`enum : uint8_t`) needs a cast to assign it to one.
    pub direction: u8,
    pub n: u16,
    pub usages: [MediusUsage; MEDIUS_MAX_USAGES],
}

/// One byte-oriented catch event: HID reports, vendor endpoints, control transactions, clone emits,
/// bus lifecycle, or a clip's control transfers. `bytes[0..len]` is what `capture` kept of the
/// packet.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediusTrafficEvent {
    /// One of `MEDIUS_CATCH_CLASS_*`.
    pub class: MediusCatchClass,
    /// Endpoint address, interface number, or endpoint number, per the class.
    pub id: u16,
    /// A `MEDIUS_DIRECTION_*` value: `Positive` is IN (device to PC), `Negative` is OUT. A byte,
    /// not `MediusDirection`, so the boundary can validate it; C++ (`enum : uint8_t`) needs a cast
    /// to assign it to one.
    pub direction: u8,
    /// Class-specific; read it with `medius_traffic_event_control_status`, `..._rule_acted`,
    /// `..._bus_event`, `..._transfer_status`, `..._zlp` or `..._bulk_end_of_transfer`.
    pub flags: u8,
    /// The packet's length before `capture` truncated it.
    pub true_len: u16,
    /// Valid bytes in `bytes`.
    pub len: u16,
    pub bytes: [u8; MEDIUS_MAX_TRAFFIC_BYTES],
}

/// The handshake the game PC received for a control transaction.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediusControlStatus {
    /// The transaction completed.
    Ok = 0,
    /// The real device STALLed, or a `Stall` rule refused the request.
    Stalled = 1,
    /// NAKed until the PC gave up: the real device left the request unanswered or it failed on the
    /// real device's bus, a `Nak` rule matched it, or the PC gave up before the real device
    /// finished. The last is raised when the PC's next SETUP on that endpoint replaces the request.
    Naked = 2,
    /// A handshake value this build does not know; `MediusTrafficEvent::flags` bits 0-1 hold it.
    Other = 3,
}

/// What a `MEDIUS_CATCH_CLASS_BUS` event describes.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediusBusEventKind {
    Reset = 0,
    /// The host suspended the bus; the box suspends the real device with it.
    Suspend = 1,
    /// The host resumed the bus, and the box resumes the real device with it. Also follows a remote
    /// wakeup the box signals, for an injection or one from the real device.
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

/// A decoded bus lifecycle event; payload fields are 0 for kinds that carry none.
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
    /// Stamp in the `clock` chip's microseconds. That box-local clock is unrelated to this
    /// machine's, so compare only within one domain. It wraps every ~71.6 minutes and restarts at 0
    /// when that chip reboots. A value below the previous one is a wrap, reboot or domain change,
    /// and the delta is meaningless.
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
    /// A relative-motion report; read `dx`/`dy`/`dz`/`pan`.
    Motion = 2,
}

/// One decoded input event: a press or release edge, or a motion report.
/// `medius_device_input_events` diffs the box's held-usage snapshots into these.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediusInputEvent {
    pub kind: MediusInputKind,
    /// The report's arrival stamp, in the `clock` chip's microseconds.
    pub ts_us: u32,
    /// Which chip's clock stamped it; always `HostChip` for native input.
    pub clock: MediusClockDomain,
    /// The usage this is an edge on; unset for `Motion`.
    pub usage: MediusUsage,
    /// Relative X this report (right positive); 0 unless `kind` is `Motion`.
    pub dx: i16,
    /// Relative Y this report (down positive); 0 unless `kind` is `Motion`.
    pub dy: i16,
    /// Wheel delta this report (up positive); 0 unless `kind` is `Motion`.
    pub dz: i16,
    /// AC Pan (horizontal-scroll) delta this report (right positive); 0 unless `kind` is `Motion`.
    pub pan: i16,
}

/// One event placed on this machine's clock by a `MediusTimeline`.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediusStamped {
    /// When the event happened, on the monotonic clock the caller passes as `now_ns`.
    pub host_ns: u64,
    /// The event's box stamp, unwrapped past the 32-bit rollover.
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
