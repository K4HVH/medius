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

/// The wire `target` byte for a lock target (X=0, Y=1, Wheel=2, Button = 3 + button id).
fn lock_target_wire(t: MediusLockTarget) -> u8 {
    match t.kind {
        MediusLockTargetKind::X => 0,
        MediusLockTargetKind::Y => 1,
        MediusLockTargetKind::Wheel => 2,
        MediusLockTargetKind::Button => 3 + (t.button as u8),
    }
}

/// Whether `target`/`dir` is locked in `locks` (`Both` requires both edges). Mirrors
/// `medius::Locks::is_locked`; `Locks` has no public constructor, so the bit logic is replicated here.
#[unsafe(no_mangle)]
pub extern "C" fn medius_locks_is_locked(
    locks: MediusLocks,
    target: MediusLockTarget,
    dir: MediusLockDirection,
) -> bool {
    guard(false, || {
        let base = lock_target_wire(target) * 2;
        let pos = locks.mask & (1 << base) != 0;
        let neg = locks.mask & (1 << (base + 1)) != 0;
        match dir {
            MediusLockDirection::Both => pos && neg,
            MediusLockDirection::Positive => pos,
            MediusLockDirection::Negative => neg,
        }
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

/// Whether `button` is held in a mouse snapshot. Delegates to `medius::MouseEvent::is_pressed`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_mouse_event_is_pressed(
    event: *const MediusMouseEvent,
    button: MediusButton,
) -> bool {
    guard(false, || {
        if event.is_null() {
            return false;
        }
        let native: medius::MouseEvent = (*unsafe { &*event }).into();
        native.is_pressed(button.into())
    })
}

/// Whether `key` is held in a keyboard snapshot (modifier from the bitmap, else searched in the
/// keycode list). Mirrors `medius::KeyboardEvent::is_pressed` without allocating.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_keyboard_event_is_pressed(
    event: *const MediusKeyboardEvent,
    key: MediusKey,
) -> bool {
    guard(false, || {
        if event.is_null() {
            return false;
        }
        let e = unsafe { &*event };
        if (0xE0..=0xE7).contains(&key) {
            e.modifiers & (1 << (key - 0xE0)) != 0
        } else {
            let n = (e.n_keys as usize).min(MEDIUS_MAX_KEYS);
            e.keys[..n].contains(&key)
        }
    })
}

/// Whether `media` is active in a media snapshot. Mirrors `medius::MediaEvent::is_pressed`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn medius_media_event_is_pressed(
    event: *const MediusMediaEvent,
    media: MediusMediaKey,
) -> bool {
    guard(false, || {
        if event.is_null() {
            return false;
        }
        let e = unsafe { &*event };
        let n = (e.n_keys as usize).min(MEDIUS_MAX_MEDIA_KEYS);
        e.keys[..n].contains(&media)
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
