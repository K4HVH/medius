//! Pure, device-free helpers: parameter constructors and inspectors over the value types. These
//! mirror the equivalent `medius` methods so a C caller has the same vocabulary.

use crate::ctypes::*;
use crate::error::guard;

/// Build an [`MediusInput`] addressing a mouse button.
#[unsafe(no_mangle)]
pub extern "C" fn medius_input_button(button: MediusButton) -> MediusInput {
    MediusInput {
        kind: MediusInputKind::Button,
        value: button as u16,
    }
}

/// Build an [`MediusInput`] addressing a keyboard key.
#[unsafe(no_mangle)]
pub extern "C" fn medius_input_key(key: MediusKey) -> MediusInput {
    MediusInput {
        kind: MediusInputKind::Key,
        value: key as u16,
    }
}

/// Build an [`MediusInput`] addressing a media key.
#[unsafe(no_mangle)]
pub extern "C" fn medius_input_media(media: MediusMediaKey) -> MediusInput {
    MediusInput {
        kind: MediusInputKind::Media,
        value: media,
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
        usage: MediusInput {
            kind: MediusInputKind::Button,
            value: 0,
        },
    }
}

/// Build a [`MediusLockTarget`] addressing a momentary usage (button, key, or media).
#[unsafe(no_mangle)]
pub extern "C" fn medius_lock_target_usage(usage: MediusInput) -> MediusLockTarget {
    MediusLockTarget {
        kind: MediusLockTargetKind::Usage,
        usage,
    }
}

/// Whether `target`/`dir` is locked in `locks` (`Both` requires both edges). Mirrors
/// `medius::Locks::is_locked`: an exact target match, not a whole-class blanket.
#[unsafe(no_mangle)]
pub extern "C" fn medius_locks_is_locked(
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
        locks.entries[..n].iter().any(|e| {
            !e.is_blanket
                && e.target == target
                && match dir {
                    MediusLockDirection::Both => e.positive && e.negative,
                    MediusLockDirection::Positive => e.positive,
                    MediusLockDirection::Negative => e.negative,
                }
        })
    })
}

/// The native report rate in Hz, written to `out_hz`. Returns false (and leaves `out_hz` untouched)
/// when there is no continuous cadence. Delegates to `medius::Rate::native_hz`.
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

/// Whether `usage` is held in a usage snapshot (a button, key, or media usage; modifiers are key usages
/// `0xE0..=0xE7`). Mirrors `medius::UsageSnapshot::is_held`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_usage_event_is_held(
    event: *const MediusUsageEvent,
    usage: MediusInput,
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
