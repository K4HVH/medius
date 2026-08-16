//! Pure, device-free helpers: parameter constructors and inspectors mirroring the `medius` value-type methods.

use crate::ctypes::*;
use crate::error::guard;

const SETUP_LEN: u16 = 8;

/// Build an [`MediusUsage`] addressing a mouse button.
#[unsafe(no_mangle)]
pub extern "C" fn medius_usage_button(button: MediusButton) -> MediusUsage {
    MediusUsage {
        kind: MediusClass::Button,
        id: button as u16,
    }
}

/// Build an [`MediusUsage`] addressing a keyboard key.
#[unsafe(no_mangle)]
pub extern "C" fn medius_usage_key(key: MediusKey) -> MediusUsage {
    MediusUsage {
        kind: MediusClass::Key,
        id: key as u16,
    }
}

/// Build an [`MediusUsage`] addressing a media key.
#[unsafe(no_mangle)]
pub extern "C" fn medius_usage_media(media: MediusMediaKey) -> MediusUsage {
    MediusUsage {
        kind: MediusClass::Media,
        id: media,
    }
}

/// Build a cursor-motion [`MediusMotion`].
#[unsafe(no_mangle)]
pub extern "C" fn medius_motion_cursor(dx: i16, dy: i16) -> MediusMotion {
    MediusMotion {
        kind: MediusMotionKind::Cursor,
        dx,
        dy,
        wheel: 0,
    }
}

/// Build a wheel [`MediusMotion`].
#[unsafe(no_mangle)]
pub extern "C" fn medius_motion_wheel(delta: i16) -> MediusMotion {
    MediusMotion {
        kind: MediusMotionKind::Wheel,
        dx: 0,
        dy: 0,
        wheel: delta,
    }
}

/// Build a [`MediusLockTarget`] addressing an axis (`kind` must be `X`, `Y`, or `Wheel`).
#[unsafe(no_mangle)]
pub extern "C" fn medius_lock_target_axis(kind: MediusLockTargetKind) -> MediusLockTarget {
    MediusLockTarget {
        kind,
        usage: MediusUsage {
            kind: MediusClass::Button,
            id: 0,
        },
    }
}

/// Build a [`MediusLockTarget`] addressing a momentary usage (button, key, or media).
#[unsafe(no_mangle)]
pub extern "C" fn medius_lock_target_usage(usage: MediusUsage) -> MediusLockTarget {
    MediusLockTarget {
        kind: MediusLockTargetKind::Usage,
        usage,
    }
}

/// Whether `target`/`dir` is locked in `locks` (`Both` requires both edges). Mirrors `medius::Locks::is_locked`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_locks_is_locked(
    locks: *const MediusLocks,
    target: MediusLockTarget,
    dir: MediusLockDirection,
) -> bool {
    guard(false, || {
        if locks.is_null() {
            return false;
        }
        let locks = unsafe { &*locks };
        let n = (locks.n as usize).min(MEDIUS_MAX_LOCKS);
        let is_usage = target.kind == MediusLockTargetKind::Usage;
        locks.entries[..n].iter().any(|e| {
            // A blanket covers any usage of its class; a specific entry matches its exact target. For an
            // axis target only the kind is significant (the usage field is an unused sentinel).
            let covers = if e.is_blanket {
                is_usage
                    && e.target.kind == MediusLockTargetKind::Usage
                    && e.target.usage.kind == target.usage.kind
            } else {
                e.target.kind == target.kind && (!is_usage || e.target.usage == target.usage)
            };
            covers
                && match dir {
                    MediusLockDirection::Both => e.positive && e.negative,
                    MediusLockDirection::Positive => e.positive,
                    MediusLockDirection::Negative => e.negative,
                }
        })
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

/// A filter matching every class, every id, both directions, whole packets. One frame on the wire.
#[unsafe(no_mangle)]
pub extern "C" fn medius_catch_filter_all() -> MediusCatchFilter {
    MediusCatchFilter {
        class: MEDIUS_CATCH_CLASS_ANY,
        id: MEDIUS_CATCH_ID_ANY,
        direction: MediusLockDirection::Both,
        snaplen: 0,
    }
}

/// A filter matching every id within one class. Set `direction`/`snaplen` on the result to narrow it.
#[unsafe(no_mangle)]
pub extern "C" fn medius_catch_filter_class(class: MediusCatchClass) -> MediusCatchFilter {
    MediusCatchFilter {
        class,
        id: MEDIUS_CATCH_ID_ANY,
        direction: MediusLockDirection::Both,
        snaplen: 0,
    }
}

/// A filter matching one exact address: an endpoint, an interface, or a usage.
#[unsafe(no_mangle)]
pub extern "C" fn medius_catch_filter_addr(class: MediusCatchClass, id: u16) -> MediusCatchFilter {
    MediusCatchFilter {
        class,
        id,
        direction: MediusLockDirection::Both,
        snaplen: 0,
    }
}

/// Whether `snaplen` cut this packet short. Without checking, a truncated capture and a genuinely
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
        // A control event whose own setup packet was cut short by snaplen has no data stage at all.
        // Falling through to "the whole buffer is the data" handed a decoder the surviving setup
        // bytes -- a GET_DESCRIPTOR request labelled as the descriptor it asked for.
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
        e.class == MEDIUS_CATCH_CLASS_VEND_BULK && e.flags & 0x01 != 0
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
        e.class == MEDIUS_CATCH_CLASS_VEND_BULK && e.flags & 0x02 != 0
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
