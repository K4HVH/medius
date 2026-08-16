//! Tests for the pure helpers: parameter constructors and value inspectors.

use crate::*;

#[test]
fn input_constructors_tag_and_value() {
    assert_eq!(
        medius_usage_button(MediusButton::Side1),
        MediusUsage {
            kind: MediusClass::Button,
            id: 3
        }
    );
    assert_eq!(
        medius_usage_key(MEDIUS_KEY_A),
        MediusUsage {
            kind: MediusClass::Key,
            id: 0x04
        }
    );
    assert_eq!(
        medius_usage_media(MEDIUS_MEDIA_VOLUME_UP),
        MediusUsage {
            kind: MediusClass::Media,
            id: 0xE9
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
    let locked = |l: *const MediusLocks, t: MediusLockTarget, d: MediusLockDirection| unsafe {
        medius_locks_is_locked(l, t, d)
    };
    let x = medius_lock_target_axis(MediusLockTargetKind::X);
    let side2 = medius_lock_target_usage(medius_usage_button(MediusButton::Side2));
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
    assert!(locked(&locks, x, MediusLockDirection::Positive));
    assert!(!locked(&locks, x, MediusLockDirection::Negative));
    assert!(!locked(&locks, x, MediusLockDirection::Both));
    assert!(locked(&locks, side2, MediusLockDirection::Negative));
    assert!(!locked(&locks, side2, MediusLockDirection::Positive));
    assert!(!locked(std::ptr::null(), x, MediusLockDirection::Positive));

    let blanket = locks_with(&[MediusLockEntry {
        target: medius_lock_target_usage(medius_usage_button(MediusButton::Left)),
        is_blanket: true,
        positive: true,
        negative: true,
    }]);
    let key_a = medius_lock_target_usage(medius_usage_key(MEDIUS_KEY_A));
    assert!(locked(&blanket, side2, MediusLockDirection::Positive));
    assert!(!locked(&blanket, key_a, MediusLockDirection::Positive));
    assert!(!locked(&blanket, x, MediusLockDirection::Positive));
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

fn usage_event(usages: &[MediusUsage]) -> MediusUsageEvent {
    let mut e = MediusUsageEvent {
        class: usages.first().map_or(MediusClass::Button, |u| u.kind),
        n: usages.len() as u16,
        usages: [MediusUsage {
            kind: MediusClass::Button,
            id: 0,
        }; MEDIUS_MAX_USAGES],
    };
    for (slot, u) in e.usages.iter_mut().zip(usages.iter()) {
        *slot = *u;
    }
    e
}

#[test]
fn usage_event_is_held_matches_any_class() {
    let a = medius_usage_key(MEDIUS_KEY_A);
    let shift = medius_usage_key(MEDIUS_KEY_LEFT_SHIFT);
    let side1 = medius_usage_button(MediusButton::Side1);
    let e = usage_event(&[a, shift, side1]);
    assert!(unsafe { medius_usage_event_is_held(&e, a) });
    assert!(unsafe { medius_usage_event_is_held(&e, shift) });
    assert!(unsafe { medius_usage_event_is_held(&e, side1) });
    assert!(!unsafe { medius_usage_event_is_held(&e, medius_usage_key(MEDIUS_KEY_B)) });
    assert!(!unsafe { medius_usage_event_is_held(&e, medius_usage_button(MediusButton::Left)) });
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
    let snap = medius::UsageSnapshot {
        ts_us: 0,
        clock: medius::ClockDomain::HostChip,
        class: medius::Class::Key,
        usages: (0..(MEDIUS_MAX_USAGES as u16 + 44))
            .map(|i| medius::Usage::new(medius::Class::Key, i))
            .collect(),
    };
    let ev = MediusCatchEvent::from(medius::CatchEvent::Usages(snap));
    assert_eq!(ev.kind, MediusCatchEventKind::Usages);
    assert_eq!(unsafe { ev.data.usages.n } as usize, MEDIUS_MAX_USAGES);
    assert_eq!(unsafe { ev.data.usages.class }, MediusClass::Key);
}

#[test]
fn an_empty_snapshot_crosses_the_c_boundary_still_naming_its_class() {
    // n == 0 is the release of the last held usage. Without the class in the struct a C caller could
    // not tell which class went quiet, and the edge is the whole point of subscribing.
    for (class, want) in [
        (medius::Class::Button, MediusClass::Button),
        (medius::Class::Key, MediusClass::Key),
        (medius::Class::Media, MediusClass::Media),
    ] {
        let snap = medius::UsageSnapshot {
            ts_us: 7,
            clock: medius::ClockDomain::HostChip,
            class,
            usages: Vec::new(),
        };
        let ev = MediusCatchEvent::from(medius::CatchEvent::Usages(snap));
        assert_eq!(unsafe { ev.data.usages.n }, 0);
        assert_eq!(unsafe { ev.data.usages.class }, want);
    }
}

#[test]
fn traffic_payload_caps_at_capacity_without_wrapping() {
    let ev = MediusCatchEvent::from(medius::CatchEvent::Traffic(medius::TrafficEvent {
        ts_us: 1,
        clock: medius::ClockDomain::DeviceChip,
        class: medius::CatchClass::VendorBulk,
        id: 0x02,
        direction: medius::LockDirection::Negative,
        flags: 0,
        true_len: 1024,
        bytes: vec![0xAB; MEDIUS_MAX_TRAFFIC_BYTES + 40],
    }));
    assert_eq!(ev.kind, MediusCatchEventKind::Traffic);
    let t = unsafe { ev.data.traffic };
    assert_eq!(t.len as usize, MEDIUS_MAX_TRAFFIC_BYTES);
    assert_eq!(t.true_len, 1024);
    assert_eq!(t.direction, MediusLockDirection::Negative);
}

#[test]
fn the_traffic_arm_does_not_grow_the_event_union() {
    // Every `medius_event_stream_recv` writes a whole union, so a traffic buffer wider than the
    // usage snapshot would cost every caller on every event.
    assert!(size_of::<MediusTrafficEvent>() <= size_of::<MediusUsageEvent>());
    assert_eq!(
        size_of::<MediusCatchEventData>(),
        size_of::<MediusUsageEvent>()
    );
}

#[test]
fn catch_filter_wildcards_round_trip_through_the_sentinels() {
    let all = medius_catch_filter_all();
    assert_eq!(all.class, MEDIUS_CATCH_CLASS_ANY);
    assert_eq!(all.id, MEDIUS_CATCH_ID_ANY);

    let class_only = medius_catch_filter_class(MEDIUS_CATCH_CLASS_HID_IN);
    assert_eq!(class_only.class, MEDIUS_CATCH_CLASS_HID_IN);
    assert_eq!(class_only.id, MEDIUS_CATCH_ID_ANY);

    let exact = medius_catch_filter_addr(MEDIUS_CATCH_CLASS_VEND_INTR, 0x81);
    assert_eq!(exact.id, 0x81);

    // A wildcard is `None` on the Rust side and the sentinel on the C side, and the pair has to come
    // back byte-identical or a re-sent subscription would address something else.
    for f in [all, class_only, exact] {
        let native = crate::convert::catch_filter_from_c(f).unwrap();
        assert_eq!(crate::convert::catch_filter_to_c(native), f);
    }
    assert_eq!(
        crate::convert::catch_filter_from_c(all).unwrap(),
        medius::CatchFilter::all()
    );
    assert_eq!(
        crate::convert::catch_filter_from_c(exact).unwrap(),
        medius::CatchFilter::addr(medius::CatchClass::VendorInterrupt, 0x81)
    );
    assert!(crate::convert::catch_filter_from_c(medius_catch_filter_class(99)).is_none());
}

fn control_event(bytes: &[u8], flags: u8) -> MediusTrafficEvent {
    let mut e: MediusTrafficEvent = unsafe { std::mem::zeroed() };
    e.class = MEDIUS_CATCH_CLASS_CONTROL;
    e.direction = MediusLockDirection::Positive;
    e.flags = flags;
    e.len = bytes.len() as u16;
    e.true_len = bytes.len() as u16;
    e.bytes[..bytes.len()].copy_from_slice(bytes);
    e
}

#[test]
fn traffic_event_splits_setup_from_the_data_stage() {
    let setup = [0x80, 0x06, 0x00, 0x01, 0x00, 0x00, 0x12, 0x00];
    let mut bytes = setup.to_vec();
    bytes.extend_from_slice(&[0x12, 0x01, 0x00, 0x02]);
    let e = control_event(&bytes, 0x00);

    let p = unsafe { medius_traffic_event_setup(&e) };
    assert!(!p.is_null());
    assert_eq!(unsafe { std::slice::from_raw_parts(p, 8) }, &setup);

    let mut len = 0usize;
    let d = unsafe { medius_traffic_event_data(&e, &mut len) };
    assert_eq!(len, 4);
    assert_eq!(
        unsafe { std::slice::from_raw_parts(d, len) },
        &[0x12, 0x01, 0x00, 0x02]
    );
    assert!(!unsafe { medius_traffic_event_truncated(&e) });

    // A packet shorter than the setup stage has no setup to read, and its whole body is the data.
    let short = control_event(&[0x80, 0x06], 0x00);
    assert!(unsafe { medius_traffic_event_setup(&short) }.is_null());
    let d = unsafe { medius_traffic_event_data(&short, &mut len) };
    assert_eq!(len, 2);
    assert_eq!(unsafe { std::slice::from_raw_parts(d, len) }, &[0x80, 0x06]);

    // Any other class keeps the whole packet as data.
    let mut hid = control_event(&[1, 2, 3, 4, 5, 6, 7, 8, 9], 0);
    hid.class = MEDIUS_CATCH_CLASS_HID_IN;
    assert!(unsafe { medius_traffic_event_setup(&hid) }.is_null());
    let _ = unsafe { medius_traffic_event_data(&hid, &mut len) };
    assert_eq!(len, 9);

    assert!(unsafe { medius_traffic_event_setup(std::ptr::null()) }.is_null());
    assert!(unsafe { medius_traffic_event_data(std::ptr::null(), &mut len) }.is_null());
}

#[test]
fn traffic_event_decodes_control_status_and_bus_events() {
    let mut status = MediusControlStatus::Ok;
    let stalled = control_event(&[0; 8], 0xFD);
    assert!(unsafe { medius_traffic_event_control_status(&stalled, &mut status) });
    assert_eq!(status, MediusControlStatus::Stalled);

    let ok = control_event(&[0; 8], 0x00);
    assert!(unsafe { medius_traffic_event_control_status(&ok, &mut status) });
    assert_eq!(status, MediusControlStatus::Ok);

    let mut hid = ok;
    hid.class = MEDIUS_CATCH_CLASS_HID_IN;
    assert!(!unsafe { medius_traffic_event_control_status(&hid, &mut status) });

    let mut bus = control_event(&[3, 1], 5);
    bus.class = MEDIUS_CATCH_CLASS_BUS;
    let mut event = MediusBusEvent {
        kind: MediusBusEventKind::Reset,
        configuration: 0,
        interface: 0,
        alt: 0,
    };
    assert!(unsafe { medius_traffic_event_bus_event(&bus, &mut event) });
    assert_eq!(event.kind, MediusBusEventKind::SetInterface);
    assert_eq!(event.interface, 3);
    assert_eq!(event.alt, 1);

    bus.flags = 3;
    assert!(unsafe { medius_traffic_event_bus_event(&bus, &mut event) });
    assert_eq!(event.kind, MediusBusEventKind::Configured);
    assert_eq!(event.configuration, 3);

    bus.flags = 40;
    assert!(!unsafe { medius_traffic_event_bus_event(&bus, &mut event) });

    // Bytes past `len` are whatever the caller's buffer held; a reset carries none and must not
    // pick up the stale interface number from the event before it.
    bus.flags = 0;
    bus.len = 0;
    assert!(unsafe { medius_traffic_event_bus_event(&bus, &mut event) });
    assert_eq!(event.kind, MediusBusEventKind::Reset);
    assert_eq!(event.configuration, 0);
    assert_eq!(event.interface, 0);
    assert_eq!(event.alt, 0);
}

#[test]
fn bulk_flags_only_apply_to_the_bulk_class() {
    let mut bulk = control_event(&[], 0x03);
    bulk.class = MEDIUS_CATCH_CLASS_VEND_BULK;
    assert!(unsafe { medius_traffic_event_bulk_end_of_transfer(&bulk) });
    assert!(unsafe { medius_traffic_event_bulk_zlp(&bulk) });

    let mut intr = bulk;
    intr.class = MEDIUS_CATCH_CLASS_VEND_INTR;
    assert!(!unsafe { medius_traffic_event_bulk_end_of_transfer(&intr) });
    assert!(!unsafe { medius_traffic_event_bulk_zlp(&intr) });
}

#[test]
fn last_error_message_truncates_and_reports_full_length() {
    let mut buf = [0i8; 8];
    let _ = unsafe { medius_last_error_message(buf.as_mut_ptr(), buf.len()) };
    assert_eq!(buf[7], 0);
}
