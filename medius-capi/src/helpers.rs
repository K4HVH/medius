//! Pure, device-free helpers: parameter constructors and inspectors mirroring the `medius` value-type methods.

use crate::ctypes::*;
use crate::error::guard;

const SETUP_LEN: u16 = 8;

/// Build an [`MediusUsage`] addressing a mouse button. `button` takes a `MEDIUS_BUTTON_*` constant;
/// a byte no constant names is carried through and refused by the call that takes the usage, since a
/// constructor has no status to return.
#[unsafe(no_mangle)]
pub extern "C" fn medius_usage_button(button: u8) -> MediusUsage {
    MediusUsage {
        kind: MediusClass::Button as u8,
        id: button as u16,
    }
}

/// Build an [`MediusUsage`] addressing a keyboard key.
#[unsafe(no_mangle)]
pub extern "C" fn medius_usage_key(key: MediusKey) -> MediusUsage {
    MediusUsage {
        kind: MediusClass::Key as u8,
        id: key as u16,
    }
}

/// Build an [`MediusUsage`] addressing a media key.
#[unsafe(no_mangle)]
pub extern "C" fn medius_usage_media(media: MediusMediaKey) -> MediusUsage {
    MediusUsage {
        kind: MediusClass::Media as u8,
        id: media,
    }
}

/// Build a cursor-motion [`MediusMotion`].
#[unsafe(no_mangle)]
pub extern "C" fn medius_motion_cursor(dx: i16, dy: i16) -> MediusMotion {
    MediusMotion {
        kind: MediusMotionKind::Cursor as u8,
        dx,
        dy,
        wheel: 0,
    }
}

/// Build a wheel [`MediusMotion`].
#[unsafe(no_mangle)]
pub extern "C" fn medius_motion_wheel(delta: i16) -> MediusMotion {
    MediusMotion {
        kind: MediusMotionKind::Wheel as u8,
        dx: 0,
        dy: 0,
        wheel: delta,
    }
}

/// Build a [`MediusLockTarget`] addressing an axis: `kind` takes `MEDIUS_LOCK_TARGET_KIND_X`, `_Y` or
/// `_WHEEL`. Any other byte is carried through and refused by the call that takes the target, since a
/// constructor has no status to return.
#[unsafe(no_mangle)]
pub extern "C" fn medius_lock_target_axis(kind: u8) -> MediusLockTarget {
    MediusLockTarget {
        kind,
        usage: crate::convert::blank_usage(),
    }
}

/// Build a [`MediusLockTarget`] addressing a momentary usage (button, key, or media).
#[unsafe(no_mangle)]
pub extern "C" fn medius_lock_target_usage(usage: MediusUsage) -> MediusLockTarget {
    MediusLockTarget {
        kind: MediusLockTargetKind::Usage as u8,
        usage,
    }
}

// A blanket covers any usage of its class; a specific entry matches its exact target. For an axis
// target only the kind is significant (the usage field is an unused sentinel).
fn lock_entry_covers(e: &MediusLockEntry, target: MediusLockTarget) -> bool {
    let usage_kind = MediusLockTargetKind::Usage as u8;
    let is_usage = target.kind == usage_kind;
    if e.is_blanket {
        is_usage && e.target.kind == usage_kind && e.target.usage.kind == target.usage.kind
    } else {
        e.target.kind == target.kind && (!is_usage || e.target.usage == target.usage)
    }
}

/// The scale in effect on `target`/`dir`: percent of the physical value kept, so
/// `MEDIUS_LOCK_SCALE_PASS` when nothing weighs it. `Both` reports the lowest across every direction.
/// Mirrors `medius::Locks::scale_of`. `dir` takes a `MEDIUS_DIRECTION_*` constant; any other value
/// names no entry and reads as `MEDIUS_LOCK_SCALE_PASS`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_locks_scale_of(
    locks: *const MediusLocks,
    target: MediusLockTarget,
    dir: u8,
) -> u8 {
    guard(MEDIUS_LOCK_SCALE_PASS, || {
        if locks.is_null() {
            return MEDIUS_LOCK_SCALE_PASS;
        }
        let locks = unsafe { &*locks };
        let n = (locks.n as usize).min(MEDIUS_MAX_LOCKS);
        locks.entries[..n]
            .iter()
            .filter(|e| {
                let both = MediusDirection::Both as u8;
                lock_entry_covers(e, target)
                    && (dir == both || e.direction == both || e.direction == dir)
            })
            .map(|e| e.scale)
            .min()
            .unwrap_or(MEDIUS_LOCK_SCALE_PASS)
    })
}

/// Whether `target`/`dir` is blocked outright in `locks`. A direction merely weighed is not locked.
/// `Both` asks about the two fixed signs, the pair it has always named; ask for a relative direction
/// by name. Mirrors `medius::Locks::is_locked`. `dir` takes a `MEDIUS_DIRECTION_*` constant; any
/// other value names no entry and reads as unlocked.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_locks_is_locked(
    locks: *const MediusLocks,
    target: MediusLockTarget,
    dir: u8,
) -> bool {
    guard(false, || {
        if dir == MediusDirection::Both as u8 {
            return unsafe {
                medius_locks_scale_of(locks, target, MediusDirection::Positive as u8)
                    == MEDIUS_LOCK_SCALE_BLOCK
                    && medius_locks_scale_of(locks, target, MediusDirection::Negative as u8)
                        == MEDIUS_LOCK_SCALE_BLOCK
            };
        }
        unsafe { medius_locks_scale_of(locks, target, dir) == MEDIUS_LOCK_SCALE_BLOCK }
    })
}

/// The native report rate in Hz written to `out_hz`, false when there is no continuous cadence. Delegates to `medius::Rate::native_hz`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_rate_native_hz(rate: MediusRate, out_hz: *mut f32) -> bool {
    guard(false, || {
        let native: medius::Rate = rate.into();
        match native.native_hz() {
            Some(hz) => {
                if !out_hz.is_null() {
                    unsafe { *out_hz = hz };
                }
                true
            }
            None => false,
        }
    })
}

/// Whether `usage` is held in a usage snapshot. Mirrors `medius::UsageSnapshot::is_held`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_usage_event_is_held(
    event: *const MediusUsageEvent,
    usage: MediusUsage,
) -> bool {
    guard(false, || {
        if event.is_null() {
            return false;
        }
        let e = unsafe { &*event };
        let n = (e.n as usize).min(MEDIUS_MAX_USAGES);
        e.usages[..n].contains(&usage)
    })
}

// The momentary classes carry the same numbering as their catch classes; `None` for a byte no
// `MEDIUS_CLASS_*` constant names.
fn input_catch_class(class: u8) -> Option<MediusCatchClass> {
    match class {
        MEDIUS_CATCH_CLASS_BTN | MEDIUS_CATCH_CLASS_KEY | MEDIUS_CATCH_CLASS_MEDIA => Some(class),
        _ => None,
    }
}

// A filter addressing nothing: the wildcard class carrying a real id, which subscribing refuses with
// MEDIUS_STATUS_ERR_INVALID_ARG. A constructor has no status of its own, and a byte no constant names
// must not become a narrower subscription that looks like the box producing no events.
fn unaddressable() -> MediusCatchFilter {
    MediusCatchFilter {
        class: MEDIUS_CATCH_CLASS_ANY,
        id: 0,
        direction: MediusDirection::Both as u8,
        capture: 0,
    }
}

/// One momentary usage: a button, a key, or a media usage. The same thing `medius_device_lock` takes.
/// A `usage.kind` no `MEDIUS_CLASS_*` constant names yields a filter subscribing refuses.
#[unsafe(no_mangle)]
pub extern "C" fn medius_catch_filter_watch(usage: MediusUsage) -> MediusCatchFilter {
    match input_catch_class(usage.kind) {
        Some(class) => exact(class, usage.id),
        None => unaddressable(),
    }
}

/// One relative axis. `axis` takes a `MEDIUS_AXIS_*` constant; any other byte yields a filter
/// subscribing refuses.
#[unsafe(no_mangle)]
pub extern "C" fn medius_catch_filter_watch_axis(axis: u8) -> MediusCatchFilter {
    match crate::convert::axis_from_c(axis) {
        Some(axis) => exact(MEDIUS_CATCH_CLASS_AXIS, axis.as_u16()),
        None => unaddressable(),
    }
}

/// Every usage in one momentary class. `class` takes a `MEDIUS_CLASS_*` constant; any other byte
/// yields a filter subscribing refuses.
#[unsafe(no_mangle)]
pub extern "C" fn medius_catch_filter_watch_class(class: u8) -> MediusCatchFilter {
    match input_catch_class(class) {
        Some(class) => blanket(class),
        None => unaddressable(),
    }
}

/// Every relative axis: X, Y and the wheel.
#[unsafe(no_mangle)]
pub extern "C" fn medius_catch_filter_watch_axes() -> MediusCatchFilter {
    blanket(MEDIUS_CATCH_CLASS_AXIS)
}

/// Write the four input-class filters to `out[0..4]`: buttons, keys, media and axes. This is the
/// whole of what `medius_device_input_events` can report.
///
/// # Safety
/// `out` must point to space for four `MediusCatchFilter`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_catch_filter_all_input(out: *mut MediusCatchFilter) {
    guard((), || {
        if out.is_null() {
            return;
        }
        let all = [
            blanket(MEDIUS_CATCH_CLASS_BTN),
            blanket(MEDIUS_CATCH_CLASS_KEY),
            blanket(MEDIUS_CATCH_CLASS_MEDIA),
            blanket(MEDIUS_CATCH_CLASS_AXIS),
        ];
        unsafe { std::ptr::copy_nonoverlapping(all.as_ptr(), out, all.len()) };
    })
}

/// One traffic address: an endpoint, an interface, or a control endpoint number. `class` must be one
/// of the traffic classes (`MEDIUS_CATCH_CLASS_HID_IN` upwards).
#[unsafe(no_mangle)]
pub extern "C" fn medius_catch_filter_traffic(
    class: MediusCatchClass,
    id: u16,
) -> MediusCatchFilter {
    exact(class, id)
}

/// Every id within one traffic class.
#[unsafe(no_mangle)]
pub extern "C" fn medius_catch_filter_traffic_class(class: MediusCatchClass) -> MediusCatchFilter {
    blanket(class)
}

/// Every class, every id, both directions, whole packets. One table entry, not an expansion.
///
/// This includes `MEDIUS_CATCH_CLASS_VENDOR_BULK`, which can saturate the control link by itself.
/// Pair it with `medius_catch_filter_with_capture` unless you mean to trace bulk in full.
#[unsafe(no_mangle)]
pub extern "C" fn medius_catch_filter_everything() -> MediusCatchFilter {
    MediusCatchFilter {
        class: MEDIUS_CATCH_CLASS_ANY,
        id: MEDIUS_CATCH_ID_ANY,
        direction: MediusDirection::Both as u8,
        capture: 0,
    }
}

fn exact(class: MediusCatchClass, id: u16) -> MediusCatchFilter {
    MediusCatchFilter {
        class,
        id,
        direction: MediusDirection::Both as u8,
        capture: 0,
    }
}

fn blanket(class: MediusCatchClass) -> MediusCatchFilter {
    MediusCatchFilter {
        class,
        id: MEDIUS_CATCH_ID_ANY,
        direction: MediusDirection::Both as u8,
        capture: 0,
    }
}

/// `f` restricted to one direction, sign or edge. `direction` takes a `MEDIUS_DIRECTION_*` constant;
/// a byte no constant names is carried through and refused at subscribe time with
/// `MEDIUS_STATUS_ERR_INVALID_ARG`, since a filter has no status to return here.
#[unsafe(no_mangle)]
pub extern "C" fn medius_catch_filter_with_direction(
    f: MediusCatchFilter,
    direction: u8,
) -> MediusCatchFilter {
    MediusCatchFilter { direction, ..f }
}

/// `f` keeping only the first `bytes` of each packet; 0 keeps the whole packet. Traffic classes only.
#[unsafe(no_mangle)]
pub extern "C" fn medius_catch_filter_with_capture(
    f: MediusCatchFilter,
    bytes: u8,
) -> MediusCatchFilter {
    MediusCatchFilter {
        capture: bytes,
        ..f
    }
}

/// `f` restricted to the press edge.
#[unsafe(no_mangle)]
pub extern "C" fn medius_catch_filter_on_press(f: MediusCatchFilter) -> MediusCatchFilter {
    medius_catch_filter_with_direction(f, MediusDirection::Positive as u8)
}

/// `f` restricted to the release edge.
#[unsafe(no_mangle)]
pub extern "C" fn medius_catch_filter_on_release(f: MediusCatchFilter) -> MediusCatchFilter {
    medius_catch_filter_with_direction(f, MediusDirection::Negative as u8)
}

/// `f` restricted to traffic from the device to the PC.
#[unsafe(no_mangle)]
pub extern "C" fn medius_catch_filter_inbound(f: MediusCatchFilter) -> MediusCatchFilter {
    medius_catch_filter_with_direction(f, MediusDirection::Positive as u8)
}

/// `f` restricted to traffic from the PC to the device.
#[unsafe(no_mangle)]
pub extern "C" fn medius_catch_filter_outbound(f: MediusCatchFilter) -> MediusCatchFilter {
    medius_catch_filter_with_direction(f, MediusDirection::Negative as u8)
}

/// Whether two filters name the same box table entry, whatever their captures.
#[unsafe(no_mangle)]
pub extern "C" fn medius_catch_filter_same_address(
    a: MediusCatchFilter,
    b: MediusCatchFilter,
) -> bool {
    (a.class, a.id, a.direction) == (b.class, b.id, b.direction)
}

/// Whether `class` is one of the four parsed-input classes, which arrive decoded and carry no packet.
#[unsafe(no_mangle)]
pub extern "C" fn medius_catch_class_is_input(class: MediusCatchClass) -> bool {
    class <= MEDIUS_CATCH_CLASS_AXIS
}

/// Whether `class` is one of the seven byte-oriented traffic classes.
#[unsafe(no_mangle)]
pub extern "C" fn medius_catch_class_is_traffic(class: MediusCatchClass) -> bool {
    (MEDIUS_CATCH_CLASS_HID_IN..=MEDIUS_CATCH_CLASS_BUS).contains(&class)
}

/// Whether the capture cut this packet short. Without checking, a truncated capture and a genuinely
/// short packet are indistinguishable. Mirrors `medius::TrafficEvent::truncated`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_traffic_event_truncated(event: *const MediusTrafficEvent) -> bool {
    guard(false, || {
        if event.is_null() {
            return false;
        }
        let e = unsafe { &*event };
        e.len < e.true_len
    })
}

/// The 8-byte setup packet of a CONTROL event, or NULL for another class or a capture cut shorter
/// than the setup stage. Points into `event`. Mirrors `medius::TrafficEvent::setup`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_traffic_event_setup(event: *const MediusTrafficEvent) -> *const u8 {
    guard(std::ptr::null(), || {
        if event.is_null() {
            return std::ptr::null();
        }
        let e = unsafe { &*event };
        if e.class == MEDIUS_CATCH_CLASS_CONTROL && e.len >= SETUP_LEN {
            e.bytes.as_ptr()
        } else {
            std::ptr::null()
        }
    })
}

/// The data stage of a CONTROL event, the whole packet for any other class; its length goes to
/// `*out_len`. Points into `event`. Mirrors `medius::TrafficEvent::data`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_traffic_event_data(
    event: *const MediusTrafficEvent,
    out_len: *mut usize,
) -> *const u8 {
    guard(std::ptr::null(), || {
        if event.is_null() {
            return std::ptr::null();
        }
        let e = unsafe { &*event };
        let n = (e.len as usize).min(MEDIUS_MAX_TRAFFIC_BYTES);
        // A control event whose own setup packet was cut short has no data stage at all.
        // Falling through to "the whole buffer is the data" handed a decoder the surviving setup
        // bytes: a GET_DESCRIPTOR request labelled as the descriptor it asked for.
        let (skip, n) = if e.class != MEDIUS_CATCH_CLASS_CONTROL {
            (0usize, n)
        } else if e.len >= SETUP_LEN {
            (SETUP_LEN as usize, n)
        } else {
            (0usize, 0usize)
        };
        if !out_len.is_null() {
            unsafe { *out_len = n - skip };
        }
        unsafe { e.bytes.as_ptr().add(skip) }
    })
}

/// What the real device answered, written to `*out`; false for any class but CONTROL. Mirrors
/// `medius::TrafficEvent::control_status`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_traffic_event_control_status(
    event: *const MediusTrafficEvent,
    out: *mut MediusControlStatus,
) -> bool {
    guard(false, || {
        if event.is_null() {
            return false;
        }
        let e = unsafe { &*event };
        if e.class != MEDIUS_CATCH_CLASS_CONTROL {
            return false;
        }
        let status = match e.flags {
            0x00 => MediusControlStatus::Ok,
            0xFD => MediusControlStatus::Stalled,
            0xFE => MediusControlStatus::Naked,
            _ => MediusControlStatus::Other,
        };
        if !out.is_null() {
            unsafe { *out = status };
        }
        true
    })
}

/// The lifecycle event, written to `*out`; false for any class but BUS or an unknown kind. Mirrors
/// `medius::TrafficEvent::bus_event`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_traffic_event_bus_event(
    event: *const MediusTrafficEvent,
    out: *mut MediusBusEvent,
) -> bool {
    guard(false, || {
        if event.is_null() {
            return false;
        }
        let e = unsafe { &*event };
        if e.class != MEDIUS_CATCH_CLASS_BUS {
            return false;
        }
        // Only the captured bytes count: past `len` the array still holds whatever the caller left
        // there, and a stale byte would name the wrong configuration or interface.
        let payload = &e.bytes[..(e.len as usize).min(MEDIUS_MAX_TRAFFIC_BYTES)];
        let a = payload.first().copied().unwrap_or(0);
        let b = payload.get(1).copied().unwrap_or(0);
        let kind = match e.flags {
            0 => MediusBusEventKind::Reset,
            1 => MediusBusEventKind::Suspend,
            2 => MediusBusEventKind::Resume,
            3 => MediusBusEventKind::Configured,
            4 => MediusBusEventKind::Deconfigured,
            5 => MediusBusEventKind::SetInterface,
            6 => MediusBusEventKind::DeviceAttached,
            7 => MediusBusEventKind::DeviceDetached,
            8 => MediusBusEventKind::CloneUp,
            9 => MediusBusEventKind::CloneDown,
            _ => return false,
        };
        if !out.is_null() {
            unsafe {
                *out = MediusBusEvent {
                    kind,
                    configuration: if kind == MediusBusEventKind::Configured {
                        a
                    } else {
                        0
                    },
                    interface: if kind == MediusBusEventKind::SetInterface {
                        a
                    } else {
                        0
                    },
                    alt: if kind == MediusBusEventKind::SetInterface {
                        b
                    } else {
                        0
                    },
                }
            };
        }
        true
    })
}

/// Whether this event carries end-of-transfer, for a VEND_BULK event. Mirrors
/// `medius::TrafficEvent::bulk_end_of_transfer`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_traffic_event_bulk_end_of_transfer(
    event: *const MediusTrafficEvent,
) -> bool {
    guard(false, || {
        if event.is_null() {
            return false;
        }
        let e = unsafe { &*event };
        e.class == MEDIUS_CATCH_CLASS_VENDOR_BULK && e.flags & 0x01 != 0
    })
}

/// Whether this event is a zero-length packet, for a VEND_BULK event. A ZLP terminates a transfer
/// whose length is an exact multiple of the packet size, so it carries no bytes and still matters.
/// Mirrors `medius::TrafficEvent::bulk_zlp`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_traffic_event_bulk_zlp(event: *const MediusTrafficEvent) -> bool {
    guard(false, || {
        if event.is_null() {
            return false;
        }
        let e = unsafe { &*event };
        e.class == MEDIUS_CATCH_CLASS_VENDOR_BULK && e.flags & 0x02 != 0
    })
}

/// Whether the clip is currently holding `usage` down. Mirrors `medius::ClipStatus::is_held`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_clip_status_is_held(
    status: *const MediusClipStatus,
    usage: MediusUsage,
) -> bool {
    guard(false, || {
        if status.is_null() {
            return false;
        }
        let s = unsafe { &*status };
        let n = (s.held_n as usize).min(MEDIUS_MAX_USAGES);
        s.held[..n].contains(&usage)
    })
}

/// Whether a mouse interface is bound. Delegates to `medius::Caps::has_mouse`.
#[unsafe(no_mangle)]
pub extern "C" fn medius_caps_has_mouse(caps: MediusCaps) -> bool {
    guard(false, || medius::Caps::from(caps).has_mouse())
}

/// Whether a keyboard interface is bound. Delegates to `medius::Caps::has_keyboard`.
#[unsafe(no_mangle)]
pub extern "C" fn medius_caps_has_keyboard(caps: MediusCaps) -> bool {
    guard(false, || medius::Caps::from(caps).has_keyboard())
}

/// Whether the clone is composite (multi-HID-interface). Delegates to `medius::Caps::is_composite`.
#[unsafe(no_mangle)]
pub extern "C" fn medius_caps_is_composite(caps: MediusCaps) -> bool {
    guard(false, || medius::Caps::from(caps).is_composite())
}
