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
            d.lock(medius::Axis::X, medius::LockDirection::Both)
                .unwrap();
            d.lock(medius::Button::Side1, medius::LockDirection::Positive)
                .unwrap();
            d.unlock(medius::Axis::X, medius::LockDirection::Both)
                .unwrap();
        },
        |dev| unsafe {
            let x = medius_lock_target_axis(MediusLockTargetKind::X);
            let side1 = medius_lock_target_usage(medius_usage_button(MediusButton::Side1));
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
            d.lock_all(medius::Blanket::Buttons, medius::LockDirection::Both)
                .unwrap();
            d.led(medius::LedTarget::Both, medius::LedMode::Blink, 128)
                .unwrap();
        },
        |dev| unsafe {
            assert_eq!(
                medius_device_lock_all(dev, MediusBlanket::Buttons, MediusLockDirection::Both),
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
    set.n = 1;
    set.entries[0] = MediusLockEntry {
        target: x,
        is_blanket: false,
        positive: true,
        negative: true,
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
    assert!(unsafe { medius_locks_is_locked(&locks, x, MediusLockDirection::Both) });
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

#[test]
fn catch_delivers_a_motion_event() {
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
        (*mock).inner.push_motion(1, 7_000, 12, -34, 1);
    }
    let mut event = zeroed_event();
    assert!(unsafe { medius_event_stream_recv_timeout(stream, 2000, &mut event) });
    assert_eq!(event.kind, MediusCatchEventKind::Motion);
    assert_eq!(event.ts_us, 7_000);
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
    let mut stream: *mut MediusEventStream = ptr::null_mut();
    assert_eq!(
        unsafe { medius_device_catch_events(dev, MEDIUS_CATCH_MASK_KEYS, &mut stream) },
        MediusStatus::Ok
    );
    unsafe {
        (*mock)
            .inner
            .push_usages(1, 7_000, &[medius::Usage::from(medius::Key::ESCAPE)]);
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
    let mut stream: *mut MediusEventStream = ptr::null_mut();
    assert_eq!(
        unsafe { medius_device_catch_events(dev, MEDIUS_CATCH_MASK_ALL, &mut stream) },
        MediusStatus::Ok
    );
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
