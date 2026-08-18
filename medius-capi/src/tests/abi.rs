//! Full-surface tests through the mock box: the same operation via the native crate and the C ABI must record byte-identical frames.

use std::ptr;
use std::time::Duration;

use std::os::raw::c_char;

use medius::{DecodedFrame, Device, MockBox};

use crate::*;

fn cname(s: &str) -> [c_char; MEDIUS_MAX_NAME] {
    let mut buf = [0; MEDIUS_MAX_NAME];
    for (i, b) in s.bytes().take(MEDIUS_MAX_NAME - 1).enumerate() {
        buf[i] = b as c_char;
    }
    buf
}

fn read_cname(buf: &[c_char]) -> String {
    let bytes: Vec<u8> = buf
        .iter()
        .take_while(|&&c| c != 0)
        .map(|&c| c as u8)
        .collect();
    String::from_utf8_lossy(&bytes).into_owned()
}

fn native_frames(f: impl FnOnce(&Device)) -> Vec<DecodedFrame> {
    let mock = MockBox::new();
    let dev = Device::with_mock(mock.clone());
    f(&dev);
    mock.recorded_frames()
}

unsafe fn capi_frames(f: impl FnOnce(*mut MediusDevice)) -> Vec<DecodedFrame> {
    let mock = medius_mock_new();
    let mut dev: *mut MediusDevice = ptr::null_mut();
    assert_eq!(
        unsafe { medius_device_with_mock(mock, &mut dev) },
        MediusStatus::Ok
    );
    f(dev);
    let frames = unsafe { (*mock).inner.recorded_frames() };
    unsafe {
        medius_device_free(dev);
        medius_mock_free(mock);
    }
    frames
}

fn assert_parity(native: impl FnOnce(&Device), capi: impl FnOnce(*mut MediusDevice)) {
    let want = native_frames(native);
    let got = unsafe { capi_frames(capi) };
    assert_eq!(want, got, "C ABI frames differ from the native crate");
}

#[test]
fn move_rel_parity() {
    assert_parity(
        |d| {
            d.move_rel(100, -50).unwrap();
        },
        |dev| unsafe {
            assert_eq!(medius_device_move_rel(dev, 100, -50), MediusStatus::Ok);
        },
    );
}

#[test]
fn wheel_parity() {
    assert_parity(
        |d| {
            d.wheel(3).unwrap();
        },
        |dev| unsafe {
            assert_eq!(medius_device_wheel(dev, 3), MediusStatus::Ok);
        },
    );
}

#[test]
fn move_axis_parity() {
    assert_parity(
        |d| {
            d.move_axis(
                medius::Motion::Cursor { dx: 7, dy: -9 },
                medius::MoveTiming::Ride,
                medius::PendingMotion::Keep,
            )
            .unwrap();
        },
        |dev| unsafe {
            assert_eq!(
                medius_device_move_axis(
                    dev,
                    medius_motion_cursor(7, -9),
                    MediusMoveTiming::Ride,
                    MediusPendingMotion::Keep
                ),
                MediusStatus::Ok
            );
        },
    );
}

#[test]
fn move_riding_override_parity() {
    assert_parity(
        |d| {
            d.move_rel_now(7, -9).unwrap();
            d.wheel_now(2).unwrap();
            d.flush_motion().unwrap();
            d.discard_motion().unwrap();
            d.move_axis(
                medius::Motion::Cursor { dx: 5, dy: 5 },
                medius::MoveTiming::Now,
                medius::PendingMotion::Flush,
            )
            .unwrap();
        },
        |dev| unsafe {
            assert_eq!(medius_device_move_rel_now(dev, 7, -9), MediusStatus::Ok);
            assert_eq!(medius_device_wheel_now(dev, 2), MediusStatus::Ok);
            assert_eq!(medius_device_flush_motion(dev), MediusStatus::Ok);
            assert_eq!(medius_device_discard_motion(dev), MediusStatus::Ok);
            assert_eq!(
                medius_device_move_axis(
                    dev,
                    medius_motion_cursor(5, 5),
                    MediusMoveTiming::Now,
                    MediusPendingMotion::Flush
                ),
                MediusStatus::Ok
            );
        },
    );
}

#[test]
fn inject_button_parity() {
    assert_parity(
        |d| {
            d.inject(medius::Button::Right, medius::Action::Press)
                .unwrap();
        },
        |dev| unsafe {
            assert_eq!(
                medius_device_inject(
                    dev,
                    medius_usage_button(MediusButton::Right),
                    MediusAction::Press
                ),
                MediusStatus::Ok
            );
        },
    );
}

#[test]
fn press_release_parity() {
    assert_parity(
        |d| {
            d.press(medius::Button::Left).unwrap();
            d.release(medius::Button::Left).unwrap();
            d.force_release(medius::Button::Left).unwrap();
        },
        |dev| unsafe {
            let left = medius_usage_button(MediusButton::Left);
            assert_eq!(medius_device_press(dev, left), MediusStatus::Ok);
            assert_eq!(medius_device_soft_release(dev, left), MediusStatus::Ok);
            assert_eq!(medius_device_force_release(dev, left), MediusStatus::Ok);
        },
    );
}

#[test]
fn inject_key_parity() {
    assert_parity(
        |d| {
            d.inject(medius::Key::new(MEDIUS_KEY_A), medius::Action::Press)
                .unwrap();
        },
        |dev| unsafe {
            assert_eq!(
                medius_device_inject(dev, medius_usage_key(MEDIUS_KEY_A), MediusAction::Press),
                MediusStatus::Ok
            );
        },
    );
}

#[test]
fn press_release_key_parity() {
    assert_parity(
        |d| {
            d.press(medius::Key::ENTER).unwrap();
            d.release(medius::Key::ENTER).unwrap();
        },
        |dev| unsafe {
            let enter = medius_usage_key(MEDIUS_KEY_ENTER);
            assert_eq!(medius_device_press(dev, enter), MediusStatus::Ok);
            assert_eq!(medius_device_soft_release(dev, enter), MediusStatus::Ok);
        },
    );
}

#[test]
fn press_media_parity() {
    assert_parity(
        |d| {
            d.press(medius::MediaKey::VOLUME_UP).unwrap();
        },
        |dev| unsafe {
            assert_eq!(
                medius_device_press(dev, medius_usage_media(MEDIUS_MEDIA_VOLUME_UP)),
                MediusStatus::Ok
            );
        },
    );
}

#[test]
fn lock_parity() {
    assert_parity(
        |d| {
            d.lock(medius::Axis::X, medius::Direction::Both).unwrap();
            d.lock(medius::Button::Side1, medius::Direction::Positive)
                .unwrap();
            d.unlock(medius::Axis::X, medius::Direction::Both).unwrap();
        },
        |dev| unsafe {
            let x = medius_lock_target_axis(MediusLockTargetKind::X);
            let side1 = medius_lock_target_usage(medius_usage_button(MediusButton::Side1));
            assert_eq!(
                medius_device_lock(dev, x, MediusDirection::Both),
                MediusStatus::Ok
            );
            assert_eq!(
                medius_device_lock(dev, side1, MediusDirection::Positive),
                MediusStatus::Ok
            );
            assert_eq!(
                medius_device_unlock(dev, x, MediusDirection::Both),
                MediusStatus::Ok
            );
        },
    );
}

#[test]
fn lock_all_and_led_parity() {
    assert_parity(
        |d| {
            d.lock_all(medius::Blanket::Buttons, medius::Direction::Both)
                .unwrap();
            d.led(medius::LedTarget::Both, medius::LedMode::Blink, 128)
                .unwrap();
        },
        |dev| unsafe {
            assert_eq!(
                medius_device_lock_all(dev, MediusBlanket::Buttons, MediusDirection::Both),
                MediusStatus::Ok
            );
            assert_eq!(
                medius_device_led(dev, MediusLedTarget::Both, MediusLedMode::Blink, 128),
                MediusStatus::Ok
            );
        },
    );
}

#[test]
fn admin_and_options_parity() {
    assert_parity(
        |d| {
            d.reset().unwrap();
            d.reboot(medius::RebootTarget::DeviceRun).unwrap();
            d.allow_imperfect_clones(true).unwrap();
            d.set_movement_riding(Some(Duration::from_millis(5)))
                .unwrap();
            d.set_movement_riding(None).unwrap();
            d.set_name("rig-3").unwrap();
            d.clear_name().unwrap();
        },
        |dev| unsafe {
            assert_eq!(medius_device_reset(dev), MediusStatus::Ok);
            assert_eq!(
                medius_device_reboot(dev, MediusRebootTarget::DeviceRun),
                MediusStatus::Ok
            );
            assert_eq!(
                medius_device_allow_imperfect_clones(dev, true),
                MediusStatus::Ok
            );
            assert_eq!(
                medius_device_set_movement_riding(dev, true, 5),
                MediusStatus::Ok
            );
            assert_eq!(
                medius_device_set_movement_riding(dev, false, 0),
                MediusStatus::Ok
            );
            assert_eq!(
                medius_device_set_name(dev, c"rig-3".as_ptr()),
                MediusStatus::Ok
            );
            assert_eq!(medius_device_clear_name(dev), MediusStatus::Ok);
        },
    );
}

#[test]
fn query_version_returns_configured_value() {
    let mock = medius_mock_new();
    unsafe {
        medius_mock_set_version(
            mock,
            MediusVersion {
                proto_ver: 2,
                fw_major: 9,
                fw_minor: 8,
                fw_patch: 7,
                mac: [0x5A, 0x4E, 0x00, 0x11, 0x1e, 0x28],
                name: cname("Left PC"),
            },
        );
    }
    let mut dev: *mut MediusDevice = ptr::null_mut();
    assert_eq!(
        unsafe { medius_device_with_mock(mock, &mut dev) },
        MediusStatus::Ok
    );
    let mut version = MediusVersion {
        proto_ver: 0,
        fw_major: 0,
        fw_minor: 0,
        fw_patch: 0,
        mac: [0; 6],
        name: [0; MEDIUS_MAX_NAME],
    };
    assert_eq!(
        unsafe { medius_device_query_version(dev, &mut version) },
        MediusStatus::Ok
    );
    assert_eq!(version.fw_major, 9);
    assert_eq!(version.fw_minor, 8);
    assert_eq!(version.fw_patch, 7);
    assert_eq!(version.mac, [0x5A, 0x4E, 0x00, 0x11, 0x1e, 0x28]);
    assert_eq!(read_cname(&version.name), "Left PC");
    unsafe {
        medius_device_free(dev);
        medius_mock_free(mock);
    }
}

#[test]
fn device_info_roundtrips_kind_and_product() {
    let mock = medius_mock_new();
    let mut product = [0 as std::os::raw::c_char; MEDIUS_MAX_PRODUCT];
    for (slot, &byte) in product.iter_mut().zip(b"Razer Mamba Elite".iter()) {
        *slot = byte as std::os::raw::c_char;
    }
    unsafe {
        medius_mock_set_device_info(
            mock,
            MediusDeviceInfo {
                vid: 0x1532,
                pid: 0x0072,
                bcd_device: 0x0200,
                bcd_usb: 0x0200,
                has_serial: 1,
                has_bos: 0,
                kind: MediusDeviceKind::Mouse,
                product,
            },
        );
    }
    let mut dev: *mut MediusDevice = ptr::null_mut();
    assert_eq!(
        unsafe { medius_device_with_mock(mock, &mut dev) },
        MediusStatus::Ok
    );
    let mut info = MediusDeviceInfo {
        vid: 0,
        pid: 0,
        bcd_device: 0,
        bcd_usb: 0,
        has_serial: 0,
        has_bos: 0,
        kind: MediusDeviceKind::Unknown,
        product: [0; MEDIUS_MAX_PRODUCT],
    };
    assert_eq!(
        unsafe { medius_device_device_info(dev, &mut info) },
        MediusStatus::Ok
    );
    assert_eq!(info.vid, 0x1532);
    assert_eq!(info.pid, 0x0072);
    assert_eq!(info.kind, MediusDeviceKind::Mouse);
    assert_eq!(info.has_serial, 1);
    let got: Vec<u8> = info
        .product
        .iter()
        .take_while(|&&c| c != 0)
        .map(|&c| c as u8)
        .collect();
    assert_eq!(&got, b"Razer Mamba Elite");
    unsafe {
        medius_device_free(dev);
        medius_mock_free(mock);
    }
}

#[test]
fn query_locks_roundtrips_through_is_locked() {
    let mock = medius_mock_new();
    let x = medius_lock_target_axis(MediusLockTargetKind::X);
    let mut set: MediusLocks = unsafe { std::mem::zeroed() };
    set.n = 2;
    set.entries[0] = MediusLockEntry {
        target: x,
        is_blanket: false,
        direction: MediusDirection::Positive,
        scale: MEDIUS_LOCK_SCALE_BLOCK,
    };
    set.entries[1] = MediusLockEntry {
        target: x,
        is_blanket: false,
        direction: MediusDirection::Negative,
        scale: MEDIUS_LOCK_SCALE_BLOCK,
    };
    unsafe { medius_mock_set_locks(mock, set) };
    let mut dev: *mut MediusDevice = ptr::null_mut();
    assert_eq!(
        unsafe { medius_device_with_mock(mock, &mut dev) },
        MediusStatus::Ok
    );
    let mut locks: MediusLocks = unsafe { std::mem::zeroed() };
    assert_eq!(
        unsafe { medius_device_query_locks(dev, &mut locks) },
        MediusStatus::Ok
    );
    assert!(unsafe { medius_locks_is_locked(&locks, x, MediusDirection::Both) });
    unsafe {
        medius_device_free(dev);
        medius_mock_free(mock);
    }
}

#[test]
fn scale_and_bearing_cross_the_boundary() {
    let mock = medius_mock_new();
    let mut dev: *mut MediusDevice = ptr::null_mut();
    assert_eq!(
        unsafe { medius_device_with_mock(mock, &mut dev) },
        MediusStatus::Ok
    );
    let x = medius_lock_target_axis(MediusLockTargetKind::X);
    assert_eq!(
        unsafe { medius_device_scale(dev, x, MediusDirection::Against, 40) },
        MediusStatus::Ok
    );
    assert_eq!(
        unsafe { medius_device_scale_all(dev, MediusBlanket::Aim, MediusDirection::With, 130) },
        MediusStatus::Ok
    );
    assert_eq!(
        unsafe { medius_device_set_bearing(dev, 20, MediusBearingMode::Vector) },
        MediusStatus::Ok
    );
    // The mock answers the default bearing, which is what the box boots holding.
    let mut bearing: MediusBearing = unsafe { std::mem::zeroed() };
    assert_eq!(
        unsafe { medius_device_query_bearing(dev, &mut bearing) },
        MediusStatus::Ok
    );
    assert_eq!(bearing.mode, MediusBearingMode::PerAxis);
    unsafe {
        medius_device_free(dev);
        medius_mock_free(mock);
    }
}

#[test]
fn counters_are_readable() {
    let mock = medius_mock_new();
    let mut dev: *mut MediusDevice = ptr::null_mut();
    assert_eq!(
        unsafe { medius_device_with_mock(mock, &mut dev) },
        MediusStatus::Ok
    );
    unsafe { assert_eq!(medius_device_move_rel(dev, 1, 0), MediusStatus::Ok) };
    let mut counters = MediusCountersSnapshot {
        frames_tx: 0,
        frames_rx: 0,
        crc_drops: 0,
        reconnects: 0,
    };
    assert_eq!(
        unsafe { medius_device_counters(dev, &mut counters) },
        MediusStatus::Ok
    );
    assert!(counters.frames_tx >= 1);
    unsafe {
        medius_device_free(dev);
        medius_mock_free(mock);
    }
}

fn zeroed_event() -> MediusCatchEvent {
    // Safe: every arm of the union is plain-old-data, and we overwrite it before reading.
    unsafe { std::mem::zeroed() }
}

unsafe fn subscribe(
    dev: *mut MediusDevice,
    filters: &[MediusCatchFilter],
) -> *mut MediusEventStream {
    let mut stream: *mut MediusEventStream = ptr::null_mut();
    assert_eq!(
        unsafe { medius_device_catch_events(dev, filters.as_ptr(), filters.len(), &mut stream) },
        MediusStatus::Ok
    );
    stream
}

#[test]
fn catch_delivers_a_motion_event() {
    let mock = medius_mock_new();
    let mut dev: *mut MediusDevice = ptr::null_mut();
    assert_eq!(
        unsafe { medius_device_with_mock(mock, &mut dev) },
        MediusStatus::Ok
    );
    let stream = unsafe { subscribe(dev, &[medius_catch_filter_everything()]) };
    unsafe {
        (*mock).inner.push_motion(1, 7_000, 12, -34, 1);
    }
    let mut event = zeroed_event();
    assert!(unsafe { medius_event_stream_recv_timeout(stream, 2000, &mut event) });
    assert_eq!(event.kind, MediusCatchEventKind::Motion);
    assert_eq!(event.ts_us, 7_000);
    assert_eq!(event.clock, MediusClockDomain::HostChip);
    let m = unsafe { event.data.motion };
    assert_eq!(m.dx, 12);
    assert_eq!(m.dy, -34);
    assert_eq!(m.dz, 1);
    unsafe {
        medius_event_stream_free(stream);
        medius_device_free(dev);
        medius_mock_free(mock);
    }
}

#[test]
fn catch_delivers_a_usage_event() {
    let mock = medius_mock_new();
    let mut dev: *mut MediusDevice = ptr::null_mut();
    assert_eq!(
        unsafe { medius_device_with_mock(mock, &mut dev) },
        MediusStatus::Ok
    );
    let stream = unsafe { subscribe(dev, &[medius_catch_filter_watch_class(MediusClass::Key)]) };
    unsafe {
        (*mock).inner.push_usages(
            1,
            7_000,
            medius::Class::Key,
            medius::Direction::Positive,
            &[medius::Usage::from(medius::Key::ESCAPE)],
        );
    }
    let mut event = zeroed_event();
    assert!(unsafe { medius_event_stream_recv_timeout(stream, 2000, &mut event) });
    assert_eq!(event.kind, MediusCatchEventKind::Usages);
    let usages = unsafe { event.data.usages };
    assert!(unsafe { medius_usage_event_is_held(&usages, medius_usage_key(MEDIUS_KEY_ESCAPE)) });
    unsafe {
        medius_event_stream_free(stream);
        medius_device_free(dev);
        medius_mock_free(mock);
    }
}

#[test]
fn catch_delivers_a_traffic_event() {
    let mock = medius_mock_new();
    let mut dev: *mut MediusDevice = ptr::null_mut();
    assert_eq!(
        unsafe { medius_device_with_mock(mock, &mut dev) },
        MediusStatus::Ok
    );
    let stream = unsafe {
        subscribe(
            dev,
            &[medius_catch_filter_traffic(
                MEDIUS_CATCH_CLASS_VENDOR_BULK,
                0x83,
            )],
        )
    };
    unsafe {
        (*mock).inner.push_traffic(
            1,
            9_000,
            medius::ClockDomain::DeviceChip,
            medius::CatchClass::VendorBulk,
            0x83,
            medius::Direction::Positive,
            0x01,
            64,
            &[0xDE, 0xAD, 0xBE, 0xEF],
        );
    }
    let mut event = zeroed_event();
    assert!(unsafe { medius_event_stream_recv_timeout(stream, 2000, &mut event) });
    assert_eq!(event.kind, MediusCatchEventKind::Traffic);
    assert_eq!(event.ts_us, 9_000);
    assert_eq!(event.clock, MediusClockDomain::DeviceChip);
    let t = unsafe { event.data.traffic };
    assert_eq!(t.class, MEDIUS_CATCH_CLASS_VENDOR_BULK);
    assert_eq!(t.id, 0x83);
    assert_eq!(t.direction, MediusDirection::Positive);
    assert_eq!(t.flags, 0x01);
    assert_eq!(t.true_len, 64);
    assert_eq!(t.len, 4);
    assert_eq!(&t.bytes[..4], &[0xDE, 0xAD, 0xBE, 0xEF]);
    // true_len above len: the capture was cut by snaplen, not a genuinely short packet.
    assert!(unsafe { medius_traffic_event_truncated(&t) });
    assert!(unsafe { medius_traffic_event_bulk_end_of_transfer(&t) });
    unsafe {
        medius_event_stream_free(stream);
        medius_device_free(dev);
        medius_mock_free(mock);
    }
}

#[test]
fn traffic_event_survives_the_mock_push_round_trip() {
    let mock = medius_mock_new();
    let mut dev: *mut MediusDevice = ptr::null_mut();
    assert_eq!(
        unsafe { medius_device_with_mock(mock, &mut dev) },
        MediusStatus::Ok
    );
    let stream = unsafe { subscribe(dev, &[medius_catch_filter_everything()]) };
    let mut pushed: MediusTrafficEvent = unsafe { std::mem::zeroed() };
    pushed.class = MEDIUS_CATCH_CLASS_CONTROL;
    pushed.id = 0;
    pushed.direction = MediusDirection::Positive;
    pushed.flags = 0xFD;
    pushed.true_len = 10;
    pushed.len = 10;
    pushed.bytes[..10]
        .copy_from_slice(&[0x80, 0x06, 0x00, 0x01, 0x00, 0x00, 0x12, 0x00, 0x12, 0x01]);
    unsafe {
        medius_mock_push_traffic(mock, 2, 4_242, MediusClockDomain::DeviceChip, &pushed);
    }
    let mut event = zeroed_event();
    assert!(unsafe { medius_event_stream_recv_timeout(stream, 2000, &mut event) });
    assert_eq!(event.kind, MediusCatchEventKind::Traffic);
    assert_eq!(event.ts_us, 4_242);
    assert_eq!(event.clock, MediusClockDomain::DeviceChip);
    assert_eq!(unsafe { event.data.traffic }, pushed);
    unsafe {
        medius_event_stream_free(stream);
        medius_device_free(dev);
        medius_mock_free(mock);
    }
}

#[test]
fn catch_events_rejects_an_empty_or_unknown_filter_list() {
    let mock = medius_mock_new();
    let mut dev: *mut MediusDevice = ptr::null_mut();
    assert_eq!(
        unsafe { medius_device_with_mock(mock, &mut dev) },
        MediusStatus::Ok
    );
    let mut stream: *mut MediusEventStream = ptr::null_mut();
    let all = [medius_catch_filter_everything()];
    assert_eq!(
        unsafe { medius_device_catch_events(dev, all.as_ptr(), 0, &mut stream) },
        MediusStatus::ErrInvalidArg
    );
    assert_eq!(
        unsafe { medius_device_catch_events(dev, ptr::null(), 1, &mut stream) },
        MediusStatus::ErrInvalidArg
    );
    // A class the box does not define must fail the whole call: a silently narrower subscription is
    // indistinguishable from a box producing no events.
    let bogus = [medius_catch_filter_traffic_class(200)];
    assert_eq!(
        unsafe { medius_device_catch_events(dev, bogus.as_ptr(), 1, &mut stream) },
        MediusStatus::ErrInvalidArg
    );
    assert!(stream.is_null());
    unsafe {
        medius_device_free(dev);
        medius_mock_free(mock);
    }
}

#[test]
fn input_events_decode_edges_across_the_abi() {
    let mock = medius_mock_new();
    let mut dev: *mut MediusDevice = ptr::null_mut();
    assert_eq!(
        unsafe { medius_device_with_mock(mock, &mut dev) },
        MediusStatus::Ok
    );
    let mut filters = [medius_catch_filter_everything(); 4];
    unsafe { medius_catch_filter_all_input(filters.as_mut_ptr()) };
    let mut stream: *mut MediusInputStream = ptr::null_mut();
    assert_eq!(
        unsafe { medius_device_input_events(dev, filters.as_ptr(), filters.len(), &mut stream) },
        MediusStatus::Ok
    );
    let esc = medius::Usage::from(medius::Key::ESCAPE);
    unsafe {
        (*mock).inner.push_usages(
            1,
            7_000,
            medius::Class::Key,
            medius::Direction::PRESS,
            &[esc],
        );
        (*mock).inner.push_usages(
            2,
            8_000,
            medius::Class::Key,
            medius::Direction::RELEASE,
            &[],
        );
        (*mock).inner.push_motion(3, 9_000, 4, -5, 0);
    }
    let mut ev: MediusInputEvent = unsafe { std::mem::zeroed() };
    assert!(unsafe { medius_input_stream_recv_timeout(stream, 2000, &mut ev) });
    assert_eq!(ev.kind, MediusInputKind::Press);
    assert_eq!(ev.ts_us, 7_000);
    assert_eq!(ev.clock, MediusClockDomain::HostChip);
    assert_eq!(ev.usage, medius_usage_key(MEDIUS_KEY_ESCAPE));
    // The unused arm is zeroed, not left as whatever the caller's buffer held.
    assert_eq!((ev.dx, ev.dy, ev.dz), (0, 0, 0));

    assert!(unsafe { medius_input_stream_recv_timeout(stream, 2000, &mut ev) });
    assert_eq!(ev.kind, MediusInputKind::Release);
    assert_eq!(ev.usage, medius_usage_key(MEDIUS_KEY_ESCAPE));

    assert!(unsafe { medius_input_stream_recv_timeout(stream, 2000, &mut ev) });
    assert_eq!(ev.kind, MediusInputKind::Motion);
    assert_eq!((ev.dx, ev.dy, ev.dz), (4, -5, 0));

    // Nothing is held once the release has been reported.
    let mut held = [MediusUsage {
        kind: MediusClass::Button,
        id: 0,
    }; 4];
    let n = unsafe { medius_input_stream_held(stream, MediusClass::Key, held.as_mut_ptr(), 4) };
    assert_eq!(n, 0);
    assert!(!unsafe { medius_input_stream_try_recv(stream, &mut ev) });
    unsafe {
        medius_input_stream_free(stream);
        medius_device_free(dev);
        medius_mock_free(mock);
    }
}

#[test]
fn input_events_report_each_refusal_with_its_own_status() {
    // Folding these into ErrUnknown would leave a C caller with a failed subscribe and no way to
    // tell a wrong filter from a dead link.
    let mock = medius_mock_new();
    let mut dev: *mut MediusDevice = ptr::null_mut();
    assert_eq!(
        unsafe { medius_device_with_mock(mock, &mut dev) },
        MediusStatus::Ok
    );
    let mut stream: *mut MediusInputStream = ptr::null_mut();
    let call = |f: &[MediusCatchFilter], stream: &mut *mut MediusInputStream| unsafe {
        medius_device_input_events(dev, f.as_ptr(), f.len(), stream)
    };
    assert_eq!(
        call(
            &[medius_catch_filter_traffic_class(
                MEDIUS_CATCH_CLASS_VENDOR_BULK
            )],
            &mut stream
        ),
        MediusStatus::ErrNotAnInputFilter
    );
    assert_eq!(
        call(&[medius_catch_filter_everything()], &mut stream),
        MediusStatus::ErrWildcardNotInput
    );
    assert_eq!(
        call(
            &[medius_catch_filter_on_press(medius_catch_filter_watch(
                medius_usage_key(MEDIUS_KEY_ESCAPE)
            ))],
            &mut stream
        ),
        MediusStatus::ErrHalfEdgeInputFilter
    );
    // And the capture refusal on the catch side, which is the same class of mistake.
    let mut ev_stream: *mut MediusEventStream = ptr::null_mut();
    let capped =
        medius_catch_filter_with_capture(medius_catch_filter_watch_class(MediusClass::Key), 8);
    assert_eq!(
        unsafe { medius_device_catch_events(dev, &capped, 1, &mut ev_stream) },
        MediusStatus::ErrCaptureNotApplicable
    );
    assert!(stream.is_null() && ev_stream.is_null());
    unsafe {
        medius_device_free(dev);
        medius_mock_free(mock);
    }
}

#[test]
fn every_new_entry_point_survives_a_null_and_respects_the_caller_s_buffer() {
    // A dropped null check is an abort inside the caller's process, and an off-by-one in the one
    // entry point that writes an unbounded-length result into a caller buffer is a heap smash. Both
    // mutations passed the whole suite before this test existed.
    let mock = medius_mock_new();
    let mut dev: *mut MediusDevice = ptr::null_mut();
    assert_eq!(
        unsafe { medius_device_with_mock(mock, &mut dev) },
        MediusStatus::Ok
    );
    let mut filters = [medius_catch_filter_everything(); 4];
    unsafe { medius_catch_filter_all_input(filters.as_mut_ptr()) };
    let mut stream: *mut MediusInputStream = ptr::null_mut();
    assert_eq!(
        unsafe { medius_device_input_events(dev, filters.as_ptr(), filters.len(), &mut stream) },
        MediusStatus::Ok
    );

    // Nulls, everywhere they can be passed.
    let mut ev: MediusInputEvent = unsafe { std::mem::zeroed() };
    let mut st: MediusStamped = unsafe { std::mem::zeroed() };
    unsafe {
        medius_catch_filter_all_input(ptr::null_mut());
        assert_eq!(
            medius_device_input_events(dev, filters.as_ptr(), 4, ptr::null_mut()),
            MediusStatus::ErrInvalidArg
        );
        assert_eq!(
            medius_input_stream_recv(ptr::null_mut(), &mut ev),
            MediusStatus::ErrInvalidArg
        );
        assert_eq!(
            medius_input_stream_recv(stream, ptr::null_mut()),
            MediusStatus::ErrInvalidArg
        );
        assert!(!medius_input_stream_try_recv(stream, ptr::null_mut()));
        assert!(!medius_input_stream_try_recv(ptr::null_mut(), &mut ev));
        assert!(!medius_input_stream_recv_timeout(
            stream,
            0,
            ptr::null_mut()
        ));
        assert_eq!(medius_input_stream_dropped(ptr::null_mut()), 0);
        assert!(!medius_input_stream_is_connected(ptr::null_mut()));
        assert!(medius_input_stream_is_connected(stream));
        assert_eq!(
            medius_input_stream_held(ptr::null_mut(), MediusClass::Key, ptr::null_mut(), 0),
            0
        );
        assert!(!medius_timeline_observe_input(
            ptr::null_mut(),
            &ev,
            0,
            &mut st
        ));
        medius_input_stream_free(ptr::null_mut());
        medius_timeline_free(ptr::null_mut());
    }

    // `held` must never write past `cap`, and must still report the true count so a caller can grow.
    let esc = medius::Usage::from(medius::Key::ESCAPE);
    let a = medius::Usage::from(medius::Key::A);
    unsafe {
        (*mock).inner.push_usages(
            1,
            1_000,
            medius::Class::Key,
            medius::Direction::PRESS,
            &[esc, a],
        );
    }
    assert!(unsafe { medius_input_stream_recv_timeout(stream, 2000, &mut ev) });
    assert!(unsafe { medius_input_stream_recv_timeout(stream, 2000, &mut ev) });

    const CANARY: u16 = 0xBEEF;
    let mut buf = [MediusUsage {
        kind: MediusClass::Button,
        id: CANARY,
    }; 4];
    let n = unsafe { medius_input_stream_held(stream, MediusClass::Key, buf.as_mut_ptr(), 1) };
    assert_eq!(
        n, 2,
        "the true count comes back even when the buffer is short"
    );
    assert_eq!(buf[1].id, CANARY, "nothing was written past cap");
    assert_eq!(buf[2].id, CANARY);
    // cap = 0 with a real pointer writes nothing at all.
    let mut none = [MediusUsage {
        kind: MediusClass::Button,
        id: CANARY,
    }; 2];
    assert_eq!(
        unsafe { medius_input_stream_held(stream, MediusClass::Key, none.as_mut_ptr(), 0) },
        2
    );
    assert_eq!(none[0].id, CANARY);
    // A null out is a size query.
    assert_eq!(
        unsafe { medius_input_stream_held(stream, MediusClass::Key, ptr::null_mut(), 4) },
        2
    );

    unsafe {
        medius_input_stream_free(stream);
        medius_device_free(dev);
        medius_mock_free(mock);
    }
}

#[test]
fn the_timeline_unwraps_and_maps_onto_the_callers_clock() {
    let t = medius_timeline_new();
    assert!(!t.is_null());
    let mut ev = zeroed_event();
    ev.kind = MediusCatchEventKind::Motion;
    ev.clock = MediusClockDomain::HostChip;

    let mut a: MediusStamped = unsafe { std::mem::zeroed() };
    ev.ts_us = u32::MAX - 500;
    assert!(unsafe { medius_timeline_observe(t, &ev, 50_000_000, &mut a) });
    assert_eq!(a.box_us, (u32::MAX - 500) as u64);
    assert_eq!(a.excess_ns, 0);

    // Past the rollover: 32 bits of microseconds is 71.6 minutes, and a raw subtraction here would
    // come out about 4295 seconds negative.
    let mut b: MediusStamped = unsafe { std::mem::zeroed() };
    ev.ts_us = 500;
    assert!(unsafe { medius_timeline_observe(t, &ev, 51_001_000, &mut b) });
    assert_eq!(b.box_us, (1u64 << 32) + 500);
    assert!(b.host_ns > a.host_ns, "{} !> {}", b.host_ns, a.host_ns);
    assert_eq!(b.host_ns - a.host_ns, 1_001_000, "1001 us on the box");
    assert_eq!(
        unsafe { medius_timeline_samples(t, MediusClockDomain::HostChip) },
        2
    );

    // A reboot restarts the clock at zero, which is not a wrap.
    unsafe { medius_timeline_reset(t, MediusClockDomain::HostChip) };
    let mut c: MediusStamped = unsafe { std::mem::zeroed() };
    ev.ts_us = 7;
    assert!(unsafe { medius_timeline_observe(t, &ev, 60_000_000, &mut c) });
    assert_eq!(c.box_us, 7);
    assert!(!unsafe { medius_timeline_observe(ptr::null_mut(), &ev, 0, &mut c) });
    unsafe { medius_timeline_free(t) };
}

#[test]
fn query_catch_returns_the_table_and_the_clock_estimate() {
    let mock = medius_mock_new();
    let mut set: MediusCatchState = unsafe { std::mem::zeroed() };
    set.table_full = 1;
    set.dropped = 77;
    set.clock = MediusClockEstimate {
        offset_us: -1234,
        rate_ppb: 56,
        delay_us: 40,
        age_ms: 250,
    };
    set.n = 2;
    set.entries[0] = MediusCatchEntry {
        filter: MediusCatchFilter {
            capture: 16,
            ..medius_catch_filter_everything()
        },
        dropped: 3,
    };
    set.entries[1] = MediusCatchEntry {
        filter: medius_catch_filter_traffic(MEDIUS_CATCH_CLASS_CONTROL, 0),
        dropped: 0,
    };
    unsafe { medius_mock_set_catch_state(mock, set) };
    let mut dev: *mut MediusDevice = ptr::null_mut();
    assert_eq!(
        unsafe { medius_device_with_mock(mock, &mut dev) },
        MediusStatus::Ok
    );
    let mut got: MediusCatchState = unsafe { std::mem::zeroed() };
    assert_eq!(
        unsafe { medius_device_query_catch(dev, &mut got) },
        MediusStatus::Ok
    );
    assert_eq!(got.table_full, 1);
    assert_eq!(got.dropped, 77);
    assert_eq!(got.clock, set.clock);
    assert_eq!(got.n, 2);
    assert_eq!(got.entries[0], set.entries[0]);
    assert_eq!(got.entries[1], set.entries[1]);
    unsafe {
        medius_device_free(dev);
        medius_mock_free(mock);
    }
}

#[test]
fn no_clock_estimate_is_distinguishable_from_a_zero_age_one() {
    let mock = medius_mock_new();
    let mut set: MediusCatchState = unsafe { std::mem::zeroed() };
    set.clock = MediusClockEstimate {
        offset_us: 0,
        rate_ppb: 0,
        delay_us: 0,
        age_ms: MEDIUS_CLOCK_AGE_NONE,
    };
    unsafe { medius_mock_set_catch_state(mock, set) };
    let mut dev: *mut MediusDevice = ptr::null_mut();
    assert_eq!(
        unsafe { medius_device_with_mock(mock, &mut dev) },
        MediusStatus::Ok
    );
    let mut got: MediusCatchState = unsafe { std::mem::zeroed() };
    assert_eq!(
        unsafe { medius_device_query_catch(dev, &mut got) },
        MediusStatus::Ok
    );
    // Zeroing the struct would have produced age_ms == 0, which means a fresh estimate of exactly
    // zero offset. The sentinel has to survive so a caller does not apply an unmeasured offset.
    assert_eq!(got.clock.age_ms, MEDIUS_CLOCK_AGE_NONE);

    set.clock.age_ms = 0;
    unsafe { medius_mock_set_catch_state(mock, set) };
    assert_eq!(
        unsafe { medius_device_query_catch(dev, &mut got) },
        MediusStatus::Ok
    );
    assert_eq!(got.clock.age_ms, 0);
    unsafe {
        medius_device_free(dev);
        medius_mock_free(mock);
    }
}

#[test]
fn log_stream_delivers_a_line() {
    let mock = medius_mock_new();
    let mut dev: *mut MediusDevice = ptr::null_mut();
    assert_eq!(
        unsafe { medius_device_with_mock(mock, &mut dev) },
        MediusStatus::Ok
    );
    let mut stream: *mut MediusLogStream = ptr::null_mut();
    assert_eq!(
        unsafe { medius_device_logs(dev, &mut stream) },
        MediusStatus::Ok
    );
    unsafe {
        (*mock)
            .inner
            .push_log(medius::LogLevel::Warn, "hello world");
    }
    let mut line: MediusLogLine = unsafe { std::mem::zeroed() };
    assert!(unsafe { medius_log_stream_recv_timeout(stream, 2000, &mut line) });
    assert_eq!(line.level, MediusLogLevel::Warn);
    let text = unsafe { std::ffi::CStr::from_ptr(line.text.as_ptr()) }
        .to_str()
        .unwrap();
    assert_eq!(text, "hello world");
    unsafe {
        medius_log_stream_free(stream);
        medius_device_free(dev);
        medius_mock_free(mock);
    }
}

#[test]
fn silent_mock_fails_the_handshake() {
    let mock = medius_mock_new();
    unsafe { medius_mock_silent(mock) };
    let mut dev: *mut MediusDevice = ptr::null_mut();
    let status = unsafe { medius_device_open_mock(mock, &mut dev) };
    assert_ne!(status, MediusStatus::Ok);
    assert!(dev.is_null());
    let mut buf = [0i8; 128];
    let len = unsafe { medius_last_error_message(buf.as_mut_ptr(), buf.len()) };
    assert!(len > 0);
    unsafe { medius_mock_free(mock) };
}

#[test]
fn bad_proto_version_is_reported() {
    let mock = medius_mock_new();
    unsafe {
        medius_mock_set_version(
            mock,
            MediusVersion {
                proto_ver: 99,
                fw_major: 1,
                fw_minor: 0,
                fw_patch: 0,
                mac: [0; 6],
                name: [0; MEDIUS_MAX_NAME],
            },
        );
    }
    let mut dev: *mut MediusDevice = ptr::null_mut();
    let status = unsafe { medius_device_open_mock(mock, &mut dev) };
    assert_eq!(status, MediusStatus::ErrBadProtoVer);
    assert_eq!(unsafe { medius_last_error_proto_ver() }, 99);
    unsafe { medius_mock_free(mock) };
}

#[test]
fn null_arguments_are_rejected() {
    assert_eq!(
        unsafe { medius_device_move_rel(ptr::null_mut(), 1, 1) },
        MediusStatus::ErrInvalidArg
    );
    let mock = medius_mock_new();
    let mut dev: *mut MediusDevice = ptr::null_mut();
    assert_eq!(
        unsafe { medius_device_with_mock(mock, &mut dev) },
        MediusStatus::Ok
    );
    assert_eq!(
        unsafe { medius_device_query_version(dev, ptr::null_mut()) },
        MediusStatus::ErrInvalidArg
    );
    unsafe {
        medius_device_free(dev);
        medius_mock_free(mock);
    }
}

#[test]
fn recorded_frame_payload_is_readable() {
    let mock = medius_mock_new();
    let mut dev: *mut MediusDevice = ptr::null_mut();
    assert_eq!(
        unsafe { medius_device_with_mock(mock, &mut dev) },
        MediusStatus::Ok
    );
    unsafe { assert_eq!(medius_device_move_rel(dev, 1, 2), MediusStatus::Ok) };
    assert_eq!(unsafe { medius_mock_recorded(mock) }, 1);
    assert!(unsafe { medius_mock_saw(mock, MediusFrameType::Move) });
    let mut ty = MediusFrameType::Reset;
    let mut seq = 0u8;
    let mut payload = [0u8; 64];
    let len = unsafe {
        medius_mock_recorded_frame(
            mock,
            0,
            &mut ty,
            &mut seq,
            payload.as_mut_ptr(),
            payload.len(),
        )
    };
    assert_eq!(ty, MediusFrameType::Move);
    assert!(len > 0);
    unsafe {
        medius_device_free(dev);
        medius_mock_free(mock);
    }
}

#[test]
fn device_and_mock_clone_share_state() {
    let mock = medius_mock_new();
    let mut dev: *mut MediusDevice = ptr::null_mut();
    assert_eq!(
        unsafe { medius_device_with_mock(mock, &mut dev) },
        MediusStatus::Ok
    );
    let dev2 = unsafe { medius_device_clone(dev) };
    assert!(!dev2.is_null());
    unsafe {
        assert_eq!(medius_device_move_rel(dev, 1, 0), MediusStatus::Ok);
        assert_eq!(medius_device_move_rel(dev2, 2, 0), MediusStatus::Ok);
    }
    let mock2 = unsafe { medius_mock_clone(mock) };
    assert_eq!(unsafe { medius_mock_recorded(mock2) }, 2);
    unsafe {
        medius_device_free(dev);
        medius_device_free(dev2);
        medius_mock_free(mock);
        medius_mock_free(mock2);
    }
}

#[test]
fn event_stream_clone_shares_the_subscription() {
    let mock = medius_mock_new();
    let mut dev: *mut MediusDevice = ptr::null_mut();
    assert_eq!(
        unsafe { medius_device_with_mock(mock, &mut dev) },
        MediusStatus::Ok
    );
    let stream = unsafe { subscribe(dev, &[medius_catch_filter_everything()]) };
    let stream2 = unsafe { medius_event_stream_clone(stream) };
    assert!(!stream2.is_null());
    unsafe {
        (*mock).inner.push_motion(1, 7_000, 5, 0, 0);
    }
    let mut event = zeroed_event();
    assert!(unsafe { medius_event_stream_recv_timeout(stream2, 2000, &mut event) });
    assert_eq!(event.kind, MediusCatchEventKind::Motion);
    unsafe {
        medius_event_stream_free(stream);
        medius_event_stream_free(stream2);
        medius_device_free(dev);
        medius_mock_free(mock);
    }
}

#[test]
fn free_null_handles_is_a_noop() {
    unsafe {
        medius_device_free(ptr::null_mut());
        medius_event_stream_free(ptr::null_mut());
        medius_log_stream_free(ptr::null_mut());
        medius_clip_free(ptr::null_mut());
        medius_clip_builder_free(ptr::null_mut());
        medius_mock_free(ptr::null_mut());
    }
}

#[test]
fn clip_control_parity() {
    assert_parity(
        |d| {
            let clip = d.clip();
            clip.set_autolock(&[medius::Blanket::Aim, medius::Blanket::Buttons])
                .unwrap();
            clip.set_loop(true).unwrap();
            clip.set_retain(true).unwrap();
            clip.set_ride(true).unwrap();
            clip.bind(medius::ClipTrigger::new(
                medius::Button::Right,
                medius::Edge::Press,
                medius::ClipAction::Start,
            ))
            .unwrap();
            clip.bind(
                medius::ClipTrigger::new(
                    medius::Key::new(MEDIUS_KEY_A),
                    medius::Edge::Release,
                    medius::ClipAction::Stop,
                )
                .consume(),
            )
            .unwrap();
            clip.bind(medius::ClipTrigger::new(
                medius::MediaKey::new(0xCD),
                medius::Edge::Both,
                medius::ClipAction::Toggle,
            ))
            .unwrap();
            clip.unbind(medius::Button::Right, medius::Edge::Press)
                .unwrap();
            clip.clear_triggers().unwrap();
            clip.start().unwrap();
            clip.stop().unwrap();
        },
        |dev| unsafe {
            let mut clip: *mut MediusClip = ptr::null_mut();
            assert_eq!(medius_device_clip(dev, &mut clip), MediusStatus::Ok);
            let scope = [MediusBlanket::Aim, MediusBlanket::Buttons];
            assert_eq!(
                medius_clip_set_autolock(clip, scope.as_ptr(), scope.len()),
                MediusStatus::Ok
            );
            assert_eq!(medius_clip_set_loop(clip, 1), MediusStatus::Ok);
            assert_eq!(medius_clip_set_retain(clip, 1), MediusStatus::Ok);
            assert_eq!(medius_clip_set_ride(clip, 1), MediusStatus::Ok);
            assert_eq!(
                medius_clip_bind(
                    clip,
                    MediusClipTrigger {
                        on: medius_usage_button(MediusButton::Right),
                        edge: MediusEdge::Press,
                        action: MediusClipAction::Start,
                        consume: 0,
                    }
                ),
                MediusStatus::Ok
            );
            assert_eq!(
                medius_clip_bind(
                    clip,
                    MediusClipTrigger {
                        on: medius_usage_key(MEDIUS_KEY_A),
                        edge: MediusEdge::Release,
                        action: MediusClipAction::Stop,
                        consume: 1,
                    }
                ),
                MediusStatus::Ok
            );
            assert_eq!(
                medius_clip_bind(
                    clip,
                    MediusClipTrigger {
                        on: medius_usage_media(0xCD),
                        edge: MediusEdge::Both,
                        action: MediusClipAction::Toggle,
                        consume: 0,
                    }
                ),
                MediusStatus::Ok
            );
            assert_eq!(
                medius_clip_unbind(
                    clip,
                    medius_usage_button(MediusButton::Right),
                    MediusEdge::Press
                ),
                MediusStatus::Ok
            );
            assert_eq!(medius_clip_clear_triggers(clip), MediusStatus::Ok);
            assert_eq!(medius_clip_start(clip), MediusStatus::Ok);
            assert_eq!(medius_clip_stop(clip), MediusStatus::Ok);
            medius_clip_free(clip);
        },
    );
}

#[test]
fn clip_append_parity() {
    // A clip past MAX_PAYLOAD so both paths chunk into the same whole-entry frames with the same seqs.
    assert_parity(
        |d| {
            let mut b = medius::ClipBuilder::new();
            for _ in 0..150 {
                b.move_by(3, -2);
            }
            b.press(medius::Button::Left);
            b.gap(4);
            b.release(medius::Button::Left);
            d.clip().append(&b).unwrap();
        },
        |dev| unsafe {
            let builder = medius_clip_builder_new();
            for _ in 0..150 {
                assert_eq!(medius_clip_builder_move(builder, 3, -2), MediusStatus::Ok);
            }
            assert_eq!(
                medius_clip_builder_press(builder, medius_usage_button(MediusButton::Left)),
                MediusStatus::Ok
            );
            assert_eq!(medius_clip_builder_gap(builder, 4), MediusStatus::Ok);
            assert_eq!(
                medius_clip_builder_release(builder, medius_usage_button(MediusButton::Left)),
                MediusStatus::Ok
            );
            let mut clip: *mut MediusClip = ptr::null_mut();
            assert_eq!(medius_device_clip(dev, &mut clip), MediusStatus::Ok);
            assert_eq!(medius_clip_append(clip, builder), MediusStatus::Ok);
            medius_clip_free(clip);
            medius_clip_builder_free(builder);
        },
    );
}

#[test]
fn clip_builder_frame_edges_match_native() {
    assert_parity(
        |d| {
            let mut b = medius::ClipBuilder::new();
            b.frame(
                1,
                2,
                -1,
                &[
                    (medius::Button::Left.into(), medius::Action::Press),
                    (medius::Key::new(0x04).into(), medius::Action::Press),
                ],
            );
            d.clip().append(&b).unwrap();
        },
        |dev| unsafe {
            let builder = medius_clip_builder_new();
            let inputs = [
                medius_usage_button(MediusButton::Left),
                medius_usage_key(0x04),
            ];
            let actions = [MediusAction::Press, MediusAction::Press];
            assert_eq!(
                medius_clip_builder_frame(
                    builder,
                    1,
                    2,
                    -1,
                    inputs.as_ptr(),
                    actions.as_ptr(),
                    inputs.len()
                ),
                MediusStatus::Ok
            );
            let mut clip: *mut MediusClip = ptr::null_mut();
            assert_eq!(medius_device_clip(dev, &mut clip), MediusStatus::Ok);
            assert_eq!(medius_clip_append(clip, builder), MediusStatus::Ok);
            medius_clip_free(clip);
            medius_clip_builder_free(builder);
        },
    );
}

#[test]
fn clip_status_query_returns_configured_value() {
    let mock = medius_mock_new();
    let mut status: MediusClipStatus = unsafe { std::mem::zeroed() };
    status.state = MediusClipState::Playing;
    status.free = 512;
    status.total = 40;
    status.played = 16;
    status.ticks = 99;
    status.underruns = 2;
    status.seq_gaps = 1;
    status.held_n = 2;
    status.held[0] = medius_usage_button(MediusButton::Side1);
    status.held[1] = medius_usage_key(MEDIUS_KEY_A);
    unsafe { medius_mock_set_clip_status(mock, status) };
    let mut dev: *mut MediusDevice = ptr::null_mut();
    assert_eq!(
        unsafe { medius_device_with_mock(mock, &mut dev) },
        MediusStatus::Ok
    );
    let mut clip: *mut MediusClip = ptr::null_mut();
    assert_eq!(
        unsafe { medius_device_clip(dev, &mut clip) },
        MediusStatus::Ok
    );
    let mut out: MediusClipStatus = unsafe { std::mem::zeroed() };
    assert_eq!(
        unsafe { medius_clip_query_status(clip, &mut out) },
        MediusStatus::Ok
    );
    assert_eq!(out, status);
    assert_eq!(out.held_n, 2);
    assert_eq!(out.held[0], medius_usage_button(MediusButton::Side1));
    assert_eq!(out.held[1], medius_usage_key(MEDIUS_KEY_A));
    unsafe {
        medius_clip_free(clip);
        medius_device_free(dev);
        medius_mock_free(mock);
    }
}
