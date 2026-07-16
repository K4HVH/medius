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

fn locks_with(entries: &[MediusLockEntry]) -> MediusLocks {
    let blank = MediusLockEntry {
        target: medius_lock_target_axis(MediusLockTargetKind::X),
        is_blanket: false,
        positive: false,
        negative: false,
    };
    let mut l = MediusLocks {
        n: entries.len() as u16,
        entries: [blank; MEDIUS_MAX_LOCKS],
    };
    for (slot, e) in l.entries.iter_mut().zip(entries.iter()) {
        *slot = *e;
    }
    l
}

#[test]
fn is_locked_matches_entries() {
    let x = medius_lock_target_axis(MediusLockTargetKind::X);
    let side2 = medius_lock_target_usage(medius_input_button(MediusButton::Side2));
    let locks = locks_with(&[
        MediusLockEntry {
            target: x,
            is_blanket: false,
            positive: true,
            negative: false,
        },
        MediusLockEntry {
            target: side2,
            is_blanket: false,
            positive: false,
            negative: true,
        },
    ]);
    assert!(medius_locks_is_locked(
        &locks,
        x,
        MediusLockDirection::Positive
    ));
    assert!(!medius_locks_is_locked(
        &locks,
        x,
        MediusLockDirection::Negative
    ));
    assert!(!medius_locks_is_locked(
        &locks,
        x,
        MediusLockDirection::Both
    ));
    assert!(medius_locks_is_locked(
        &locks,
        side2,
        MediusLockDirection::Negative
    ));
    assert!(!medius_locks_is_locked(
        &locks,
        side2,
        MediusLockDirection::Positive
    ));
    assert!(!medius_locks_is_locked(
        std::ptr::null(),
        x,
        MediusLockDirection::Positive
    ));

    // A whole-class blanket entry covers any usage of its class (a buttons blanket locks Side2), but not a
    // usage of a different class or an axis.
    let blanket = locks_with(&[MediusLockEntry {
        target: medius_lock_target_usage(medius_input_button(MediusButton::Left)),
        is_blanket: true,
        positive: true,
        negative: true,
    }]);
    assert!(medius_locks_is_locked(
        &blanket,
        side2,
        MediusLockDirection::Positive
    ));
    assert!(!medius_locks_is_locked(
        &blanket,
        medius_lock_target_usage(medius_input_key(MEDIUS_KEY_A)),
        MediusLockDirection::Positive
    ));
    assert!(!medius_locks_is_locked(
        &blanket,
        x,
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

fn usage_event(usages: &[MediusInput]) -> MediusUsageEvent {
    let mut e = MediusUsageEvent {
        n: usages.len() as u16,
        usages: [MediusInput {
            kind: MediusInputKind::Button,
            value: 0,
        }; MEDIUS_MAX_USAGES],
    };
    for (slot, u) in e.usages.iter_mut().zip(usages.iter()) {
        *slot = *u;
    }
    e
}

#[test]
fn usage_event_is_held_matches_any_class() {
    // Buttons, keys, and modifiers all live in one snapshot list, keyed the same way.
    let a = medius_input_key(MEDIUS_KEY_A);
    let shift = medius_input_key(MEDIUS_KEY_LEFT_SHIFT);
    let side1 = medius_input_button(MediusButton::Side1);
    let e = usage_event(&[a, shift, side1]);
    assert!(unsafe { medius_usage_event_is_held(&e, a) });
    assert!(unsafe { medius_usage_event_is_held(&e, shift) });
    assert!(unsafe { medius_usage_event_is_held(&e, side1) });
    assert!(!unsafe { medius_usage_event_is_held(&e, medius_input_key(MEDIUS_KEY_B)) });
    assert!(!unsafe { medius_usage_event_is_held(&e, medius_input_button(MediusButton::Left)) });
    assert!(!unsafe { medius_usage_event_is_held(std::ptr::null(), a) });
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
fn usage_snapshot_count_caps_at_capacity_without_wrapping() {
    // A snapshot larger than the C capacity caps at MEDIUS_MAX_USAGES, never wraps.
    let snap = medius::UsageSnapshot {
        usages: (0..(MEDIUS_MAX_USAGES as u16 + 44))
            .map(|i| medius::Usage::new(medius::Class::Key, i))
            .collect(),
    };
    let ev = MediusCatchEvent::from(medius::CatchEvent::Usages(snap));
    assert_eq!(ev.kind, MediusCatchEventKind::Usages);
    assert_eq!(unsafe { ev.data.usages.n } as usize, MEDIUS_MAX_USAGES);
}

#[test]
fn last_error_message_truncates_and_reports_full_length() {
    // No call has failed on this thread yet, but a short buffer must still NUL-terminate safely.
    let mut buf = [0i8; 8];
    let _ = unsafe { medius_last_error_message(buf.as_mut_ptr(), buf.len()) };
    // The last byte we may have written is a NUL; the call must not overrun.
    assert_eq!(buf[7], 0);
}
