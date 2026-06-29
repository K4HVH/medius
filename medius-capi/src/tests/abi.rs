//! Full-surface tests through the mock box. The core check is parity: the same operation driven
//! through the native crate and through the C ABI must record byte-identical frames.

use std::ptr;
use std::time::Duration;

use medius::{DecodedFrame, Device, MockBox};

use crate::*;

/// Frames recorded when `f` drives a native `Device` over a fresh mock.
fn native_frames(f: impl FnOnce(&Device)) -> Vec<DecodedFrame> {
    let mock = MockBox::new();
    let dev = Device::with_mock(mock.clone());
    f(&dev);
    mock.recorded_frames()
}

/// Frames recorded when `f` drives the C ABI device over a fresh mock.
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

/// Assert the native operation and the C ABI operation record identical frames.
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
            d.move_axis(medius::Motion::Cursor { dx: 7, dy: -9 })
                .unwrap();
        },
        |dev| unsafe {
            assert_eq!(
                medius_device_move_axis(dev, medius_motion_cursor(7, -9)),
                MediusStatus::Ok
            );
        },
    );
}

#[test]
fn button_parity() {
    assert_parity(
        |d| {
            d.button(medius::Button::Right, medius::Action::Press)
                .unwrap();
        },
        |dev| unsafe {
            assert_eq!(
                medius_device_button(dev, MediusButton::Right, MediusAction::Press),
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
            d.soft_release(medius::Button::Left).unwrap();
            d.force_release(medius::Button::Left).unwrap();
        },
        |dev| unsafe {
            assert_eq!(
                medius_device_press(dev, MediusButton::Left),
                MediusStatus::Ok
            );
            assert_eq!(
                medius_device_soft_release(dev, MediusButton::Left),
                MediusStatus::Ok
            );
            assert_eq!(
                medius_device_force_release(dev, MediusButton::Left),
                MediusStatus::Ok
            );
        },
    );
}

#[test]
fn inject_parity() {
    assert_parity(
        |d| {
            d.inject(medius::Key::new(MEDIUS_KEY_A), medius::Action::Press)
                .unwrap();
        },
        |dev| unsafe {
            assert_eq!(
                medius_device_inject(dev, medius_input_key(MEDIUS_KEY_A), MediusAction::Press),
                MediusStatus::Ok
            );
        },
    );
}

#[test]
fn keyboard_parity() {
    assert_parity(
        |d| {
            d.key_down(medius::Key::ENTER).unwrap();
            d.key_up(medius::Key::ENTER).unwrap();
        },
        |dev| unsafe {
            assert_eq!(
                medius_device_key_down(dev, MEDIUS_KEY_ENTER),
                MediusStatus::Ok
            );
            assert_eq!(
                medius_device_key_up(dev, MEDIUS_KEY_ENTER),
                MediusStatus::Ok
            );
        },
    );
}

#[test]
fn media_parity() {
    assert_parity(
        |d| {
            d.media_down(medius::MediaKey::VOLUME_UP).unwrap();
        },
        |dev| unsafe {
            assert_eq!(
                medius_device_media_down(dev, MEDIUS_MEDIA_VOLUME_UP),
                MediusStatus::Ok
            );
        },
    );
}

#[test]
fn lock_parity() {
    assert_parity(
        |d| {
            d.lock(medius::LockTarget::X, medius::LockDirection::Both)
                .unwrap();
            d.lock(
                medius::LockTarget::Button(medius::Button::Side1),
                medius::LockDirection::Positive,
            )
            .unwrap();
            d.unlock(medius::LockTarget::X, medius::LockDirection::Both)
                .unwrap();
        },
        |dev| unsafe {
            let x = MediusLockTarget {
                kind: MediusLockTargetKind::X,
                button: MediusButton::Left,
            };
            let side1 = MediusLockTarget {
                kind: MediusLockTargetKind::Button,
                button: MediusButton::Side1,
            };
            assert_eq!(
                medius_device_lock(dev, x, MediusLockDirection::Both),
                MediusStatus::Ok
            );
            assert_eq!(
                medius_device_lock(dev, side1, MediusLockDirection::Positive),
                MediusStatus::Ok
            );
            assert_eq!(
                medius_device_unlock(dev, x, MediusLockDirection::Both),
                MediusStatus::Ok
            );
        },
    );
}

#[test]
fn lock_all_and_led_parity() {
    assert_parity(
        |d| {
            d.lock_all(medius::Blanket::Buttons).unwrap();
            d.led(medius::LedTarget::Both, medius::LedMode::Blink, 128)
                .unwrap();
        },
        |dev| unsafe {
            assert_eq!(
                medius_device_lock_all(dev, MediusBlanket::Buttons),
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
        },
    );
}

// --- queries ---

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
    };
    assert_eq!(
        unsafe { medius_device_query_version(dev, &mut version) },
        MediusStatus::Ok
    );
    assert_eq!(version.fw_major, 9);
    assert_eq!(version.fw_minor, 8);
    assert_eq!(version.fw_patch, 7);
    unsafe {
        medius_device_free(dev);
        medius_mock_free(mock);
    }
}

#[test]
fn query_locks_roundtrips_through_is_locked() {
    let mock = medius_mock_new();
    // X positive + negative locked -> bits 0 and 1.
    unsafe { medius_mock_set_locks(mock, MediusLocks { mask: 0b11 }) };
    let mut dev: *mut MediusDevice = ptr::null_mut();
    assert_eq!(
        unsafe { medius_device_with_mock(mock, &mut dev) },
        MediusStatus::Ok
    );
    let mut locks = MediusLocks { mask: 0 };
    assert_eq!(
        unsafe { medius_device_query_locks(dev, &mut locks) },
        MediusStatus::Ok
    );
    let x = MediusLockTarget {
        kind: MediusLockTargetKind::X,
        button: MediusButton::Left,
    };
    assert!(medius_locks_is_locked(locks, x, MediusLockDirection::Both));
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

// --- streams ---

fn zeroed_event() -> MediusCatchEvent {
    // Safe: every arm of the union is plain-old-data, and we overwrite it before reading.
    unsafe { std::mem::zeroed() }
}

#[test]
fn catch_delivers_a_mouse_event() {
    let mock = medius_mock_new();
    let mut dev: *mut MediusDevice = ptr::null_mut();
    assert_eq!(
        unsafe { medius_device_with_mock(mock, &mut dev) },
        MediusStatus::Ok
    );
    let mut stream: *mut MediusEventStream = ptr::null_mut();
    assert_eq!(
        unsafe { medius_device_catch_events(dev, MEDIUS_CATCH_MASK_ALL, &mut stream) },
        MediusStatus::Ok
    );
    unsafe {
        (*mock).inner.push_event(
            1,
            medius::MouseEvent {
                buttons: 1 << 3,
                dx: 12,
                dy: -34,
                wheel: 1,
            },
        );
    }
    let mut event = zeroed_event();
    assert!(unsafe { medius_event_stream_recv_timeout(stream, 2000, &mut event) });
    assert_eq!(event.kind, MediusCatchEventKind::Mouse);
    let m = unsafe { event.data.mouse };
    assert_eq!(m.dx, 12);
    assert_eq!(m.dy, -34);
    assert_eq!(m.wheel, 1);
    assert!(unsafe { medius_mouse_event_is_pressed(&m, MediusButton::Side1) });
    unsafe {
        medius_event_stream_free(stream);
        medius_device_free(dev);
        medius_mock_free(mock);
    }
}

#[test]
fn catch_delivers_a_keyboard_event() {
    let mock = medius_mock_new();
    let mut dev: *mut MediusDevice = ptr::null_mut();
    assert_eq!(
        unsafe { medius_device_with_mock(mock, &mut dev) },
        MediusStatus::Ok
    );
    let mut stream: *mut MediusEventStream = ptr::null_mut();
    assert_eq!(
        unsafe { medius_device_catch_events(dev, MEDIUS_CATCH_MASK_KEYS, &mut stream) },
        MediusStatus::Ok
    );
    unsafe {
        let kb = medius::KeyboardEvent {
            modifiers: 0,
            keys: vec![medius::Key::ESCAPE],
        };
        (*mock).inner.push_kb_event(1, &kb);
    }
    let mut event = zeroed_event();
    assert!(unsafe { medius_event_stream_recv_timeout(stream, 2000, &mut event) });
    assert_eq!(event.kind, MediusCatchEventKind::Keyboard);
    let kb = unsafe { event.data.keyboard };
    assert!(unsafe { medius_keyboard_event_is_pressed(&kb, MEDIUS_KEY_ESCAPE) });
    unsafe {
        medius_event_stream_free(stream);
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

// --- errors ---

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
fn free_null_handles_is_a_noop() {
    unsafe {
        medius_device_free(ptr::null_mut());
        medius_event_stream_free(ptr::null_mut());
        medius_log_stream_free(ptr::null_mut());
        medius_mock_free(ptr::null_mut());
    }
}
