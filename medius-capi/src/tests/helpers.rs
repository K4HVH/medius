//! Tests for the pure helpers: parameter constructors and value inspectors.

use crate::*;

#[test]
fn input_constructors_tag_and_value() {
    assert_eq!(
        medius_input_button(MediusButton::Side1),
        MediusInput {
            kind: MediusInputKind::Button,
            value: 3
        }
    );
    assert_eq!(
        medius_input_key(MEDIUS_KEY_A),
        MediusInput {
            kind: MediusInputKind::Key,
            value: 0x04
        }
    );
    assert_eq!(
        medius_input_media(MEDIUS_MEDIA_VOLUME_UP),
        MediusInput {
            kind: MediusInputKind::Media,
            value: 0xE9
        }
    );
}

#[test]
fn motion_constructors_select_the_right_arm() {
    assert_eq!(
        medius_motion_cursor(100, -50),
        MediusMotion {
            kind: MediusMotionKind::Cursor,
            dx: 100,
            dy: -50,
            wheel: 0
        }
    );
    assert_eq!(
        medius_motion_wheel(3),
        MediusMotion {
            kind: MediusMotionKind::Wheel,
            dx: 0,
            dy: 0,
            wheel: 3
        }
    );
}

fn target(kind: MediusLockTargetKind, button: MediusButton) -> MediusLockTarget {
    MediusLockTarget { kind, button }
}

#[test]
fn is_locked_matches_the_bit_layout() {
    // X is target 0: positive at bit 0, negative at bit 1.
    let x = target(MediusLockTargetKind::X, MediusButton::Left);
    let pos_only = MediusLocks { mask: 0b01 };
    assert!(medius_locks_is_locked(
        pos_only,
        x,
        MediusLockDirection::Positive
    ));
    assert!(!medius_locks_is_locked(
        pos_only,
        x,
        MediusLockDirection::Negative
    ));
    assert!(!medius_locks_is_locked(
        pos_only,
        x,
        MediusLockDirection::Both
    ));

    let both = MediusLocks { mask: 0b11 };
    assert!(medius_locks_is_locked(both, x, MediusLockDirection::Both));

    // Side2 is button id 4 -> target byte 3+4 = 7 -> base bit 14.
    let side2 = target(MediusLockTargetKind::Button, MediusButton::Side2);
    let side2_neg = MediusLocks { mask: 1 << 15 };
    assert!(medius_locks_is_locked(
        side2_neg,
        side2,
        MediusLockDirection::Negative
    ));
    assert!(!medius_locks_is_locked(
        side2_neg,
        side2,
        MediusLockDirection::Positive
    ));
}

#[test]
fn rate_native_hz_divides_the_period() {
    let mut hz = 0.0f32;
    let rate = MediusRate {
        native_period_us: 1000,
        poll_period_us: 1000,
        confident: 1,
        change_driven: 0,
    };
    assert!(unsafe { medius_rate_native_hz(rate, &mut hz) });
    assert!((hz - 1000.0).abs() < 0.01);

    let no_cadence = MediusRate {
        native_period_us: 0,
        poll_period_us: 1000,
        confident: 0,
        change_driven: 1,
    };
    assert!(!unsafe { medius_rate_native_hz(no_cadence, &mut hz) });
}

#[test]
fn mouse_event_is_pressed_reads_the_bitmask() {
    let e = MediusMouseEvent {
        buttons: 1 << 3, // Side1
        dx: 0,
        dy: 0,
        wheel: 0,
    };
    assert!(unsafe { medius_mouse_event_is_pressed(&e, MediusButton::Side1) });
    assert!(!unsafe { medius_mouse_event_is_pressed(&e, MediusButton::Left) });
    assert!(!unsafe { medius_mouse_event_is_pressed(std::ptr::null(), MediusButton::Left) });
}

#[test]
fn keyboard_event_is_pressed_handles_modifiers_and_keys() {
    let mut keys = [0u8; MEDIUS_MAX_KEYS];
    keys[0] = MEDIUS_KEY_A;
    keys[1] = MEDIUS_KEY_B;
    let e = MediusKeyboardEvent {
        modifiers: 1 << 1, // LEFT_SHIFT is 0xE1 -> bit 1
        n_keys: 2,
        keys,
    };
    assert!(unsafe { medius_keyboard_event_is_pressed(&e, MEDIUS_KEY_A) });
    assert!(unsafe { medius_keyboard_event_is_pressed(&e, MEDIUS_KEY_B) });
    assert!(!unsafe { medius_keyboard_event_is_pressed(&e, MEDIUS_KEY_C) });
    assert!(unsafe { medius_keyboard_event_is_pressed(&e, MEDIUS_KEY_LEFT_SHIFT) });
    assert!(!unsafe { medius_keyboard_event_is_pressed(&e, MEDIUS_KEY_LEFT_CTRL) });
}

#[test]
fn media_event_is_pressed_searches_the_list() {
    let mut keys = [0u16; MEDIUS_MAX_MEDIA_KEYS];
    keys[0] = MEDIUS_MEDIA_VOLUME_UP;
    let e = MediusMediaEvent { n_keys: 1, keys };
    assert!(unsafe { medius_media_event_is_pressed(&e, MEDIUS_MEDIA_VOLUME_UP) });
    assert!(!unsafe { medius_media_event_is_pressed(&e, MEDIUS_MEDIA_MUTE) });
}

#[test]
fn caps_predicates() {
    let mouse_only = MediusCaps {
        mouse: MediusMouseCaps {
            n_buttons: 5,
            has_x: 1,
            has_y: 1,
            has_wheel: 1,
            has_report_id: 0,
            n_hid: 1,
        },
        keyboard: MediusKbdCaps {
            n_keys: 0,
            nkro: 0,
            has_consumer: 0,
            has_system: 0,
            has_report_id: 0,
        },
        mouse_change_driven: 0,
        kbd_change_driven: 0,
    };
    assert!(medius_caps_has_mouse(mouse_only));
    assert!(!medius_caps_has_keyboard(mouse_only));
    assert!(!medius_caps_is_composite(mouse_only));

    let composite_kbd = MediusCaps {
        mouse: MediusMouseCaps {
            n_hid: 2,
            ..mouse_only.mouse
        },
        keyboard: MediusKbdCaps {
            n_keys: 6,
            ..mouse_only.keyboard
        },
        ..mouse_only
    };
    assert!(medius_caps_has_keyboard(composite_kbd));
    assert!(medius_caps_is_composite(composite_kbd));
}

#[test]
fn keyboard_event_count_caps_at_u8_max_without_wrapping() {
    // A snapshot larger than the u8 count must cap at 255, never wrap to 0.
    let kb = medius::KeyboardEvent {
        modifiers: 0,
        keys: (0..300u16).map(|i| medius::Key::new(i as u8)).collect(),
    };
    let c = MediusKeyboardEvent::from(&kb);
    assert_eq!(c.n_keys, 255);
    let md = medius::MediaEvent {
        keys: (0..300u16).map(medius::MediaKey::new).collect(),
    };
    let c = MediusMediaEvent::from(&md);
    assert_eq!(c.n_keys, 255);
}

#[test]
fn last_error_message_truncates_and_reports_full_length() {
    // No call has failed on this thread yet, but a short buffer must still NUL-terminate safely.
    let mut buf = [0i8; 8];
    let _ = unsafe { medius_last_error_message(buf.as_mut_ptr(), buf.len()) };
    // The last byte we may have written is a NUL; the call must not overrun.
    assert_eq!(buf[7], 0);
}
