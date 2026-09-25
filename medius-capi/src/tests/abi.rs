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
                    MediusMoveTiming::Ride as u8,
                    MediusPendingMotion::Keep as u8
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
                    MediusMoveTiming::Now as u8,
                    MediusPendingMotion::Flush as u8
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
            d.inject(medius::Button::RIGHT, medius::Action::Press)
                .unwrap();
        },
        |dev| unsafe {
            assert_eq!(
                medius_device_inject(
                    dev,
                    medius_usage_button(MediusButton::Right as u8),
                    MediusAction::Press as u8
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
            d.press(medius::Button::LEFT).unwrap();
            d.release(medius::Button::LEFT).unwrap();
            d.force_release(medius::Button::LEFT).unwrap();
        },
        |dev| unsafe {
            let left = medius_usage_button(MediusButton::Left as u8);
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
                medius_device_inject(
                    dev,
                    medius_usage_key(MEDIUS_KEY_A),
                    MediusAction::Press as u8
                ),
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
            d.lock(medius::Button::SIDE1, medius::Direction::Positive)
                .unwrap();
            d.unlock(medius::Axis::X, medius::Direction::Both).unwrap();
        },
        |dev| unsafe {
            let x = medius_lock_target_axis(MediusLockTargetKind::X as u8);
            let side1 = medius_lock_target_usage(medius_usage_button(MediusButton::Side1 as u8));
            assert_eq!(
                medius_device_lock(dev, x, MediusDirection::Both as u8),
                MediusStatus::Ok
            );
            assert_eq!(
                medius_device_lock(dev, side1, MediusDirection::Positive as u8),
                MediusStatus::Ok
            );
            assert_eq!(
                medius_device_unlock(dev, x, MediusDirection::Both as u8),
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
                medius_device_lock_all(
                    dev,
                    MediusBlanket::Buttons as u8,
                    MediusDirection::Both as u8
                ),
                MediusStatus::Ok
            );
            assert_eq!(
                medius_device_led(
                    dev,
                    MediusLedTarget::Both as u8,
                    MediusLedMode::Blink as u8,
                    128
                ),
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
                medius_device_reboot(dev, MediusRebootTarget::DeviceRun as u8),
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
                kind: MediusDeviceKind::Mouse as u8,
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
        kind: MediusDeviceKind::Unknown as u8,
        product: [0; MEDIUS_MAX_PRODUCT],
    };
    assert_eq!(
        unsafe { medius_device_device_info(dev, &mut info) },
        MediusStatus::Ok
    );
    assert_eq!(info.vid, 0x1532);
    assert_eq!(info.pid, 0x0072);
    assert_eq!(info.kind, MediusDeviceKind::Mouse as u8);
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
    let x = medius_lock_target_axis(MediusLockTargetKind::X as u8);
    let mut set: MediusLocks = unsafe { std::mem::zeroed() };
    set.n = 2;
    set.entries[0] = MediusLockEntry {
        target: x,
        is_blanket: false,
        direction: MediusDirection::Positive as u8,
        scale: MEDIUS_LOCK_SCALE_BLOCK,
    };
    set.entries[1] = MediusLockEntry {
        target: x,
        is_blanket: false,
        direction: MediusDirection::Negative as u8,
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
    assert!(unsafe { medius_locks_is_locked(&locks, x, MediusDirection::Both as u8) });
    unsafe {
        medius_device_free(dev);
        medius_mock_free(mock);
    }
}

#[test]
fn scale_and_bearing_cross_the_boundary() {
    // Byte-for-byte against the native crate: a status of Ok proves only that the call returned.
    assert_parity(
        |d| {
            d.scale(medius::Axis::X, medius::Direction::Against, 40)
                .unwrap();
            d.scale_all(medius::Blanket::Aim, medius::Direction::With, 130)
                .unwrap();
            d.scale_axis(medius::Axis::Wheel, medius::Direction::Both, 50)
                .unwrap();
            d.set_bearing(Some(Duration::from_millis(35)), medius::BearingMode::Vector)
                .unwrap();
            d.set_bearing(None, medius::BearingMode::PerAxis).unwrap();
        },
        |dev| unsafe {
            let x = medius_lock_target_axis(MediusLockTargetKind::X as u8);
            let wheel = medius_lock_target_axis(MediusLockTargetKind::Wheel as u8);
            assert_eq!(
                medius_device_scale(dev, x, MediusDirection::Against as u8, 40),
                MediusStatus::Ok
            );
            assert_eq!(
                medius_device_scale_all(
                    dev,
                    MediusBlanket::Aim as u8,
                    MediusDirection::With as u8,
                    130
                ),
                MediusStatus::Ok
            );
            assert_eq!(
                medius_device_scale(dev, wheel, MediusDirection::Both as u8, 50),
                MediusStatus::Ok
            );
            assert_eq!(
                medius_device_set_bearing(dev, 35, MediusBearingMode::Vector as u8),
                MediusStatus::Ok
            );
            assert_eq!(
                medius_device_set_bearing(dev, 0, MediusBearingMode::PerAxis as u8),
                MediusStatus::Ok
            );
        },
    );
}

#[test]
fn scale_frames_carry_the_direction_and_the_number() {
    // The payload bytes, derived from ctrl_proto.h: [class][id u16 LE][direction][scale i16 LE].
    let frames = native_frames(|d| {
        d.scale(medius::Axis::X, medius::Direction::Against, 40)
            .unwrap();
        d.scale_all(medius::Blanket::Aim, medius::Direction::With, 130)
            .unwrap();
        d.set_bearing(Some(Duration::from_millis(35)), medius::BearingMode::Vector)
            .unwrap();
    });
    let payloads: Vec<Vec<u8>> = frames.into_iter().map(|f| f.payload).collect();
    assert_eq!(
        payloads,
        vec![
            vec![3, 0, 0, 4, 40, 0],  // AXIS X, AGAINST, 40%
            vec![3, 0, 0, 3, 130, 0], // AXIS X, WITH, 130%
            vec![3, 1, 0, 3, 130, 0], // AXIS Y, WITH, 130%
            vec![4, 35, 0, 1],        // OPTION(BEARING), 35 ms, VECTOR
        ]
    );
}

#[test]
fn the_render_option_reads_back_what_was_set_through_the_boundary() {
    let mock = medius_mock_new();
    let mut dev: *mut MediusDevice = ptr::null_mut();
    assert_eq!(
        unsafe { medius_device_with_mock(mock, &mut dev) },
        MediusStatus::Ok
    );
    let mut st: MediusRenderStatus = unsafe { std::mem::zeroed() };
    // The mock boots holding what a real box boots holding: de-spiked, relayed, and unarmed.
    assert_eq!(
        unsafe { medius_device_query_render(dev, &mut st) },
        MediusStatus::Ok
    );
    assert_eq!(st.mode, MediusRenderMode::Despiked);
    assert_eq!(st.full, 0);
    assert_eq!(st.ready, 0);
    // Both stored fields have to survive the trip, and a mode has to decode as itself.
    assert_eq!(
        unsafe { medius_device_set_render(dev, MediusRenderMode::Unsmoothed as u8, true) },
        MediusStatus::Ok
    );
    assert_eq!(
        unsafe { medius_device_query_render(dev, &mut st) },
        MediusStatus::Ok
    );
    assert_eq!(st.mode, MediusRenderMode::Unsmoothed);
    assert_eq!(st.full, 1);
    // `ready` is the box's own state, not something the host sets, so it comes from the mock.
    unsafe { medius_mock_set_render(mock, MediusRenderMode::Stock as u8, false, true) };
    assert_eq!(
        unsafe { medius_device_query_render(dev, &mut st) },
        MediusStatus::Ok
    );
    assert_eq!(st.mode, MediusRenderMode::Stock);
    assert_eq!(st.full, 0);
    assert_eq!(st.ready, 1);
    unsafe { medius_device_free(dev) };
}

#[test]
fn the_spread_option_reads_back_what_was_set_through_the_boundary() {
    let mock = medius_mock_new();
    let mut dev: *mut MediusDevice = ptr::null_mut();
    assert_eq!(
        unsafe { medius_device_with_mock(mock, &mut dev) },
        MediusStatus::Ok
    );
    let mut st: MediusSpreadStatus = unsafe { std::mem::zeroed() };
    // A fresh box boots at the full percent with no command period learned, so it spreads nothing.
    assert_eq!(
        unsafe { medius_device_query_spread(dev, &mut st) },
        MediusStatus::Ok
    );
    assert_eq!(st.percent, 100);
    assert_eq!(st.span_us, 0);
    // The period is the box's own state, not something the host sets.
    unsafe { medius_mock_set_spread_learned(mock, 8000) };
    assert_eq!(
        unsafe { medius_device_query_spread(dev, &mut st) },
        MediusStatus::Ok
    );
    assert_eq!(st.span_us, 8000);
    // A percent past 100 overlaps rather than being clamped, and both fields survive the trip. Past
    // a byte too: 250 would round-trip through a u8 boundary and prove nothing about the width.
    assert_eq!(
        unsafe { medius_device_set_spread(dev, 1000) },
        MediusStatus::Ok
    );
    assert_eq!(
        unsafe { medius_device_query_spread(dev, &mut st) },
        MediusStatus::Ok
    );
    assert_eq!(st.percent, 1000);
    assert_eq!(st.span_us, 80000);
    unsafe { medius_device_free(dev) };
}

#[test]
fn the_bearing_reads_back_what_was_set_through_the_boundary() {
    let mock = medius_mock_new();
    let mut dev: *mut MediusDevice = ptr::null_mut();
    assert_eq!(
        unsafe { medius_device_with_mock(mock, &mut dev) },
        MediusStatus::Ok
    );
    let mut bearing: MediusBearing = unsafe { std::mem::zeroed() };
    // The mock boots holding what a real box boots holding.
    assert_eq!(
        unsafe { medius_device_query_bearing(dev, &mut bearing) },
        MediusStatus::Ok
    );
    assert_eq!(bearing.mode, MediusBearingMode::PerAxis);
    assert_eq!(bearing.window_ms, MEDIUS_BEARING_WINDOW_DEFAULT_MS);
    // Both fields have to survive the trip, and Vector has to decode as Vector.
    assert_eq!(
        unsafe { medius_device_set_bearing(dev, 35, MediusBearingMode::Vector as u8) },
        MediusStatus::Ok
    );
    assert_eq!(
        unsafe { medius_device_query_bearing(dev, &mut bearing) },
        MediusStatus::Ok
    );
    assert_eq!(bearing.mode, MediusBearingMode::Vector);
    assert_eq!(bearing.window_ms, 35);
    // A window of 0 is the bearing off, with the mode still carried.
    assert_eq!(
        unsafe { medius_device_set_bearing(dev, 0, MediusBearingMode::PerAxis as u8) },
        MediusStatus::Ok
    );
    assert_eq!(
        unsafe { medius_device_query_bearing(dev, &mut bearing) },
        MediusStatus::Ok
    );
    assert_eq!(bearing.window_ms, 0);
    assert_eq!(bearing.mode, MediusBearingMode::PerAxis);
    // And the mock's own setter puts one there without a frame, for a decode test that never writes.
    unsafe { medius_mock_set_bearing(mock, 250, MediusBearingMode::Vector as u8) };
    assert_eq!(
        unsafe { medius_device_query_bearing(dev, &mut bearing) },
        MediusStatus::Ok
    );
    assert_eq!(
        (bearing.window_ms, bearing.mode),
        (250, MediusBearingMode::Vector)
    );
    unsafe {
        medius_device_free(dev);
        medius_mock_free(mock);
    }
}

#[test]
fn a_scale_reaches_the_box_lock_table_through_the_boundary() {
    let mock = medius_mock_new();
    let mut dev: *mut MediusDevice = ptr::null_mut();
    assert_eq!(
        unsafe { medius_device_with_mock(mock, &mut dev) },
        MediusStatus::Ok
    );
    let x = medius_lock_target_axis(MediusLockTargetKind::X as u8);
    let y = medius_lock_target_axis(MediusLockTargetKind::Y as u8);
    assert_eq!(
        unsafe { medius_device_scale(dev, x, MediusDirection::With as u8, 130) },
        MediusStatus::Ok
    );
    assert_eq!(
        unsafe { medius_device_scale(dev, y, MediusDirection::With as u8, 60) },
        MediusStatus::Ok
    );
    let mut locks: MediusLocks = unsafe { std::mem::zeroed() };
    assert_eq!(
        unsafe { medius_device_query_locks(dev, &mut locks) },
        MediusStatus::Ok
    );
    assert_eq!(
        unsafe { medius_locks_scale_of(&locks, x, MediusDirection::With as u8) },
        130
    );
    // In vector mode one relative scale governs both axes, the lower of the two, and the box reports
    // that number on both axes rather than each axis's stored byte.
    assert_eq!(
        unsafe { medius_device_set_bearing(dev, 20, MediusBearingMode::Vector as u8) },
        MediusStatus::Ok
    );
    assert_eq!(
        unsafe { medius_device_query_locks(dev, &mut locks) },
        MediusStatus::Ok
    );
    assert_eq!(
        unsafe { medius_locks_scale_of(&locks, x, MediusDirection::With as u8) },
        60
    );
    assert_eq!(
        unsafe { medius_locks_scale_of(&locks, y, MediusDirection::With as u8) },
        60
    );
    unsafe {
        medius_device_free(dev);
        medius_mock_free(mock);
    }
}

#[test]
fn a_relative_direction_with_no_bearing_to_read_has_its_own_status() {
    let mock = medius_mock_new();
    let mut dev: *mut MediusDevice = ptr::null_mut();
    assert_eq!(
        unsafe { medius_device_with_mock(mock, &mut dev) },
        MediusStatus::Ok
    );
    let left = medius_lock_target_usage(medius_usage_button(MediusButton::Left as u8));
    assert_eq!(
        unsafe { medius_device_scale(dev, left, MediusDirection::Against as u8, 40) },
        MediusStatus::ErrRelativeDirection
    );
    assert_eq!(
        unsafe {
            medius_device_lock_all(dev, MediusBlanket::Keys as u8, MediusDirection::With as u8)
        },
        MediusStatus::ErrRelativeDirection
    );
    unsafe {
        medius_device_free(dev);
        medius_mock_free(mock);
    }
}

#[test]
fn an_unnamed_direction_byte_in_a_caller_built_lock_entry_is_dropped() {
    // `MediusLockEntry.direction` is a `uint8_t` the caller fills in through
    // `medius_mock_set_locks`, and Python has always handed it a raw byte.
    const BAD: u8 = 40;
    let mock = medius_mock_new();
    let x = medius_lock_target_axis(MediusLockTargetKind::X as u8);
    let mut set: MediusLocks = unsafe { std::mem::zeroed() };
    set.n = 2;
    set.entries[0] = MediusLockEntry {
        target: x,
        is_blanket: false,
        direction: BAD,
        scale: MEDIUS_LOCK_SCALE_BLOCK,
    };
    set.entries[1] = MediusLockEntry {
        target: x,
        is_blanket: false,
        direction: MediusDirection::Positive as u8,
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
    // The named entry survives and the unnamed one is gone, count included: a caller reading `n`
    // must not walk an entry that was never decoded.
    assert_eq!(locks.n, 1);
    assert_eq!(locks.entries[0].direction, MediusDirection::Positive as u8);
    assert_eq!(locks.entries[0].scale, MEDIUS_LOCK_SCALE_BLOCK);
    assert!(unsafe { medius_locks_is_locked(&locks, x, MediusDirection::Positive as u8) });
    unsafe {
        medius_device_free(dev);
        medius_mock_free(mock);
    }
}

#[test]
fn an_unnamed_direction_byte_in_a_catch_filter_is_refused() {
    // `MediusCatchFilter.direction` is a `uint8_t` the caller fills in, and Python has always
    // handed it a raw byte.
    const BAD: u8 = 40;
    let mock = medius_mock_new();
    let mut dev: *mut MediusDevice = ptr::null_mut();
    assert_eq!(
        unsafe { medius_device_with_mock(mock, &mut dev) },
        MediusStatus::Ok
    );
    let base = medius_catch_filter_watch(medius_usage_button(MediusButton::Left as u8));
    let bad = medius_catch_filter_with_direction(base, BAD);
    assert_eq!(bad.direction, BAD, "the byte rides the filter unchanged");
    let mut stream: *mut MediusEventStream = ptr::null_mut();
    assert_eq!(
        unsafe { medius_device_catch_events(dev, &bad, 1, &mut stream) },
        MediusStatus::ErrInvalidArg
    );
    assert!(stream.is_null());
    let mut input: *mut MediusInputStream = ptr::null_mut();
    assert_eq!(
        unsafe { medius_device_input_events(dev, &bad, 1, &mut input) },
        MediusStatus::ErrInvalidArg
    );
    assert!(input.is_null());
    // A named one still subscribes, so the refusal is about the byte and not about the call.
    let good = medius_catch_filter_with_direction(base, MediusDirection::Positive as u8);
    assert_eq!(
        unsafe { medius_device_catch_events(dev, &good, 1, &mut stream) },
        MediusStatus::Ok
    );
    assert!(!stream.is_null());
    unsafe {
        medius_event_stream_free(stream);
        medius_device_free(dev);
        medius_mock_free(mock);
    }
}

#[test]
fn a_byte_no_constant_names_is_refused_at_every_entry_point() {
    // These parameters are `uint8_t` in the header, so any byte is a legal call. Each entry point has
    // to name it an argument error rather than act on whichever constant its low bits resemble.
    const BAD: u8 = 40;
    let mock = medius_mock_new();
    let mut dev: *mut MediusDevice = ptr::null_mut();
    assert_eq!(
        unsafe { medius_device_with_mock(mock, &mut dev) },
        MediusStatus::Ok
    );
    let x = medius_lock_target_axis(MediusLockTargetKind::X as u8);
    let both = MediusDirection::Both as u8;
    let aim = MediusBlanket::Aim as u8;
    for (what, status) in [
        ("scale dir", unsafe { medius_device_scale(dev, x, BAD, 50) }),
        ("scale_all dir", unsafe {
            medius_device_scale_all(dev, aim, BAD, 50)
        }),
        ("scale_all what", unsafe {
            medius_device_scale_all(dev, BAD, both, 50)
        }),
        ("lock dir", unsafe { medius_device_lock(dev, x, BAD) }),
        ("unlock dir", unsafe { medius_device_unlock(dev, x, BAD) }),
        ("lock_all dir", unsafe {
            medius_device_lock_all(dev, aim, BAD)
        }),
        ("lock_all what", unsafe {
            medius_device_lock_all(dev, BAD, both)
        }),
        ("unlock_all dir", unsafe {
            medius_device_unlock_all(dev, aim, BAD)
        }),
        ("unlock_all what", unsafe {
            medius_device_unlock_all(dev, BAD, both)
        }),
        // Two named values is one ABI bit while the parameter is an enum, which folded every stray
        // byte onto Vector. As a byte it is refused like any other.
        ("set_bearing mode", unsafe {
            medius_device_set_bearing(dev, 20, BAD)
        }),
    ] {
        assert_eq!(status, MediusStatus::ErrInvalidArg, "{what}");
    }
    // Nothing refused reached the box.
    let mut locks: MediusLocks = unsafe { std::mem::zeroed() };
    assert_eq!(
        unsafe { medius_device_query_locks(dev, &mut locks) },
        MediusStatus::Ok
    );
    assert_eq!(locks.n, 0);
    let mut bearing: MediusBearing = unsafe { std::mem::zeroed() };
    assert_eq!(
        unsafe { medius_device_query_bearing(dev, &mut bearing) },
        MediusStatus::Ok
    );
    assert_eq!(
        (bearing.window_ms, bearing.mode),
        (MEDIUS_BEARING_WINDOW_DEFAULT_MS, MediusBearingMode::PerAxis)
    );
    // The mock setter has no status to return, so it leaves the bearing alone.
    unsafe { medius_mock_set_bearing(mock, 250, BAD) };
    assert_eq!(
        unsafe { medius_device_query_bearing(dev, &mut bearing) },
        MediusStatus::Ok
    );
    assert_eq!(
        (bearing.window_ms, bearing.mode),
        (MEDIUS_BEARING_WINDOW_DEFAULT_MS, MediusBearingMode::PerAxis)
    );
    // The two readers answer with a status of their own: an unnamed direction names no entry, so
    // nothing weighs the target and nothing is locked.
    unsafe {
        assert_eq!(medius_device_lock(dev, x, both), MediusStatus::Ok);
        assert_eq!(medius_device_query_locks(dev, &mut locks), MediusStatus::Ok);
        assert!(medius_locks_is_locked(&locks, x, both));
        assert_eq!(
            medius_locks_scale_of(&locks, x, BAD),
            MEDIUS_LOCK_SCALE_PASS
        );
        assert!(!medius_locks_is_locked(&locks, x, BAD));
    }
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
        restarts: 0,
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
        (*mock).inner.push_motion(1, 7_000, 12, -34, 1, 2);
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
    assert_eq!(m.pan, 2);
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
    let stream = unsafe {
        subscribe(
            dev,
            &[medius_catch_filter_watch_class(MediusClass::Key as u8)],
        )
    };
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
                3,
            )],
        )
    };
    unsafe {
        (*mock).inner.push_traffic(
            1,
            9_000,
            medius::ClockDomain::DeviceChip,
            medius::CatchClass::VendorBulk,
            3,
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
    assert_eq!(t.id, 3);
    assert_eq!(t.direction, MediusDirection::Positive as u8);
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
    pushed.direction = MediusDirection::Positive as u8;
    pushed.flags = 0xFD;
    pushed.true_len = 10;
    pushed.len = 10;
    pushed.bytes[..10]
        .copy_from_slice(&[0x80, 0x06, 0x00, 0x01, 0x00, 0x00, 0x12, 0x00, 0x12, 0x01]);
    unsafe {
        medius_mock_push_traffic(mock, 2, 4_242, MediusClockDomain::DeviceChip as u8, &pushed);
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
fn a_clip_transfer_event_reads_through_the_helpers() {
    let mock = medius_mock_new();
    let mut dev: *mut MediusDevice = ptr::null_mut();
    assert_eq!(
        unsafe { medius_device_with_mock(mock, &mut dev) },
        MediusStatus::Ok
    );
    let stream = unsafe {
        subscribe(
            dev,
            &[medius_catch_filter_traffic_class(
                MEDIUS_CATCH_CLASS_CLIP_TRANSFER,
            )],
        )
    };
    let setup = [0xA1, 0x01, 0x00, 0x03, 0x00, 0x00, 0x02, 0x00];
    let mut pushed: MediusTrafficEvent = unsafe { std::mem::zeroed() };
    pushed.class = MEDIUS_CATCH_CLASS_CLIP_TRANSFER;
    pushed.direction = MediusDirection::Positive as u8;
    pushed.flags = MediusTransferStatus::Ok as u8;
    pushed.true_len = 10;
    pushed.len = 10;
    pushed.bytes[..8].copy_from_slice(&setup);
    pushed.bytes[8..10].copy_from_slice(&[0x04, 0x01]);
    unsafe {
        medius_mock_push_traffic(mock, 1, 7_000, MediusClockDomain::HostChip as u8, &pushed);
    }
    let mut event = zeroed_event();
    assert!(unsafe { medius_event_stream_recv_timeout(stream, 2000, &mut event) });
    assert_eq!(event.kind, MediusCatchEventKind::Traffic);
    let t = unsafe { event.data.traffic };
    assert_eq!(t, pushed);
    assert!(medius_catch_class_is_traffic(t.class));

    let p = unsafe { medius_traffic_event_setup(&t) };
    assert!(!p.is_null());
    assert_eq!(unsafe { std::slice::from_raw_parts(p, 8) }, &setup);
    let mut len = 0usize;
    let d = unsafe { medius_traffic_event_data(&t, &mut len) };
    assert_eq!(unsafe { std::slice::from_raw_parts(d, len) }, &[0x04, 0x01]);

    let mut status = 0xEEu8;
    assert!(unsafe { medius_traffic_event_transfer_status(&t, &mut status) });
    assert_eq!(status, MediusTransferStatus::Ok as u8);
    // The flags byte is a transfer status here, so the control reading does not apply.
    let mut control = MediusControlStatus::Ok;
    assert!(!unsafe { medius_traffic_event_control_status(&t, &mut control) });

    let mut stalled = t;
    stalled.flags = MediusTransferStatus::Stall as u8;
    assert!(unsafe { medius_traffic_event_transfer_status(&stalled, &mut status) });
    assert_eq!(status, MediusTransferStatus::Stall as u8);
    // A byte no constant names is carried through.
    stalled.flags = 0x42;
    assert!(unsafe { medius_traffic_event_transfer_status(&stalled, &mut status) });
    assert_eq!(status, 0x42);
    // The out is optional: a null one is skipped and the answer still comes back.
    assert!(unsafe { medius_traffic_event_transfer_status(&stalled, ptr::null_mut()) });

    // Any other class answers false and leaves the out alone.
    let mut other = t;
    other.class = MEDIUS_CATCH_CLASS_CONTROL;
    status = 0xEE;
    assert!(!unsafe { medius_traffic_event_transfer_status(&other, &mut status) });
    assert_eq!(status, 0xEE);

    // A capture cut inside the setup packet has neither a setup nor a data stage.
    let mut short = t;
    short.len = 4;
    assert!(unsafe { medius_traffic_event_setup(&short) }.is_null());
    let _ = unsafe { medius_traffic_event_data(&short, &mut len) };
    assert_eq!(len, 0);
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
    // A class the box does not define must fail the whole call: a narrower subscription is
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
        (*mock).inner.push_motion(3, 9_000, 4, -5, 0, 0);
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
        kind: MediusClass::Button as u8,
        id: 0,
    }; 4];
    let n =
        unsafe { medius_input_stream_held(stream, MediusClass::Key as u8, held.as_mut_ptr(), 4) };
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
    let capped = medius_catch_filter_with_capture(
        medius_catch_filter_watch_class(MediusClass::Key as u8),
        8,
    );
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
    // entry point that writes an unbounded-length result into a caller buffer is a heap smash.
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
            medius_input_stream_held(ptr::null_mut(), MediusClass::Key as u8, ptr::null_mut(), 0),
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
        kind: MediusClass::Button as u8,
        id: CANARY,
    }; 4];
    let n =
        unsafe { medius_input_stream_held(stream, MediusClass::Key as u8, buf.as_mut_ptr(), 1) };
    assert_eq!(
        n, 2,
        "the true count comes back even when the buffer is short"
    );
    assert_eq!(buf[1].id, CANARY, "nothing was written past cap");
    assert_eq!(buf[2].id, CANARY);
    // cap = 0 with a real pointer writes nothing at all.
    let mut none = [MediusUsage {
        kind: MediusClass::Button as u8,
        id: CANARY,
    }; 2];
    assert_eq!(
        unsafe { medius_input_stream_held(stream, MediusClass::Key as u8, none.as_mut_ptr(), 0) },
        2
    );
    assert_eq!(none[0].id, CANARY);
    // A null out is a size query.
    assert_eq!(
        unsafe { medius_input_stream_held(stream, MediusClass::Key as u8, ptr::null_mut(), 4) },
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
        unsafe { medius_timeline_samples(t, MediusClockDomain::HostChip as u8) },
        2
    );

    // A reboot restarts the clock at zero, which is not a wrap.
    unsafe { medius_timeline_reset(t, MediusClockDomain::HostChip as u8) };
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
    assert!(unsafe { medius_mock_saw(mock, MediusFrameType::Move as u8) });
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
        (*mock).inner.push_motion(1, 7_000, 5, 0, 0, 0);
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
        medius_clip_frame_free(ptr::null_mut());
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
                medius::Button::RIGHT,
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
            clip.unbind(medius::Button::RIGHT, medius::Edge::Press)
                .unwrap();
            clip.clear_triggers().unwrap();
            clip.start().unwrap();
            clip.stop().unwrap();
        },
        |dev| unsafe {
            let mut clip: *mut MediusClip = ptr::null_mut();
            assert_eq!(medius_device_clip(dev, &mut clip), MediusStatus::Ok);
            let scope = [MediusBlanket::Aim as u8, MediusBlanket::Buttons as u8];
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
                        on: medius_usage_button(MediusButton::Right as u8),
                        edge: MediusEdge::Press as u8,
                        action: MediusClipAction::Start as u8,
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
                        edge: MediusEdge::Release as u8,
                        action: MediusClipAction::Stop as u8,
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
                        edge: MediusEdge::Both as u8,
                        action: MediusClipAction::Toggle as u8,
                        consume: 0,
                    }
                ),
                MediusStatus::Ok
            );
            assert_eq!(
                medius_clip_unbind(
                    clip,
                    medius_usage_button(MediusButton::Right as u8),
                    MediusEdge::Press as u8
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

// --- Clip packet triggers: the trigger set's second kind, bound on a traffic class ---

fn c_packet(
    class: u8,
    id: u16,
    direction: u8,
    action: u8,
    match_bytes: &[u8],
    mask: &[u8],
) -> MediusClipPacketTrigger {
    let mut t: MediusClipPacketTrigger = unsafe { std::mem::zeroed() };
    t.class = class;
    t.id = id;
    t.direction = direction;
    t.action = action;
    t.match_len = match_bytes.len() as u16;
    t.mask_len = mask.len() as u16;
    t.match_bytes[..match_bytes.len()].copy_from_slice(match_bytes);
    t.mask[..mask.len()].copy_from_slice(mask);
    t
}

// The protocol's own example: HID_IN interface 2, IN, START, consume and once per run, selector 1,
// match `07 20` under mask `FF 20`.
fn spec_trigger() -> MediusClipPacketTrigger {
    MediusClipPacketTrigger {
        consume: 1,
        once_per_run: 1,
        selector_len: 1,
        ..c_packet(
            MEDIUS_CATCH_CLASS_HID_IN,
            2,
            MediusDirection::Positive as u8,
            MediusClipAction::Start as u8,
            &[0x07, 0x20],
            &[0xFF, 0x20],
        )
    }
}

// Eight triggers that differ in every field, each as the crate holds it and as the C struct carries
// it, written out field by field so neither side is derived from the other.
fn packet_rows() -> Vec<(medius::ClipPacketTriggerEntry, MediusClipPacketTrigger)> {
    use medius::{ClipAction, ClipPacketTrigger, Direction, TrafficClass};
    let (inbound, outbound, both) = (
        (Direction::IN, MediusDirection::Positive as u8),
        (Direction::OUT, MediusDirection::Negative as u8),
        (Direction::Both, MediusDirection::Both as u8),
    );
    let rows = [
        (
            (TrafficClass::HidIn, MEDIUS_CATCH_CLASS_HID_IN),
            0x0102u16,
            inbound,
            (ClipAction::Start, MediusClipAction::Start as u8),
            1usize,
            true,
            None,
            0u16,
        ),
        (
            (TrafficClass::HidOut, MEDIUS_CATCH_CLASS_HID_OUT),
            0x0001,
            outbound,
            (ClipAction::Stop, MediusClipAction::Stop as u8),
            2,
            false,
            Some(1u8),
            1,
        ),
        (
            (
                TrafficClass::VendorInterrupt,
                MEDIUS_CATCH_CLASS_VENDOR_INTERRUPT,
            ),
            0x0083,
            inbound,
            (ClipAction::Pause, MediusClipAction::Pause as u8),
            3,
            true,
            Some(2),
            0xFFFF,
        ),
        (
            (TrafficClass::VendorBulk, MEDIUS_CATCH_CLASS_VENDOR_BULK),
            ClipPacketTrigger::ANY_ID,
            both,
            (ClipAction::Resume, MediusClipAction::Resume as u8),
            4,
            false,
            None,
            0x1234,
        ),
        (
            (TrafficClass::Control, MEDIUS_CATCH_CLASS_CONTROL),
            0,
            both,
            (ClipAction::Restart, MediusClipAction::Restart as u8),
            5,
            false,
            None,
            0x00FF,
        ),
        (
            (TrafficClass::Emit, MEDIUS_CATCH_CLASS_EMIT),
            0x0081,
            inbound,
            (ClipAction::Toggle, MediusClipAction::Toggle as u8),
            0,
            false,
            None,
            0xFF00,
        ),
        (
            (TrafficClass::VendorBulk, MEDIUS_CATCH_CLASS_VENDOR_BULK),
            0x0003,
            outbound,
            (ClipAction::Start, MediusClipAction::Start as u8),
            7,
            true,
            Some(3),
            65534,
        ),
        (
            (TrafficClass::Emit, MEDIUS_CATCH_CLASS_EMIT),
            0x0082,
            inbound,
            (ClipAction::Stop, MediusClipAction::Stop as u8),
            MEDIUS_MAX_PKT_MATCH,
            false,
            None,
            9,
        ),
    ];
    rows.into_iter()
        .enumerate()
        .map(
            |(i, (class, id, direction, action, len, consume, selector, hits))| {
                let masks = [0xFFu8, 0xF0, 0x0F, 0x20, 0x81, 0x7E, 0xC3, 0x01];
                let mask: Vec<u8> = (0..len).map(|j| masks[(i + j) % masks.len()]).collect();
                let match_bytes: Vec<u8> = (0..len)
                    .map(|j| (0x35 + 0x1B * i + 0x47 * j) as u8 & mask[j])
                    .collect();
                let mut native = ClipPacketTrigger::new(class.0, id, direction.0, action.0)
                    .matching(match_bytes.clone(), mask.clone());
                native.consume = consume;
                native.once_per_run = selector.is_some();
                native.selector_len = selector.unwrap_or(0);
                let c = MediusClipPacketTrigger {
                    consume: consume as u8,
                    once_per_run: selector.is_some() as u8,
                    selector_len: selector.unwrap_or(0),
                    hits,
                    ..c_packet(class.1, id, direction.1, action.1, &match_bytes, &mask)
                };
                (
                    medius::ClipPacketTriggerEntry {
                        trigger: native,
                        hits,
                    },
                    c,
                )
            },
        )
        .collect()
}

fn last_error() -> String {
    let mut buf = [0 as c_char; 256];
    let n = unsafe { medius_last_error_message(buf.as_mut_ptr(), buf.len()) };
    assert!(n < buf.len(), "the message outgrew the test's buffer");
    read_cname(&buf)
}

unsafe fn clip_of(dev: *mut MediusDevice) -> *mut MediusClip {
    let mut clip: *mut MediusClip = ptr::null_mut();
    assert_eq!(
        unsafe { medius_device_clip(dev, &mut clip) },
        MediusStatus::Ok
    );
    clip
}

unsafe fn query_config(clip: *mut MediusClip) -> MediusClipSettings {
    let mut out: MediusClipSettings = unsafe { std::mem::zeroed() };
    assert_eq!(
        unsafe { medius_clip_query_config(clip, &mut out) },
        MediusStatus::Ok
    );
    out
}

fn clip_trigger_payloads(frames: &[DecodedFrame]) -> Vec<Vec<u8>> {
    frames
        .iter()
        .filter(|f| f.ty == medius::FrameType::ClipTrigger)
        .map(|f| f.payload.clone())
        .collect()
}

#[test]
fn clip_packet_trigger_parity() {
    let rows = packet_rows();
    assert_parity(
        |d| {
            let clip = d.clip();
            for (entry, _) in &rows {
                clip.bind_packet(&entry.trigger).unwrap();
            }
            clip.unbind_packet(&rows[2].0.trigger).unwrap();
            clip.unbind_packet(&rows[7].0.trigger).unwrap();
            clip.clear_triggers().unwrap();
        },
        |dev| unsafe {
            let clip = clip_of(dev);
            for (_, c) in &rows {
                assert_eq!(medius_clip_bind_packet(clip, c), MediusStatus::Ok);
            }
            assert_eq!(
                medius_clip_unbind_packet(clip, &rows[2].1),
                MediusStatus::Ok
            );
            assert_eq!(
                medius_clip_unbind_packet(clip, &rows[7].1),
                MediusStatus::Ok
            );
            assert_eq!(medius_clip_clear_triggers(clip), MediusStatus::Ok);
            medius_clip_free(clip);
        },
    );
}

#[test]
fn a_packet_trigger_goes_out_as_the_bytes_the_protocol_names() {
    let frames = unsafe {
        capi_frames(|dev| {
            let clip = clip_of(dev);
            assert_eq!(
                medius_clip_bind_packet(clip, &spec_trigger()),
                MediusStatus::Ok
            );
            // A removal sends the key alone, whatever the other fields hold.
            let stale = MediusClipPacketTrigger {
                action: 200,
                consume: 1,
                once_per_run: 1,
                selector_len: 9,
                hits: 77,
                ..spec_trigger()
            };
            assert_eq!(medius_clip_unbind_packet(clip, &stale), MediusStatus::Ok);
            assert_eq!(medius_clip_clear_triggers(clip), MediusStatus::Ok);
            medius_clip_free(clip);
        })
    };
    assert_eq!(
        clip_trigger_payloads(&frames),
        [
            vec![
                0x04, 0x02, 0x00, 0x01, 0x00, 0x07, 0x01, 0x02, 0x07, 0x20, 0xFF, 0x20
            ],
            vec![
                0x04, 0x02, 0x00, 0x01, 0x00, 0x00, 0x00, 0x02, 0x07, 0x20, 0xFF, 0x20
            ],
            vec![0xFF, 0xFF, 0xFF, 0x00, 0x00, 0x00],
        ]
    );
}

#[test]
fn each_packet_trigger_flag_has_its_own_wire_bit() {
    let flags_of = |consume: u8, once_per_run: u8, selector_len: u8| {
        let t = MediusClipPacketTrigger {
            consume,
            once_per_run,
            selector_len,
            ..spec_trigger()
        };
        let frames = unsafe {
            capi_frames(|dev| {
                let clip = clip_of(dev);
                assert_eq!(medius_clip_bind_packet(clip, &t), MediusStatus::Ok);
                medius_clip_free(clip);
            })
        };
        let payload = clip_trigger_payloads(&frames).remove(0);
        (payload[5], payload[6])
    };
    assert_eq!(flags_of(0, 0, 0), (0x01, 0));
    assert_eq!(flags_of(1, 0, 0), (0x03, 0));
    assert_eq!(flags_of(0, 1, 1), (0x05, 1));
    // Any non-zero byte is set, as `MediusClipTrigger::consume` reads.
    assert_eq!(flags_of(0xFF, 0x80, 0), (0x07, 0));
}

#[test]
fn packet_triggers_the_box_holds_read_back_field_for_field() {
    let rows = packet_rows();
    assert_eq!(
        rows[2].1.hits, 0xFFFF,
        "one row reads back a saturated count"
    );
    let input = medius::ClipTrigger::new(
        medius::Button::RIGHT,
        medius::Edge::Press,
        medius::ClipAction::Start,
    );
    for n in [0usize, 1, 8] {
        let mock = medius_mock_new();
        // Rows 0, 2 and 6 consume, which the box holds only under the opt-in.
        unsafe { medius_mock_set_imperfect_status(mock, allowed_status()) };
        unsafe {
            (*mock).inner.set_clip_settings(medius::ClipSettings {
                triggers: vec![input],
                packet_triggers: rows[..n].iter().map(|(e, _)| e.clone()).collect(),
                ..Default::default()
            })
        };
        let mut dev: *mut MediusDevice = ptr::null_mut();
        assert_eq!(
            unsafe { medius_device_with_mock(mock, &mut dev) },
            MediusStatus::Ok
        );
        let clip = unsafe { clip_of(dev) };
        let out = unsafe { query_config(clip) };
        assert_eq!(out.packet_n as usize, n);
        assert_eq!(out.n, 1, "the input triggers keep their own count");
        for (i, (_, want)) in rows[..n].iter().enumerate() {
            assert_eq!(&out.packet_triggers[i], want, "packet trigger {i} of {n}");
        }
        unsafe {
            medius_clip_free(clip);
            medius_device_free(dev);
            medius_mock_free(mock);
        }
    }
    // The protocol's read-back example: HID_IN id 0x0102, IN, TOGGLE, consume and once per run,
    // selector 1, hits saturated.
    let mock = medius_mock_new();
    unsafe { medius_mock_set_imperfect_status(mock, allowed_status()) };
    let spec = medius::ClipPacketTrigger::new(
        medius::TrafficClass::HidIn,
        0x0102,
        medius::Direction::IN,
        medius::ClipAction::Toggle,
    )
    .matching([0x07, 0x20], [0xFF, 0x20])
    .consume()
    .once_per_run(1);
    unsafe {
        (*mock).inner.set_clip_settings(medius::ClipSettings {
            packet_triggers: vec![medius::ClipPacketTriggerEntry {
                trigger: spec.clone(),
                hits: 0xFFFF,
            }],
            ..Default::default()
        })
    };
    let mut dev: *mut MediusDevice = ptr::null_mut();
    assert_eq!(
        unsafe { medius_device_with_mock(mock, &mut dev) },
        MediusStatus::Ok
    );
    let clip = unsafe { clip_of(dev) };
    let out = unsafe { query_config(clip) };
    assert_eq!((out.n, out.packet_n), (0, 1));
    let got = out.packet_triggers[0];
    assert_eq!(
        (got.class, got.id, got.direction, got.action),
        (4, 0x0102, 1, 5)
    );
    assert_eq!(
        (got.consume, got.once_per_run, got.selector_len, got.hits),
        (1, 1, 1, 0xFFFF)
    );
    assert_eq!((got.match_len, got.mask_len), (2, 2));
    assert_eq!(&got.match_bytes[..2], &[0x07, 0x20]);
    assert_eq!(&got.mask[..2], &[0xFF, 0x20]);
    assert_eq!(&got.match_bytes[2..], &[0; MEDIUS_MAX_PKT_MATCH - 2]);
    // A read trigger replays as a bind, its `hits` left behind.
    unsafe { (*mock).inner.clear_recorded() };
    assert_eq!(
        unsafe { medius_clip_bind_packet(clip, &got) },
        MediusStatus::Ok
    );
    let sent = clip_trigger_payloads(&unsafe { (*mock).inner.recorded_frames() });
    let want = native_frames(|d| d.clip().bind_packet(&spec).unwrap());
    assert_eq!(sent, clip_trigger_payloads(&want));
    unsafe {
        medius_clip_free(clip);
        medius_device_free(dev);
        medius_mock_free(mock);
    }
}

// `clip_settings_to_c` on settings the test builds by hand, for a state the box never answers with.
fn settings_to_c(s: &medius::ClipSettings) -> MediusClipSettings {
    let mut c: MediusClipSettings = unsafe { std::mem::zeroed() };
    unsafe { crate::convert::clip_settings_to_c(s, &mut c) };
    c
}

#[test]
fn a_read_back_past_the_arrays_is_clamped_to_them() {
    // The box holds eight of each kind and sixteen match bytes, so only a hand-built state has more.
    let mut trigger = medius::ClipPacketTrigger::new(
        medius::TrafficClass::HidIn,
        2,
        medius::Direction::IN,
        medius::ClipAction::Start,
    );
    trigger.match_bytes = (1..=20).collect();
    trigger.mask = vec![0xFF; 20];
    let input = medius::ClipTrigger::new(
        medius::Button::RIGHT,
        medius::Edge::Press,
        medius::ClipAction::Start,
    );
    let c = settings_to_c(&medius::ClipSettings {
        triggers: vec![input; MEDIUS_CLIP_TRIG_MAX + 1],
        packet_triggers: vec![
            medius::ClipPacketTriggerEntry { trigger, hits: 3 };
            MEDIUS_CLIP_PKT_TRIG_MAX + 1
        ],
        ..Default::default()
    });
    assert_eq!(c.n as usize, MEDIUS_CLIP_TRIG_MAX);
    assert_eq!(c.packet_n as usize, MEDIUS_CLIP_PKT_TRIG_MAX);
    for t in &c.packet_triggers {
        assert_eq!((t.match_len, t.mask_len), (16, 16));
        assert_eq!(t.match_bytes.to_vec(), (1..=16).collect::<Vec<u8>>());
        assert_eq!(t.hits, 3);
    }
}

// The bytes `query` writes into a buffer prefilled with `fill`, read without a typed copy of `T`, which
// would leave its padding undefined again.
unsafe fn out_bytes<T>(fill: u8, query: impl FnOnce(*mut T) -> MediusStatus) -> Vec<u8> {
    let mut out = std::mem::MaybeUninit::<T>::uninit();
    unsafe { ptr::write_bytes(out.as_mut_ptr(), fill, 1) };
    assert_eq!(query(out.as_mut_ptr()), MediusStatus::Ok);
    unsafe { std::slice::from_raw_parts(out.as_ptr() as *const u8, std::mem::size_of::<T>()) }
        .to_vec()
}

#[test]
fn a_clip_read_back_is_defined_in_every_byte() {
    use std::mem::{offset_of, size_of};
    let mock = medius_mock_new();
    unsafe { medius_mock_set_imperfect_status(mock, allowed_status()) };
    let rows = packet_rows();
    unsafe {
        (*mock).inner.set_clip_settings(medius::ClipSettings {
            autolock: vec![medius::Blanket::Aim],
            triggers: vec![medius::ClipTrigger::new(
                medius::Button::RIGHT,
                medius::Edge::Press,
                medius::ClipAction::Start,
            )],
            packet_triggers: vec![rows[2].0.clone()],
            ..Default::default()
        });
        (*mock).inner.set_clip_status(medius::ClipStatus {
            state: medius::ClipState::Playing,
            free: 512,
            held: vec![medius::Button::LEFT.into()],
            ..Default::default()
        });
    }
    let mut dev: *mut MediusDevice = ptr::null_mut();
    assert_eq!(
        unsafe { medius_device_with_mock(mock, &mut dev) },
        MediusStatus::Ok
    );
    let clip = unsafe { clip_of(dev) };

    // Whatever the caller's buffer held, two reads of one state are the same bytes, so a caller can
    // compare or hash a read-back.
    let config = |fill| unsafe {
        out_bytes::<MediusClipSettings>(fill, |out| medius_clip_query_config(clip, out))
    };
    let settings = config(0xAA);
    assert_eq!(settings, config(0x55));
    assert_eq!(settings, config(0xAA));
    let triggers = offset_of!(MediusClipSettings, triggers);
    let packets = offset_of!(MediusClipSettings, packet_triggers);
    let (entry, packet_entry) = (
        size_of::<MediusClipTrigger>(),
        size_of::<MediusClipPacketTrigger>(),
    );
    // The padding: after `ride`, inside and after the first input trigger, after `n`, inside the first
    // packet trigger, and after `packet_n`.
    for pad in [
        5,
        triggers + 1,
        triggers + 7,
        71,
        packets + 1,
        packets + 9,
        457,
    ] {
        assert_eq!(settings[pad], 0, "settings byte {pad}");
    }
    assert_eq!(settings[offset_of!(MediusClipSettings, n)], 1);
    assert_eq!(settings[offset_of!(MediusClipSettings, packet_n)], 1);
    assert_eq!(settings[packets], MEDIUS_CATCH_CLASS_VENDOR_INTERRUPT);
    // A slot past its count reads zero, in both arrays.
    let n_at = offset_of!(MediusClipSettings, n);
    let packet_n_at = offset_of!(MediusClipSettings, packet_n);
    assert!(settings[triggers + entry..n_at].iter().all(|&b| b == 0));
    assert!(
        settings[packets + packet_entry..packet_n_at]
            .iter()
            .all(|&b| b == 0)
    );

    let status = |fill| unsafe {
        out_bytes::<MediusClipStatus>(fill, |out| medius_clip_query_status(clip, out))
    };
    let read = status(0xAA);
    assert_eq!(read, status(0x55));
    assert_eq!(read, status(0xAA));
    let held = offset_of!(MediusClipStatus, held);
    // The padding: after `state`, inside the first held usage, and after the last.
    for pad in [1, 2, 3, held + 1, 1058, 1059] {
        assert_eq!(read[pad], 0, "status byte {pad}");
    }
    assert_eq!(read[0], MediusClipState::Playing as u8);
    assert_eq!(read[offset_of!(MediusClipStatus, held_n)], 1);
    assert!(
        read[held + size_of::<MediusUsage>()..]
            .iter()
            .all(|&b| b == 0)
    );
    unsafe {
        medius_clip_free(clip);
        medius_device_free(dev);
        medius_mock_free(mock);
    }
}

#[test]
fn a_read_back_packet_trigger_carries_each_length_in_its_own_field() {
    // The box holds a match and a mask of one length, so only a hand-built entry tells the two apart.
    let entry = |match_bytes: Vec<u8>, mask: Vec<u8>| {
        let mut trigger = medius::ClipPacketTrigger::new(
            medius::TrafficClass::HidIn,
            2,
            medius::Direction::IN,
            medius::ClipAction::Start,
        );
        trigger.match_bytes = match_bytes;
        trigger.mask = mask;
        medius::ClipPacketTriggerEntry { trigger, hits: 0 }
    };
    let c = settings_to_c(&medius::ClipSettings {
        packet_triggers: vec![
            entry(vec![1, 2, 3], vec![0xFF]),
            entry(vec![7], vec![0xF0; 20]),
        ],
        ..Default::default()
    });
    assert_eq!(c.packet_n, 2);
    let (short_mask, long_mask) = (c.packet_triggers[0], c.packet_triggers[1]);
    assert_eq!((short_mask.match_len, short_mask.mask_len), (3, 1));
    assert_eq!(&short_mask.match_bytes[..4], &[1, 2, 3, 0]);
    assert_eq!(&short_mask.mask[..2], &[0xFF, 0]);
    // A length past the array reads back as the array's.
    assert_eq!((long_mask.match_len, long_mask.mask_len), (1, 16));
    assert_eq!(long_mask.mask, [0xF0; MEDIUS_MAX_PKT_MATCH]);
}

#[test]
fn a_consuming_trigger_scripted_with_the_opt_in_off_is_left_out() {
    let consuming = MediusClipPacketTrigger {
        hits: 7,
        ..spec_trigger()
    };
    let watching = MediusClipPacketTrigger {
        id: 3,
        consume: 0,
        hits: 5,
        ..spec_trigger()
    };
    let mut settings: MediusClipSettings = unsafe { std::mem::zeroed() };
    settings.n = 1;
    settings.triggers[0] = MediusClipTrigger {
        on: medius_usage_button(MediusButton::Right as u8),
        edge: MediusEdge::Press as u8,
        action: MediusClipAction::Start as u8,
        consume: 1,
    };
    settings.packet_n = 2;
    settings.packet_triggers[0] = consuming;
    settings.packet_triggers[1] = watching;

    let (mock, dev, clip) = unsafe { mock_clip() };
    unsafe { medius_mock_set_clip_settings(mock, settings) };
    let out = unsafe { query_config(clip) };
    assert_eq!(
        out.n, 1,
        "an input trigger that consumes is held whatever the opt-in"
    );
    assert_eq!(out.packet_n, 1);
    assert_eq!(out.packet_triggers[0], watching);

    // Scripted under the opt-in, both are held, and turning it off takes the consuming one away.
    unsafe {
        medius_mock_set_imperfect_status(mock, allowed_status());
        medius_mock_set_clip_settings(mock, settings);
    }
    let out = unsafe { query_config(clip) };
    assert_eq!(out.packet_n, 2);
    assert_eq!(out.packet_triggers[..2], [consuming, watching]);
    unsafe {
        medius_mock_set_imperfect_status(
            mock,
            MediusImperfectStatus {
                allowed: 0,
                ..allowed_status()
            },
        )
    };
    let out = unsafe { query_config(clip) };
    assert_eq!(out.packet_n, 1);
    assert_eq!(out.packet_triggers[0], watching);
    unsafe { free_mock_clip(mock, dev, clip) };
}

#[test]
fn scripted_packet_triggers_reach_the_mock_field_for_field() {
    let rows = packet_rows();
    for n in [0usize, 1, 8] {
        let mock = medius_mock_new();
        // Rows 0, 2 and 6 consume, which the box holds only under the opt-in.
        unsafe { medius_mock_set_imperfect_status(mock, allowed_status()) };
        let mut settings: MediusClipSettings = unsafe { std::mem::zeroed() };
        settings.n = 1;
        settings.triggers[0] = MediusClipTrigger {
            on: medius_usage_key(MEDIUS_KEY_A),
            edge: MediusEdge::Release as u8,
            action: MediusClipAction::Stop as u8,
            consume: 1,
        };
        settings.packet_n = n as u8;
        // The slots past `packet_n` hold triggers the count leaves out.
        for (slot, (_, c)) in settings.packet_triggers.iter_mut().zip(&rows) {
            *slot = *c;
        }
        unsafe { medius_mock_set_clip_settings(mock, settings) };
        let native = Device::with_mock(unsafe { (*mock).inner.clone() })
            .clip()
            .query_config()
            .unwrap();
        let want: Vec<_> = rows[..n].iter().map(|(e, _)| e.clone()).collect();
        assert_eq!(native.packet_triggers, want, "{n} scripted");
        assert_eq!(native.triggers.len(), 1);
        unsafe { medius_mock_free(mock) };
    }
    // A count past the array reads the array, and a byte no constant names skips that trigger.
    let mock = medius_mock_new();
    unsafe { medius_mock_set_imperfect_status(mock, allowed_status()) };
    let mut settings: MediusClipSettings = unsafe { std::mem::zeroed() };
    settings.packet_n = 200;
    for (slot, (_, c)) in settings.packet_triggers.iter_mut().zip(&rows) {
        *slot = *c;
    }
    settings.packet_triggers[3].class = 200;
    settings.packet_triggers[4].direction = 200;
    settings.packet_triggers[5].action = 200;
    unsafe { medius_mock_set_clip_settings(mock, settings) };
    let native = Device::with_mock(unsafe { (*mock).inner.clone() })
        .clip()
        .query_config()
        .unwrap();
    let want: Vec<_> = [0, 1, 2, 6, 7].iter().map(|&i| rows[i].0.clone()).collect();
    assert_eq!(native.packet_triggers, want);
    unsafe { medius_mock_free(mock) };
}

#[test]
fn a_packet_trigger_the_crate_refuses_has_its_own_status_and_sends_nothing() {
    let mock = medius_mock_new();
    let mut dev: *mut MediusDevice = ptr::null_mut();
    assert_eq!(
        unsafe { medius_device_with_mock(mock, &mut dev) },
        MediusStatus::Ok
    );
    let clip = unsafe { clip_of(dev) };
    assert_eq!(MediusStatus::ErrClipPacketTrigger as i32, 33);
    let base = MediusClipPacketTrigger {
        consume: 0,
        once_per_run: 0,
        selector_len: 0,
        ..spec_trigger()
    };
    let lengths = |match_len: u16, mask_len: u16| MediusClipPacketTrigger {
        match_len,
        mask_len,
        ..base
    };
    // (what, trigger, the reason the message carries, whether the key is at fault)
    let refused = [
        (
            "bus class",
            MediusClipPacketTrigger {
                class: MEDIUS_CATCH_CLASS_BUS,
                ..base
            },
            "names a surface packets cross",
            true,
        ),
        (
            "clip transfer class",
            MediusClipPacketTrigger {
                class: MEDIUS_CATCH_CLASS_CLIP_TRANSFER,
                ..base
            },
            "names a surface packets cross",
            true,
        ),
        (
            "match longer than mask",
            lengths(2, 1),
            "a match and a mask of one length",
            true,
        ),
        (
            "mask longer than match",
            lengths(1, 2),
            "a match and a mask of one length",
            true,
        ),
        (
            "match past the limit",
            lengths(17, 17),
            "at most 16 match bytes",
            true,
        ),
        (
            "match at the u16 ceiling",
            lengths(u16::MAX, u16::MAX),
            "at most 16 match bytes",
            true,
        ),
        (
            "consume on control",
            MediusClipPacketTrigger {
                class: MEDIUS_CATCH_CLASS_CONTROL,
                consume: 1,
                ..base
            },
            "takes consume on a report or vendor class",
            false,
        ),
        (
            "selector without once_per_run",
            MediusClipPacketTrigger {
                selector_len: 1,
                ..base
            },
            "a selector length only with once_per_run",
            false,
        ),
        (
            "once_per_run on control",
            MediusClipPacketTrigger {
                class: MEDIUS_CATCH_CLASS_CONTROL,
                once_per_run: 1,
                ..base
            },
            "needs one stream",
            false,
        ),
        (
            "once_per_run on every id",
            MediusClipPacketTrigger {
                id: MEDIUS_CATCH_ID_ANY,
                once_per_run: 1,
                ..base
            },
            "needs one stream",
            false,
        ),
        (
            "once_per_run on both directions",
            MediusClipPacketTrigger {
                direction: MediusDirection::Both as u8,
                once_per_run: 1,
                ..base
            },
            "needs one stream",
            false,
        ),
        (
            "selector as long as the match",
            MediusClipPacketTrigger {
                once_per_run: 1,
                selector_len: 2,
                ..base
            },
            "needs match bytes past its selector",
            false,
        ),
        (
            "once_per_run with no match",
            MediusClipPacketTrigger {
                once_per_run: 1,
                ..lengths(0, 0)
            },
            "needs match bytes past its selector",
            false,
        ),
    ];
    for (what, trigger, reason, key_fault) in &refused {
        assert_eq!(
            unsafe { medius_clip_bind_packet(clip, trigger) },
            MediusStatus::ErrClipPacketTrigger,
            "bind: {what}"
        );
        let message = last_error();
        assert!(
            message.starts_with("a clip packet trigger ") && message.contains(reason),
            "{what}: {message}"
        );
        // A removal reads the key alone, so it refuses a bad key and takes anything else.
        let want = if *key_fault {
            MediusStatus::ErrClipPacketTrigger
        } else {
            MediusStatus::Ok
        };
        assert_eq!(
            unsafe { medius_clip_unbind_packet(clip, trigger) },
            want,
            "unbind: {what}"
        );
        if *key_fault {
            assert!(last_error().contains(reason), "unbind: {what}");
        } else {
            assert_eq!(last_error(), "", "unbind: {what}");
        }
    }
    for relative in [MediusDirection::With, MediusDirection::Against] {
        let t = MediusClipPacketTrigger {
            direction: relative as u8,
            ..base
        };
        assert_eq!(
            unsafe { medius_clip_bind_packet(clip, &t) },
            MediusStatus::ErrRelativeDirection
        );
        assert_eq!(
            unsafe { medius_clip_unbind_packet(clip, &t) },
            MediusStatus::ErrRelativeDirection
        );
    }
    for (what, status) in [
        ("bind, null trigger", unsafe {
            medius_clip_bind_packet(clip, ptr::null())
        }),
        ("unbind, null trigger", unsafe {
            medius_clip_unbind_packet(clip, ptr::null())
        }),
    ] {
        assert_eq!(status, MediusStatus::ErrInvalidArg, "{what}");
    }
    // The removals of the seven sound keys are all that went out.
    let sent = clip_trigger_payloads(&unsafe { (*mock).inner.recorded_frames() });
    assert_eq!(sent.len(), refused.iter().filter(|r| !r.3).count());
    assert!(sent.iter().all(|p| p[4..7] == [0, 0, 0]), "{sent:?}");
    assert_eq!(unsafe { query_config(clip) }.packet_n, 0);

    // The longest match the box compares is bound and read back whole.
    let full = MediusClipPacketTrigger {
        match_bytes: [0x11; MEDIUS_MAX_PKT_MATCH],
        mask: [0xFF; MEDIUS_MAX_PKT_MATCH],
        ..lengths(16, 16)
    };
    assert_eq!(
        unsafe { medius_clip_bind_packet(clip, &full) },
        MediusStatus::Ok
    );
    assert_eq!(last_error(), "");
    let out = unsafe { query_config(clip) };
    assert_eq!(out.packet_n, 1);
    assert_eq!(out.packet_triggers[0], full);
    unsafe {
        medius_clip_free(clip);
        medius_device_free(dev);
        medius_mock_free(mock);
    }
}

// A device over a fresh mock and its clip handle; `free_mock_clip` lets all three go.
unsafe fn mock_clip() -> (*mut MediusMockBox, *mut MediusDevice, *mut MediusClip) {
    let mock = medius_mock_new();
    let mut dev: *mut MediusDevice = ptr::null_mut();
    assert_eq!(
        unsafe { medius_device_with_mock(mock, &mut dev) },
        MediusStatus::Ok
    );
    (mock, dev, unsafe { clip_of(dev) })
}

unsafe fn free_mock_clip(mock: *mut MediusMockBox, dev: *mut MediusDevice, clip: *mut MediusClip) {
    unsafe {
        medius_clip_free(clip);
        medius_device_free(dev);
        medius_mock_free(mock);
    }
}

// `status` is the packet trigger refusal and the last error carries `reason`.
fn assert_refused_for(status: MediusStatus, reason: &str, what: &str) {
    assert_eq!(status, MediusStatus::ErrClipPacketTrigger, "{what}");
    let message = last_error();
    assert!(
        message.starts_with("a clip packet trigger ") && message.contains(reason),
        "{what}: {message}"
    );
}

#[test]
fn a_packet_trigger_on_a_direction_its_class_never_carries_is_refused() {
    let (mock, dev, clip) = unsafe { mock_clip() };
    let (inbound, outbound, both) = (
        MediusDirection::Positive as u8,
        MediusDirection::Negative as u8,
        MediusDirection::Both as u8,
    );
    let on = |class: u8, direction: u8| {
        c_packet(
            class,
            1,
            direction,
            MediusClipAction::Start as u8,
            &[0x07],
            &[0xFF],
        )
    };
    let reason =
        "names a direction its class never carries: HidIn and Emit flow IN, HidOut flows OUT";
    for (what, class, direction) in [
        ("HID_IN, OUT", MEDIUS_CATCH_CLASS_HID_IN, outbound),
        ("EMIT, OUT", MEDIUS_CATCH_CLASS_EMIT, outbound),
        ("HID_OUT, IN", MEDIUS_CATCH_CLASS_HID_OUT, inbound),
    ] {
        let t = on(class, direction);
        assert_refused_for(unsafe { medius_clip_bind_packet(clip, &t) }, reason, what);
        assert_refused_for(unsafe { medius_clip_unbind_packet(clip, &t) }, reason, what);
    }
    assert_eq!(
        clip_trigger_payloads(&unsafe { (*mock).inner.recorded_frames() }),
        Vec::<Vec<u8>>::new()
    );

    // Every flow a class carries is bound, and every class takes both.
    let carried = [
        (MEDIUS_CATCH_CLASS_HID_IN, inbound),
        (MEDIUS_CATCH_CLASS_HID_IN, both),
        (MEDIUS_CATCH_CLASS_HID_OUT, outbound),
        (MEDIUS_CATCH_CLASS_HID_OUT, both),
        (MEDIUS_CATCH_CLASS_VENDOR_INTERRUPT, inbound),
        (MEDIUS_CATCH_CLASS_VENDOR_INTERRUPT, outbound),
        (MEDIUS_CATCH_CLASS_VENDOR_BULK, both),
        (MEDIUS_CATCH_CLASS_EMIT, inbound),
    ];
    for (class, direction) in carried {
        assert_eq!(
            unsafe { medius_clip_bind_packet(clip, &on(class, direction)) },
            MediusStatus::Ok,
            "class {class} direction {direction}"
        );
    }
    let out = unsafe { query_config(clip) };
    assert_eq!(out.packet_n as usize, carried.len());
    for (held, (class, direction)) in out.packet_triggers.iter().zip(carried) {
        assert_eq!((held.class, held.direction), (class, direction));
    }
    unsafe { free_mock_clip(mock, dev, clip) };
}

#[test]
fn a_packet_trigger_with_a_match_bit_outside_its_mask_is_refused() {
    let (mock, dev, clip) = unsafe { mock_clip() };
    let reason = "has a match bit outside its mask, which no packet can equal";
    let hid_in = |match_bytes: &[u8], mask: &[u8]| {
        c_packet(
            MEDIUS_CATCH_CLASS_HID_IN,
            2,
            MediusDirection::Positive as u8,
            MediusClipAction::Start as u8,
            match_bytes,
            mask,
        )
    };
    let mut last_byte = hid_in(&[0; MEDIUS_MAX_PKT_MATCH], &[0xFF; MEDIUS_MAX_PKT_MATCH]);
    last_byte.match_bytes[MEDIUS_MAX_PKT_MATCH - 1] = 0x80;
    last_byte.mask[MEDIUS_MAX_PKT_MATCH - 1] = 0x7F;
    for (what, t) in [
        ("one stray bit", hid_in(&[0x07, 0x21], &[0xFF, 0x20])),
        ("a match under an empty mask", hid_in(&[0x01], &[0x00])),
        ("the last byte the box compares", last_byte),
    ] {
        assert_refused_for(unsafe { medius_clip_bind_packet(clip, &t) }, reason, what);
        assert_refused_for(unsafe { medius_clip_unbind_packet(clip, &t) }, reason, what);
    }
    assert_eq!(
        clip_trigger_payloads(&unsafe { (*mock).inner.recorded_frames() }),
        Vec::<Vec<u8>>::new()
    );

    // The key is `match_bytes[0..match_len]`: what the arrays hold past it stays on this side.
    let mut past_the_length = hid_in(&[0x07, 0x20], &[0xFF, 0x20]);
    past_the_length.match_bytes[2] = 0xFF;
    assert_eq!(
        unsafe { medius_clip_bind_packet(clip, &past_the_length) },
        MediusStatus::Ok
    );
    let sent = clip_trigger_payloads(&unsafe { (*mock).inner.recorded_frames() });
    assert_eq!(
        sent,
        [vec![
            0x04, 0x02, 0x00, 0x01, 0x00, 0x01, 0x00, 0x02, 0x07, 0x20, 0xFF, 0x20
        ]]
    );
    let out = unsafe { query_config(clip) };
    assert_eq!(out.packet_n, 1);
    assert_eq!(out.packet_triggers[0], hid_in(&[0x07, 0x20], &[0xFF, 0x20]));
    unsafe { free_mock_clip(mock, dev, clip) };
}

#[test]
fn a_once_per_run_trigger_with_no_masked_bit_past_its_selector_is_refused() {
    let (mock, dev, clip) = unsafe { mock_clip() };
    let reason = "needs a masked bit past its selector";
    let run = |selector_len: u8, match_bytes: &[u8], mask: &[u8]| MediusClipPacketTrigger {
        once_per_run: 1,
        selector_len,
        ..c_packet(
            MEDIUS_CATCH_CLASS_HID_IN,
            2,
            MediusDirection::Positive as u8,
            MediusClipAction::Start as u8,
            match_bytes,
            mask,
        )
    };
    let refused = [
        (
            "a condition byte with an empty mask",
            run(1, &[0x07, 0x00], &[0xFF, 0x00]),
        ),
        (
            "no selector and an empty mask",
            run(0, &[0x00, 0x00], &[0x00, 0x00]),
        ),
        (
            "every condition byte empty",
            run(2, &[0x07, 0x01, 0x00, 0x00], &[0xFF, 0xFF, 0x00, 0x00]),
        ),
    ];
    for (what, t) in &refused {
        assert_refused_for(unsafe { medius_clip_bind_packet(clip, t) }, reason, what);
    }
    assert_eq!(
        clip_trigger_payloads(&unsafe { (*mock).inner.recorded_frames() }),
        Vec::<Vec<u8>>::new()
    );
    // The key is sound, so a removal goes out, and so does the same trigger bound on each packet.
    for (what, t) in &refused {
        assert_eq!(
            unsafe { medius_clip_unbind_packet(clip, t) },
            MediusStatus::Ok,
            "unbind: {what}"
        );
        let each_packet = MediusClipPacketTrigger {
            once_per_run: 0,
            selector_len: 0,
            ..*t
        };
        assert_eq!(
            unsafe { medius_clip_bind_packet(clip, &each_packet) },
            MediusStatus::Ok,
            "bind on each packet: {what}"
        );
    }
    // One masked bit past the selector is a condition.
    let one_bit = run(1, &[0x07, 0x00, 0x00], &[0xFF, 0x00, 0x01]);
    assert_eq!(
        unsafe { medius_clip_bind_packet(clip, &one_bit) },
        MediusStatus::Ok
    );
    let out = unsafe { query_config(clip) };
    assert_eq!(out.packet_n, 4);
    assert_eq!(out.packet_triggers[3], one_bit);
    unsafe { free_mock_clip(mock, dev, clip) };
}

#[test]
fn a_packet_that_cannot_exist_fires_nothing_in_the_mock() {
    let (mock, dev, clip) = unsafe { mock_clip() };
    let surfaces = [
        MEDIUS_CATCH_CLASS_HID_IN,
        MEDIUS_CATCH_CLASS_HID_OUT,
        MEDIUS_CATCH_CLASS_VENDOR_INTERRUPT,
        MEDIUS_CATCH_CLASS_VENDOR_BULK,
        MEDIUS_CATCH_CLASS_CONTROL,
        MEDIUS_CATCH_CLASS_EMIT,
    ];
    // One trigger on every packet of each surface, so any packet the mock takes fires.
    for class in surfaces {
        let wide = c_packet(
            class,
            MEDIUS_CATCH_ID_ANY,
            MediusDirection::Both as u8,
            MediusClipAction::Toggle as u8,
            &[],
            &[],
        );
        assert_eq!(
            unsafe { medius_clip_bind_packet(clip, &wide) },
            MediusStatus::Ok
        );
    }
    let run = |class: u8, direction: u8| {
        let (mut action, mut consumed) = (0xEEu8, true);
        let fired = unsafe {
            medius_mock_clip_packet(
                mock,
                class,
                1,
                direction,
                [0x07u8].as_ptr(),
                1,
                &mut action,
                &mut consumed,
            )
        };
        (fired, action, consumed)
    };
    let (inbound, outbound) = (
        MediusDirection::Positive as u8,
        MediusDirection::Negative as u8,
    );
    let mut no_packet = vec![
        (MEDIUS_CATCH_CLASS_HID_IN, outbound),
        (MEDIUS_CATCH_CLASS_EMIT, outbound),
        (MEDIUS_CATCH_CLASS_HID_OUT, inbound),
        (MEDIUS_CATCH_CLASS_BUS, inbound),
        (MEDIUS_CATCH_CLASS_CLIP_TRANSFER, inbound),
    ];
    for class in surfaces {
        for direction in [
            MediusDirection::Both,
            MediusDirection::With,
            MediusDirection::Against,
        ] {
            no_packet.push((class, direction as u8));
        }
    }
    for (class, direction) in no_packet {
        assert_eq!(
            run(class, direction),
            (false, 0xEE, false),
            "class {class} direction {direction}"
        );
    }
    let out = unsafe { query_config(clip) };
    assert_eq!(out.packet_n as usize, surfaces.len());
    assert!(out.packet_triggers[..6].iter().all(|t| t.hits == 0));

    // Each flow a surface carries is a packet, and its trigger counts it.
    let toggle = MediusClipAction::Toggle as u8;
    for (class, direction) in [
        (MEDIUS_CATCH_CLASS_HID_IN, inbound),
        (MEDIUS_CATCH_CLASS_HID_OUT, outbound),
        (MEDIUS_CATCH_CLASS_VENDOR_INTERRUPT, inbound),
        (MEDIUS_CATCH_CLASS_VENDOR_INTERRUPT, outbound),
        (MEDIUS_CATCH_CLASS_VENDOR_BULK, inbound),
        (MEDIUS_CATCH_CLASS_VENDOR_BULK, outbound),
        (MEDIUS_CATCH_CLASS_CONTROL, inbound),
        (MEDIUS_CATCH_CLASS_CONTROL, outbound),
        (MEDIUS_CATCH_CLASS_EMIT, inbound),
    ] {
        assert_eq!(
            run(class, direction),
            (true, toggle, false),
            "class {class} direction {direction}"
        );
    }
    let hits: Vec<u16> = unsafe { query_config(clip) }.packet_triggers[..6]
        .iter()
        .map(|t| t.hits)
        .collect();
    assert_eq!(hits, [1, 1, 2, 2, 2, 1]);
    unsafe { free_mock_clip(mock, dev, clip) };
}

#[test]
fn clear_triggers_removes_both_kinds() {
    let rows = packet_rows();
    let mock = medius_mock_new();
    let mut settings: MediusClipSettings = unsafe { std::mem::zeroed() };
    settings.n = 1;
    settings.triggers[0] = MediusClipTrigger {
        on: medius_usage_button(MediusButton::Right as u8),
        edge: MediusEdge::Press as u8,
        action: MediusClipAction::Start as u8,
        consume: 0,
    };
    settings.packet_n = 1;
    settings.packet_triggers[0] = rows[1].1;
    unsafe { medius_mock_set_clip_settings(mock, settings) };
    let mut dev: *mut MediusDevice = ptr::null_mut();
    assert_eq!(
        unsafe { medius_device_with_mock(mock, &mut dev) },
        MediusStatus::Ok
    );
    let clip = unsafe { clip_of(dev) };
    assert_eq!(
        unsafe { medius_clip_bind_packet(clip, &rows[5].1) },
        MediusStatus::Ok
    );
    let held = unsafe { query_config(clip) };
    assert_eq!((held.n, held.packet_n), (1, 2));
    // A bind starts its count at zero, whatever `hits` the caller's struct held.
    assert_eq!(held.packet_triggers[1].hits, 0);
    assert_eq!(
        held.packet_triggers[1],
        MediusClipPacketTrigger {
            hits: 0,
            ..rows[5].1
        }
    );

    assert_eq!(
        unsafe { medius_clip_unbind_packet(clip, &rows[1].1) },
        MediusStatus::Ok
    );
    let held = unsafe { query_config(clip) };
    assert_eq!((held.n, held.packet_n), (1, 1));
    assert_eq!(held.packet_triggers[0].class, MEDIUS_CATCH_CLASS_EMIT);

    assert_eq!(
        unsafe { medius_clip_clear_triggers(clip) },
        MediusStatus::Ok
    );
    let cleared = unsafe { query_config(clip) };
    assert_eq!((cleared.n, cleared.packet_n), (0, 0));
    unsafe {
        medius_clip_free(clip);
        medius_device_free(dev);
        medius_mock_free(mock);
    }
}

#[test]
fn the_mock_runs_a_packet_through_its_triggers() {
    let mock = medius_mock_new();
    unsafe { medius_mock_set_imperfect_status(mock, allowed_status()) };
    let mut dev: *mut MediusDevice = ptr::null_mut();
    assert_eq!(
        unsafe { medius_device_with_mock(mock, &mut dev) },
        MediusStatus::Ok
    );
    let clip = unsafe { clip_of(dev) };
    let hid_in = MEDIUS_CATCH_CLASS_HID_IN;
    let inbound = MediusDirection::Positive as u8;
    let held = spec_trigger();
    let let_go = MediusClipPacketTrigger {
        once_per_run: 1,
        selector_len: 1,
        ..c_packet(
            hid_in,
            2,
            inbound,
            MediusClipAction::Stop as u8,
            &[0x07, 0x00],
            &[0xFF, 0x20],
        )
    };
    let wide = c_packet(
        hid_in,
        MEDIUS_CATCH_ID_ANY,
        MediusDirection::Both as u8,
        MediusClipAction::Toggle as u8,
        &[],
        &[],
    );
    for t in [&wide, &held, &let_go] {
        assert_eq!(
            unsafe { medius_clip_bind_packet(clip, t) },
            MediusStatus::Ok
        );
    }
    // (fired, action, consumed), with the outs primed so an untouched one shows.
    let run = |class: u8, id: u16, direction: u8, head: &[u8]| {
        let (mut action, mut consumed) = (0xEEu8, true);
        let fired = unsafe {
            medius_mock_clip_packet(
                mock,
                class,
                id,
                direction,
                head.as_ptr(),
                head.len(),
                &mut action,
                &mut consumed,
            )
        };
        (fired, action, consumed)
    };
    let start = MediusClipAction::Start as u8;
    let stop = MediusClipAction::Stop as u8;
    let toggle = MediusClipAction::Toggle as u8;
    // The held report starts the clip once, and every packet of the hold is consumed.
    assert_eq!(
        run(hid_in, 2, inbound, &[0x07, 0x20, 0x55]),
        (true, start, true)
    );
    assert_eq!(
        run(hid_in, 2, inbound, &[0x07, 0x20, 0x56]),
        (false, 0xEE, true)
    );
    // Another report ID leaves the run as it was.
    assert_eq!(
        run(hid_in, 2, inbound, &[0x09, 0x20]),
        (true, toggle, false)
    );
    assert_eq!(run(hid_in, 2, inbound, &[0x07, 0x20]), (false, 0xEE, true));
    // The release ends it, and the next hold starts it again.
    assert_eq!(run(hid_in, 2, inbound, &[0x07, 0x00]), (true, stop, false));
    assert_eq!(run(hid_in, 2, inbound, &[0x07, 0x20]), (true, start, true));
    // The id wildcard takes what the exact triggers leave.
    assert_eq!(
        run(hid_in, 5, inbound, &[0x07, 0x20]),
        (true, toggle, false)
    );
    // Nothing matches on another class, and the outs say so.
    assert_eq!(
        run(
            MEDIUS_CATCH_CLASS_HID_OUT,
            2,
            MediusDirection::Negative as u8,
            &[0x07, 0x20]
        ),
        (false, 0xEE, false)
    );

    let out = unsafe { query_config(clip) };
    assert_eq!(out.packet_n, 3);
    let hits: Vec<u16> = out.packet_triggers[..3].iter().map(|t| t.hits).collect();
    assert_eq!(hits, [2, 4, 1], "wide, held, let_go");

    // A byte no constant names matches nothing, and neither does a head the caller left null.
    assert_eq!(run(200, 2, inbound, &[0x07, 0x20]), (false, 0xEE, false));
    assert_eq!(run(hid_in, 2, 200, &[0x07, 0x20]), (false, 0xEE, false));
    assert_eq!(
        run(MEDIUS_CATCH_CLASS_KEY, 2, inbound, &[0x07, 0x20]),
        (false, 0xEE, false)
    );
    unsafe {
        let mut consumed = true;
        assert!(!medius_mock_clip_packet(
            mock,
            hid_in,
            2,
            inbound,
            ptr::null(),
            2,
            ptr::null_mut(),
            &mut consumed
        ));
        assert!(!consumed);
        assert!(!medius_mock_clip_packet(
            ptr::null_mut(),
            hid_in,
            2,
            inbound,
            [0x07u8, 0x20].as_ptr(),
            2,
            ptr::null_mut(),
            ptr::null_mut()
        ));
        // Each out is optional: the packet still runs and the answer still comes back.
        assert!(medius_mock_clip_packet(
            mock,
            hid_in,
            5,
            inbound,
            [0x01u8].as_ptr(),
            1,
            ptr::null_mut(),
            ptr::null_mut()
        ));
        // An empty head matches a trigger with no match bytes.
        assert!(medius_mock_clip_packet(
            mock,
            hid_in,
            5,
            inbound,
            ptr::null(),
            0,
            ptr::null_mut(),
            ptr::null_mut()
        ));
    }
    assert_eq!(unsafe { query_config(clip) }.packet_triggers[0].hits, 4);
    unsafe {
        medius_clip_free(clip);
        medius_device_free(dev);
        medius_mock_free(mock);
    }
}

#[test]
fn a_consuming_packet_trigger_needs_the_imperfect_opt_in_and_goes_when_it_is_turned_off() {
    let mock = medius_mock_new();
    let mut dev: *mut MediusDevice = ptr::null_mut();
    assert_eq!(
        unsafe { medius_device_with_mock(mock, &mut dev) },
        MediusStatus::Ok
    );
    let clip = unsafe { clip_of(dev) };
    let consuming = spec_trigger();
    let watching = MediusClipPacketTrigger {
        consume: 0,
        id: 3,
        ..spec_trigger()
    };
    // The bind goes out either way; the box holds the consuming one only under the opt-in.
    for t in [&consuming, &watching] {
        assert_eq!(
            unsafe { medius_clip_bind_packet(clip, t) },
            MediusStatus::Ok
        );
    }
    let out = unsafe { query_config(clip) };
    assert_eq!(out.packet_n, 1);
    assert_eq!(out.packet_triggers[0], watching);

    assert_eq!(
        unsafe { medius_device_allow_imperfect_clones(dev, true) },
        MediusStatus::Ok
    );
    assert_eq!(
        unsafe { medius_clip_bind_packet(clip, &consuming) },
        MediusStatus::Ok
    );
    let out = unsafe { query_config(clip) };
    assert_eq!(out.packet_n, 2);
    assert_eq!(out.packet_triggers[1], consuming);

    assert_eq!(
        unsafe { medius_device_allow_imperfect_clones(dev, false) },
        MediusStatus::Ok
    );
    let out = unsafe { query_config(clip) };
    assert_eq!(out.packet_n, 1);
    assert_eq!(out.packet_triggers[0], watching);
    unsafe {
        medius_clip_free(clip);
        medius_device_free(dev);
        medius_mock_free(mock);
    }
}

#[test]
fn the_clip_packet_trigger_layout_is_the_one_the_header_declares() {
    use std::mem::{offset_of, size_of};
    assert_eq!(MEDIUS_CLIP_PKT_TRIG_MAX, 8);
    assert_eq!(MEDIUS_CLIP_PKT_MATCH_POOL, 112);
    assert_eq!(MEDIUS_MAX_PKT_MATCH, 16);
    assert_eq!(offset_of!(MediusClipPacketTrigger, class), 0);
    assert_eq!(offset_of!(MediusClipPacketTrigger, id), 2);
    assert_eq!(offset_of!(MediusClipPacketTrigger, direction), 4);
    assert_eq!(offset_of!(MediusClipPacketTrigger, action), 5);
    assert_eq!(offset_of!(MediusClipPacketTrigger, consume), 6);
    assert_eq!(offset_of!(MediusClipPacketTrigger, once_per_run), 7);
    assert_eq!(offset_of!(MediusClipPacketTrigger, selector_len), 8);
    assert_eq!(offset_of!(MediusClipPacketTrigger, match_len), 10);
    assert_eq!(offset_of!(MediusClipPacketTrigger, mask_len), 12);
    assert_eq!(offset_of!(MediusClipPacketTrigger, match_bytes), 14);
    assert_eq!(offset_of!(MediusClipPacketTrigger, mask), 30);
    assert_eq!(offset_of!(MediusClipPacketTrigger, hits), 46);
    assert_eq!(size_of::<MediusClipPacketTrigger>(), 48);

    assert_eq!(size_of::<MediusClipTrigger>(), 8);
    assert_eq!(offset_of!(MediusClipSettings, autolock_bits), 0);
    assert_eq!(offset_of!(MediusClipSettings, loop_), 1);
    assert_eq!(offset_of!(MediusClipSettings, retain), 2);
    assert_eq!(offset_of!(MediusClipSettings, finalized), 3);
    assert_eq!(offset_of!(MediusClipSettings, ride), 4);
    assert_eq!(offset_of!(MediusClipSettings, triggers), 6);
    assert_eq!(offset_of!(MediusClipSettings, n), 70);
    assert_eq!(offset_of!(MediusClipSettings, packet_triggers), 72);
    assert_eq!(offset_of!(MediusClipSettings, packet_n), 456);
    assert_eq!(size_of::<MediusClipSettings>(), 458);
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
            b.press(medius::Button::LEFT);
            b.gap(4);
            b.release(medius::Button::LEFT);
            d.clip().append(&b).unwrap();
        },
        |dev| unsafe {
            let builder = medius_clip_builder_new();
            for _ in 0..150 {
                assert_eq!(medius_clip_builder_move(builder, 3, -2), MediusStatus::Ok);
            }
            assert_eq!(
                medius_clip_builder_press(builder, medius_usage_button(MediusButton::Left as u8)),
                MediusStatus::Ok
            );
            assert_eq!(medius_clip_builder_gap(builder, 4), MediusStatus::Ok);
            assert_eq!(
                medius_clip_builder_release(builder, medius_usage_button(MediusButton::Left as u8)),
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
                medius::ClipFrame::new()
                    .move_by(1, 2)
                    .wheel(-1)
                    .press(medius::Button::LEFT)
                    .press(medius::Key::new(0x04)),
            );
            d.clip().append(&b).unwrap();
        },
        |dev| unsafe {
            let builder = medius_clip_builder_new();
            let frame = medius_clip_frame_new();
            assert_eq!(medius_clip_frame_move(frame, 1, 2), MediusStatus::Ok);
            assert_eq!(medius_clip_frame_wheel(frame, -1), MediusStatus::Ok);
            assert_eq!(
                medius_clip_frame_press(frame, medius_usage_button(MediusButton::Left as u8)),
                MediusStatus::Ok
            );
            assert_eq!(
                medius_clip_frame_edge(frame, medius_usage_key(0x04), MediusAction::Press as u8),
                MediusStatus::Ok
            );
            assert_eq!(medius_clip_builder_frame(builder, frame), MediusStatus::Ok);
            let mut clip: *mut MediusClip = ptr::null_mut();
            assert_eq!(medius_device_clip(dev, &mut clip), MediusStatus::Ok);
            assert_eq!(medius_clip_append(clip, builder), MediusStatus::Ok);
            medius_clip_free(clip);
            medius_clip_frame_free(frame);
            medius_clip_builder_free(builder);
        },
    );
}

#[test]
fn a_clip_frame_carries_every_field_like_the_crate() {
    let setup_out = medius::Setup::new(0x21, 0x09, 0x0300, 0, 2);
    let setup_in = medius::Setup::new(0xA1, 0x01, 0x0300, 0, 8);
    assert_parity(
        |d| {
            let mut b = medius::ClipBuilder::new();
            let frame = medius::ClipFrame::new()
                .move_by(4, -2)
                .wheel(1)
                .pan(-3)
                .press(medius::Button::LEFT)
                .release(medius::Key::new(0x04))
                .force_release(medius::MediaKey::new(0xCD))
                .raw(2, medius::Direction::OUT, [0x10, 0xFF, 0x05])
                .raw(1, medius::Direction::IN, [])
                .transfer(0, setup_out, [0x04, 0x01])
                .transfer(0, setup_in, []);
            b.frame(frame.clone());
            b.frame(frame);
            b.pan(7);
            b.raw(2, medius::Direction::OUT, [0xAA]);
            b.transfer(0, setup_out, [0x01, 0x02]);
            b.frame(medius::ClipFrame::new().pan(9));
            d.clip().append(&b).unwrap();
        },
        |dev| unsafe {
            let c_out = MediusSetup {
                request_type: 0x21,
                request: 0x09,
                value: 0x0300,
                index: 0,
                length: 2,
            };
            let c_in = MediusSetup {
                request_type: 0xA1,
                request: 0x01,
                value: 0x0300,
                index: 0,
                length: 8,
            };
            let out = MediusDirection::Negative as u8;
            let builder = medius_clip_builder_new();
            let frame = medius_clip_frame_new();
            assert_eq!(medius_clip_frame_move(frame, 4, -2), MediusStatus::Ok);
            assert_eq!(medius_clip_frame_wheel(frame, 1), MediusStatus::Ok);
            assert_eq!(medius_clip_frame_pan(frame, -3), MediusStatus::Ok);
            assert_eq!(
                medius_clip_frame_press(frame, medius_usage_button(MediusButton::Left as u8)),
                MediusStatus::Ok
            );
            assert_eq!(
                medius_clip_frame_release(frame, medius_usage_key(0x04)),
                MediusStatus::Ok
            );
            assert_eq!(
                medius_clip_frame_force_release(frame, medius_usage_media(0xCD)),
                MediusStatus::Ok
            );
            let report = [0x10u8, 0xFF, 0x05];
            assert_eq!(
                medius_clip_frame_raw(frame, 2, out, report.as_ptr(), report.len()),
                MediusStatus::Ok
            );
            assert_eq!(
                medius_clip_frame_raw(frame, 1, MediusDirection::Positive as u8, ptr::null(), 0),
                MediusStatus::Ok
            );
            let data = [0x04u8, 0x01];
            assert_eq!(
                medius_clip_frame_transfer(frame, 0, c_out, data.as_ptr(), data.len()),
                MediusStatus::Ok
            );
            assert_eq!(
                medius_clip_frame_transfer(frame, 0, c_in, ptr::null(), 0),
                MediusStatus::Ok
            );
            // The builder copies the frame, so the same handle appends twice.
            assert_eq!(medius_clip_builder_frame(builder, frame), MediusStatus::Ok);
            assert_eq!(medius_clip_builder_frame(builder, frame), MediusStatus::Ok);
            assert_eq!(medius_clip_builder_pan(builder, 7), MediusStatus::Ok);
            assert_eq!(
                medius_clip_builder_raw(builder, 2, out, [0xAAu8].as_ptr(), 1),
                MediusStatus::Ok
            );
            let data = [0x01u8, 0x02];
            assert_eq!(
                medius_clip_builder_transfer(builder, 0, c_out, data.as_ptr(), data.len()),
                MediusStatus::Ok
            );
            assert_eq!(medius_clip_frame_clear(frame), MediusStatus::Ok);
            assert_eq!(medius_clip_frame_pan(frame, 9), MediusStatus::Ok);
            assert_eq!(medius_clip_builder_frame(builder, frame), MediusStatus::Ok);
            let mut clip: *mut MediusClip = ptr::null_mut();
            assert_eq!(medius_device_clip(dev, &mut clip), MediusStatus::Ok);
            assert_eq!(medius_clip_append(clip, builder), MediusStatus::Ok);
            medius_clip_free(clip);
            medius_clip_frame_free(frame);
            medius_clip_builder_free(builder);
        },
    );
}

#[test]
fn a_refused_clip_frame_has_its_own_status_and_sends_nothing() {
    let mock = medius_mock_new();
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
    let builder = medius_clip_builder_new();
    let frame = medius_clip_frame_new();
    let left = medius_usage_button(MediusButton::Left as u8);
    let out = MediusDirection::Negative as u8;
    let byte = [0u8];
    let setup = MediusSetup {
        request_type: 0x21,
        request: 0x09,
        value: 0x0300,
        index: 0,
        length: 2,
    };

    // A good entry ahead of each bad one: the refusal must hold the whole append back.
    let refused = |status: MediusStatus, what: &str| unsafe {
        assert_eq!(medius_clip_builder_clear(builder), MediusStatus::Ok);
        assert_eq!(medius_clip_builder_move(builder, 1, 1), MediusStatus::Ok);
        assert_eq!(medius_clip_builder_frame(builder, frame), MediusStatus::Ok);
        assert_eq!(medius_clip_append(clip, builder), status, "{what}");
        assert_eq!(medius_clip_frame_clear(frame), MediusStatus::Ok);
    };

    unsafe {
        for _ in 0..=MEDIUS_CLIP_EDGES_MAX {
            assert_eq!(medius_clip_frame_press(frame, left), MediusStatus::Ok);
        }
    }
    refused(MediusStatus::ErrClipFrameCount, "edges");

    unsafe {
        for _ in 0..=MEDIUS_CLIP_RAW_MAX {
            assert_eq!(
                medius_clip_frame_raw(frame, 1, out, byte.as_ptr(), 1),
                MediusStatus::Ok
            );
        }
    }
    refused(MediusStatus::ErrClipFrameCount, "raw reports");

    let long = [0u8; MEDIUS_CLIP_ENTRY_MAX];
    assert_eq!(
        unsafe { medius_clip_frame_raw(frame, 1, out, long.as_ptr(), long.len()) },
        MediusStatus::Ok
    );
    refused(MediusStatus::ErrClipFrameTooLong, "entry length");

    // An OUT request announcing two bytes and carrying one.
    assert_eq!(
        unsafe { medius_clip_frame_transfer(frame, 0, setup, byte.as_ptr(), 1) },
        MediusStatus::Ok
    );
    refused(MediusStatus::ErrClipTransferData, "transfer data");

    assert_eq!(
        unsafe { medius_clip_frame_raw(frame, 1, MediusDirection::Both as u8, byte.as_ptr(), 1) },
        MediusStatus::Ok
    );
    refused(MediusStatus::ErrRawDirection, "raw direction");

    assert_eq!(
        unsafe { medius_clip_frame_raw(frame, 1, MediusDirection::With as u8, byte.as_ptr(), 1) },
        MediusStatus::Ok
    );
    refused(MediusStatus::ErrRelativeDirection, "relative raw direction");

    let frames = unsafe { (*mock).inner.recorded_frames() };
    assert!(
        frames.iter().all(|f| f.ty != medius::FrameType::ClipAppend),
        "a refused append put a frame on the wire: {frames:?}"
    );
    unsafe {
        medius_clip_frame_free(frame);
        medius_clip_builder_free(builder);
        medius_clip_free(clip);
        medius_device_free(dev);
        medius_mock_free(mock);
    }
}

#[test]
fn the_clip_entry_points_refuse_a_null() {
    let builder = medius_clip_builder_new();
    let frame = medius_clip_frame_new();
    let left = medius_usage_button(MediusButton::Left as u8);
    let out = MediusDirection::Negative as u8;
    let byte = [0u8];
    let setup = MediusSetup {
        request_type: 0x21,
        request: 0x09,
        value: 0x0300,
        index: 0,
        length: 1,
    };
    let none: *mut MediusClipFrame = ptr::null_mut();
    for (what, status) in [
        ("frame clear", unsafe { medius_clip_frame_clear(none) }),
        ("frame move", unsafe { medius_clip_frame_move(none, 1, 1) }),
        ("frame wheel", unsafe { medius_clip_frame_wheel(none, 1) }),
        ("frame pan", unsafe { medius_clip_frame_pan(none, 1) }),
        ("frame edge", unsafe {
            medius_clip_frame_edge(none, left, MediusAction::Press as u8)
        }),
        ("frame press", unsafe {
            medius_clip_frame_press(none, left)
        }),
        ("frame release", unsafe {
            medius_clip_frame_release(none, left)
        }),
        ("frame force_release", unsafe {
            medius_clip_frame_force_release(none, left)
        }),
        ("frame raw", unsafe {
            medius_clip_frame_raw(none, 1, out, byte.as_ptr(), 1)
        }),
        ("frame raw bytes", unsafe {
            medius_clip_frame_raw(frame, 1, out, ptr::null(), 1)
        }),
        ("frame transfer", unsafe {
            medius_clip_frame_transfer(none, 0, setup, byte.as_ptr(), 1)
        }),
        ("frame transfer data", unsafe {
            medius_clip_frame_transfer(frame, 0, setup, ptr::null(), 1)
        }),
        ("builder frame, null builder", unsafe {
            medius_clip_builder_frame(ptr::null_mut(), frame)
        }),
        ("builder frame, null frame", unsafe {
            medius_clip_builder_frame(builder, ptr::null())
        }),
        ("builder pan", unsafe {
            medius_clip_builder_pan(ptr::null_mut(), 1)
        }),
        ("builder raw", unsafe {
            medius_clip_builder_raw(ptr::null_mut(), 1, out, byte.as_ptr(), 1)
        }),
        ("builder raw bytes", unsafe {
            medius_clip_builder_raw(builder, 1, out, ptr::null(), 1)
        }),
        ("builder transfer", unsafe {
            medius_clip_builder_transfer(ptr::null_mut(), 0, setup, byte.as_ptr(), 1)
        }),
        ("builder transfer data", unsafe {
            medius_clip_builder_transfer(builder, 0, setup, ptr::null(), 1)
        }),
        ("bind_packet, null clip", unsafe {
            medius_clip_bind_packet(ptr::null_mut(), &spec_trigger())
        }),
        ("unbind_packet, null clip", unsafe {
            medius_clip_unbind_packet(ptr::null_mut(), &spec_trigger())
        }),
    ] {
        assert_eq!(status, MediusStatus::ErrInvalidArg, "{what}");
    }
    let mut byte_out = 0u8;
    unsafe {
        assert!(!medius_traffic_event_transfer_status(
            ptr::null(),
            &mut byte_out
        ));
        medius_clip_frame_free(frame);
        medius_clip_builder_free(builder);
    }
}

// The CLIP_APPEND payloads a C-side build sends, joined in order.
unsafe fn appended(build: impl FnOnce(*mut MediusClipBuilder)) -> Vec<u8> {
    let frames = unsafe {
        capi_frames(|dev| {
            let builder = medius_clip_builder_new();
            build(builder);
            let mut clip: *mut MediusClip = ptr::null_mut();
            assert_eq!(medius_device_clip(dev, &mut clip), MediusStatus::Ok);
            assert_eq!(medius_clip_append(clip, builder), MediusStatus::Ok);
            medius_clip_free(clip);
            medius_clip_builder_free(builder);
        })
    };
    frames
        .into_iter()
        .filter(|f| f.ty == medius::FrameType::ClipAppend)
        .flat_map(|f| f.payload)
        .collect()
}

#[test]
fn a_clip_transfer_keeps_its_endpoint() {
    let setup = MediusSetup {
        request_type: 0x21,
        request: 0x09,
        value: 0x0300,
        index: 0,
        length: 1,
    };
    let bytes = unsafe {
        appended(|builder| {
            let frame = medius_clip_frame_new();
            assert_eq!(
                medius_clip_frame_transfer(frame, 3, setup, [0xAAu8].as_ptr(), 1),
                MediusStatus::Ok
            );
            assert_eq!(medius_clip_builder_frame(builder, frame), MediusStatus::Ok);
            assert_eq!(
                medius_clip_builder_transfer(builder, 5, setup, [0xBBu8].as_ptr(), 1),
                MediusStatus::Ok
            );
            medius_clip_frame_free(frame);
        })
    };
    // [flags XFER][n][ep][setup 8][data], once per entry.
    let setup_bytes = [0x21, 0x09, 0x00, 0x03, 0x00, 0x00, 0x01, 0x00];
    let mut want = vec![0x20, 0x01, 0x03];
    want.extend_from_slice(&setup_bytes);
    want.push(0xAA);
    want.extend_from_slice(&[0x20, 0x01, 0x05]);
    want.extend_from_slice(&setup_bytes);
    want.push(0xBB);
    assert_eq!(bytes, want);
}

#[test]
fn a_cleared_clip_frame_encodes_as_an_empty_one() {
    let setup = MediusSetup {
        request_type: 0x21,
        request: 0x09,
        value: 0x0300,
        index: 0,
        length: 1,
    };
    let bytes = unsafe {
        appended(|builder| {
            let frame = medius_clip_frame_new();
            let out = MediusDirection::Negative as u8;
            assert_eq!(medius_clip_frame_move(frame, 4, -2), MediusStatus::Ok);
            assert_eq!(
                medius_clip_frame_press(frame, medius_usage_button(MediusButton::Left as u8)),
                MediusStatus::Ok
            );
            assert_eq!(
                medius_clip_frame_raw(frame, 2, out, [0x10u8, 0xFF].as_ptr(), 2),
                MediusStatus::Ok
            );
            assert_eq!(
                medius_clip_frame_transfer(frame, 3, setup, [0xAAu8].as_ptr(), 1),
                MediusStatus::Ok
            );
            assert!(medius_clip_frame_byte_len(frame) > 5);
            assert_eq!(medius_clip_frame_clear(frame), MediusStatus::Ok);
            assert_eq!(medius_clip_frame_byte_len(frame), 5);
            assert_eq!(medius_clip_builder_frame(builder, frame), MediusStatus::Ok);
            medius_clip_frame_free(frame);
        })
    };
    // A frame carrying nothing is a zero XY tick.
    assert_eq!(bytes, [0x01, 0, 0, 0, 0]);
}

#[test]
fn byte_len_is_what_an_append_puts_in_the_ring() {
    let setup = MediusSetup {
        request_type: 0x21,
        request: 0x09,
        value: 0x0300,
        index: 0,
        length: 2,
    };
    let mut frame_len = 0;
    let mut builder_len = 0;
    let bytes = unsafe {
        appended(|builder| {
            let frame = medius_clip_frame_new();
            let out = MediusDirection::Negative as u8;
            assert_eq!(medius_clip_frame_byte_len(frame), 5);
            assert_eq!(medius_clip_frame_move(frame, 4, -2), MediusStatus::Ok);
            assert_eq!(medius_clip_frame_wheel(frame, 1), MediusStatus::Ok);
            assert_eq!(medius_clip_frame_pan(frame, -3), MediusStatus::Ok);
            assert_eq!(
                medius_clip_frame_press(frame, medius_usage_button(MediusButton::Left as u8)),
                MediusStatus::Ok
            );
            assert_eq!(
                medius_clip_frame_raw(frame, 2, out, [0x10u8, 0xFF, 0x05].as_ptr(), 3),
                MediusStatus::Ok
            );
            assert_eq!(
                medius_clip_frame_transfer(frame, 3, setup, [0x04u8, 0x01].as_ptr(), 2),
                MediusStatus::Ok
            );
            frame_len = medius_clip_frame_byte_len(frame);
            assert_eq!(medius_clip_builder_byte_len(builder), 0);
            assert_eq!(medius_clip_builder_frame(builder, frame), MediusStatus::Ok);
            assert_eq!(medius_clip_builder_gap(builder, 4), MediusStatus::Ok);
            assert_eq!(medius_clip_builder_pan(builder, 7), MediusStatus::Ok);
            builder_len = medius_clip_builder_byte_len(builder);
            medius_clip_frame_free(frame);
        })
    };
    // flags + XY + wheel + pan + (n + one edge) + (n + raw header + 3) + (n + ep + setup + 2)
    assert_eq!(
        frame_len,
        1 + 4 + 2 + 2 + (1 + 4) + (1 + 4 + 3) + (1 + 9 + 2)
    );
    assert_eq!(builder_len, frame_len + 3 + 3);
    assert_eq!(builder_len, bytes.len());
    unsafe {
        assert_eq!(medius_clip_frame_byte_len(ptr::null()), 0);
        assert_eq!(medius_clip_builder_byte_len(ptr::null()), 0);
    }
}

#[test]
fn the_clip_status_layout_is_the_one_the_header_declares() {
    use std::mem::{offset_of, size_of};
    assert_eq!(offset_of!(MediusClipStatus, state), 0);
    assert_eq!(offset_of!(MediusClipStatus, free), 4);
    assert_eq!(offset_of!(MediusClipStatus, total), 8);
    assert_eq!(offset_of!(MediusClipStatus, played), 12);
    assert_eq!(offset_of!(MediusClipStatus, ticks), 16);
    assert_eq!(offset_of!(MediusClipStatus, underruns), 20);
    assert_eq!(offset_of!(MediusClipStatus, overruns), 22);
    assert_eq!(offset_of!(MediusClipStatus, seq_gaps), 24);
    assert_eq!(offset_of!(MediusClipStatus, xfers), 26);
    assert_eq!(offset_of!(MediusClipStatus, xfer_errs), 28);
    assert_eq!(offset_of!(MediusClipStatus, gated), 30);
    assert_eq!(offset_of!(MediusClipStatus, held_n), 32);
    assert_eq!(offset_of!(MediusClipStatus, held), 34);
    assert_eq!(
        size_of::<MediusClipStatus>(),
        (34 + MEDIUS_MAX_USAGES * size_of::<MediusUsage>()).next_multiple_of(4)
    );
}

#[test]
fn clip_status_query_returns_configured_value() {
    let mock = medius_mock_new();
    let mut status: MediusClipStatus = unsafe { std::mem::zeroed() };
    status.state = MediusClipState::Playing as u8;
    status.free = 512;
    status.total = 40;
    status.played = 16;
    status.ticks = 99;
    status.underruns = 2;
    status.seq_gaps = 1;
    status.xfers = 7;
    status.xfer_errs = 3;
    status.gated = 5;
    status.held_n = 2;
    status.held[0] = medius_usage_button(MediusButton::Side1 as u8);
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
    assert_eq!((out.xfers, out.xfer_errs, out.gated), (7, 3, 5));
    assert_eq!(out.held_n, 2);
    assert_eq!(out.held[0], medius_usage_button(MediusButton::Side1 as u8));
    assert_eq!(out.held[1], medius_usage_key(MEDIUS_KEY_A));
    unsafe {
        medius_clip_free(clip);
        medius_device_free(dev);
        medius_mock_free(mock);
    }
}

#[test]
fn every_enum_byte_on_the_boundary_is_refused_rather_than_materialized() {
    // A byte a caller chose, read as a `#[repr(u8)]` enum, is undefined before any check can run:
    // the value falls outside the match's jump table and the process dies, or lands on a constant it
    // never named and the wrong command goes out. Each of these takes the byte and refuses it.
    const BAD: u8 = 200;
    let mock = medius_mock_new();
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
    let builder = medius_clip_builder_new();
    let frame = medius_clip_frame_new();
    let left = medius_usage_button(MediusButton::Left as u8);
    let bad_usage = MediusUsage { kind: BAD, id: 3 };
    let bad_motion = MediusMotion {
        kind: BAD,
        dx: 1,
        dy: 1,
        wheel: 0,
        pan: 0,
    };
    let ride = MediusMoveTiming::Ride as u8;
    let keep = MediusPendingMotion::Keep as u8;
    for (what, status) in [
        ("led target", unsafe { medius_device_led(dev, BAD, 2, 55) }),
        ("led mode", unsafe { medius_device_led(dev, 0, BAD, 5) }),
        ("reboot target", unsafe { medius_device_reboot(dev, BAD) }),
        ("emit mode", unsafe {
            medius_device_set_emit_pace(dev, BAD, 1000, 0)
        }),
        ("render mode", unsafe {
            medius_device_set_render(dev, BAD, false)
        }),
        ("inject action", unsafe {
            medius_device_inject(dev, left, BAD)
        }),
        ("press usage kind", unsafe {
            medius_device_press(dev, bad_usage)
        }),
        ("move_axis motion kind", unsafe {
            medius_device_move_axis(dev, bad_motion, ride, keep)
        }),
        ("move_axis timing", unsafe {
            medius_device_move_axis(dev, medius_motion_cursor(1, 1), BAD, keep)
        }),
        ("move_axis pending", unsafe {
            medius_device_move_axis(dev, medius_motion_cursor(1, 1), ride, BAD)
        }),
        ("lock target kind", unsafe {
            medius_device_lock(dev, medius_lock_target_axis(BAD), 0)
        }),
        ("clip edge action", unsafe {
            medius_clip_builder_edge(builder, left, BAD)
        }),
        ("clip frame edge action", unsafe {
            medius_clip_frame_edge(frame, left, BAD)
        }),
        ("clip frame edge usage kind", unsafe {
            medius_clip_frame_edge(frame, bad_usage, MediusAction::Press as u8)
        }),
        ("clip frame press usage kind", unsafe {
            medius_clip_frame_press(frame, bad_usage)
        }),
        ("clip frame release usage kind", unsafe {
            medius_clip_frame_release(frame, bad_usage)
        }),
        ("clip frame force_release usage kind", unsafe {
            medius_clip_frame_force_release(frame, bad_usage)
        }),
        ("clip frame raw direction", unsafe {
            medius_clip_frame_raw(frame, 1, BAD, [0u8].as_ptr(), 1)
        }),
        ("clip builder raw direction", unsafe {
            medius_clip_builder_raw(builder, 1, BAD, [0u8].as_ptr(), 1)
        }),
        ("clip bind_packet class", unsafe {
            medius_clip_bind_packet(
                clip,
                &MediusClipPacketTrigger {
                    class: BAD,
                    ..spec_trigger()
                },
            )
        }),
        ("clip bind_packet input class", unsafe {
            medius_clip_bind_packet(
                clip,
                &MediusClipPacketTrigger {
                    class: MEDIUS_CATCH_CLASS_KEY,
                    ..spec_trigger()
                },
            )
        }),
        ("clip bind_packet wildcard class", unsafe {
            medius_clip_bind_packet(
                clip,
                &MediusClipPacketTrigger {
                    class: MEDIUS_CATCH_CLASS_ANY,
                    ..spec_trigger()
                },
            )
        }),
        ("clip bind_packet direction", unsafe {
            medius_clip_bind_packet(
                clip,
                &MediusClipPacketTrigger {
                    direction: BAD,
                    ..spec_trigger()
                },
            )
        }),
        ("clip bind_packet action", unsafe {
            medius_clip_bind_packet(
                clip,
                &MediusClipPacketTrigger {
                    action: BAD,
                    ..spec_trigger()
                },
            )
        }),
        ("clip unbind_packet class", unsafe {
            medius_clip_unbind_packet(
                clip,
                &MediusClipPacketTrigger {
                    class: BAD,
                    ..spec_trigger()
                },
            )
        }),
        ("clip unbind_packet direction", unsafe {
            medius_clip_unbind_packet(
                clip,
                &MediusClipPacketTrigger {
                    direction: BAD,
                    ..spec_trigger()
                },
            )
        }),
        ("clip autolock group", unsafe {
            medius_clip_set_autolock(clip, [BAD].as_ptr(), 1)
        }),
        ("clip unbind edge", unsafe {
            medius_clip_unbind(clip, left, BAD)
        }),
        ("clip bind edge", unsafe {
            medius_clip_bind(
                clip,
                MediusClipTrigger {
                    on: left,
                    edge: BAD,
                    action: MediusClipAction::Start as u8,
                    consume: 0,
                },
            )
        }),
        ("clip bind action", unsafe {
            medius_clip_bind(
                clip,
                MediusClipTrigger {
                    on: left,
                    edge: MediusEdge::Press as u8,
                    action: BAD,
                    consume: 0,
                },
            )
        }),
    ] {
        assert_eq!(status, MediusStatus::ErrInvalidArg, "{what}");
    }

    // A filter constructor has no status, so a byte no constant names becomes one that addresses
    // nothing and the subscribe call refuses.
    let mut stream: *mut MediusEventStream = ptr::null_mut();
    for (what, f) in [
        ("watch_class", medius_catch_filter_watch_class(BAD)),
        ("watch_axis", medius_catch_filter_watch_axis(BAD)),
        ("watch usage kind", medius_catch_filter_watch(bad_usage)),
    ] {
        assert_eq!(
            unsafe { medius_device_catch_events(dev, &f, 1, &mut stream) },
            MediusStatus::ErrInvalidArg,
            "{what}"
        );
    }

    // Every refusal left the frame and the builder as they were: one empty frame is all it holds.
    assert_eq!(
        unsafe { medius_clip_builder_frame(builder, frame) },
        MediusStatus::Ok
    );
    let mut fresh = medius::ClipBuilder::new();
    fresh.frame(medius::ClipFrame::new());
    assert_eq!(unsafe { &(*builder).inner }, &fresh);

    // The no-status surfaces answer their fallback rather than acting on the byte.
    assert!(!unsafe { medius_mock_saw(mock, BAD) });
    let timeline = medius_timeline_new();
    unsafe { medius_timeline_reset(timeline, BAD) };
    assert_eq!(unsafe { medius_timeline_samples(timeline, BAD) }, 0);

    // Nothing refused reached the box: the handshake is the only frame recorded.
    let frames = unsafe { (*mock).inner.recorded_frames() };
    assert!(
        frames
            .iter()
            .all(|f| f.ty == medius::FrameType::Query || f.ty == medius::FrameType::ClipCtrl),
        "an invalid byte put a frame on the wire: {frames:?}"
    );

    unsafe {
        medius_timeline_free(timeline);
        medius_clip_frame_free(frame);
        medius_clip_builder_free(builder);
        medius_clip_free(clip);
        medius_device_free(dev);
        medius_mock_free(mock);
    }
}

#[test]
fn firmware_update_frames_match_the_native_crate() {
    let image: Vec<u8> = (0..1200u32).map(|i| (i % 251) as u8).collect();
    let native = native_frames(|d| {
        d.stage_firmware(medius::UpdateTarget::Host, &image, &mut |_| {})
            .expect("staged");
        d.activate_firmware().expect("activated");
    });
    let img = image.clone();
    let calls = |dev: *mut MediusDevice| {
        assert_eq!(
            unsafe {
                medius_device_stage_firmware(dev, 1, img.as_ptr(), img.len(), None, ptr::null_mut())
            },
            MediusStatus::Ok
        );
        assert_eq!(
            unsafe { medius_device_activate_firmware(dev) },
            MediusStatus::Ok
        );
    };
    let capi = unsafe { capi_frames(calls) };
    assert_eq!(
        native, capi,
        "the two paths must put identical bytes on the wire"
    );
    assert!(
        native.iter().any(|f| f.ty == medius::FrameType::Update),
        "the transfer must actually have sent UPDATE frames"
    );
}

#[test]
fn firmware_info_decodes_the_same_through_both() {
    let mock = MockBox::new();
    let dev = Device::with_mock(mock.clone());
    let native = dev.firmware_info().expect("native");

    let cmock = medius_mock_new();
    let mut cdev: *mut MediusDevice = ptr::null_mut();
    assert_eq!(
        unsafe { medius_device_with_mock(cmock, &mut cdev) },
        MediusStatus::Ok
    );
    let mut out = MediusFirmwareInfo {
        device: MediusChipFirmware {
            major: 0,
            minor: 0,
            patch: 0,
            slot: 0,
            state: 0,
        },
        host_present: 0,
        host: MediusChipFirmware {
            major: 0,
            minor: 0,
            patch: 0,
            slot: 0,
            state: 0,
        },
        slot_size: 0,
        device_staged: 0,
        host_staged: 0,
    };
    assert_eq!(
        unsafe { medius_device_firmware_info(cdev, &mut out) },
        MediusStatus::Ok
    );
    unsafe {
        medius_device_free(cdev);
        medius_mock_free(cmock);
    }

    assert_eq!(out.device.major, native.device.major);
    assert_eq!(out.device.slot, native.device.slot);
    assert_eq!(out.slot_size, native.slot_size);
    assert_eq!(out.host_present, u8::from(native.host.is_some()));
}

#[test]
fn a_bad_update_target_is_refused_rather_than_sent() {
    let calls = |dev: *mut MediusDevice| {
        let img = [1u8, 2, 3, 4];
        assert_eq!(
            unsafe {
                medius_device_stage_firmware(dev, 9, img.as_ptr(), img.len(), None, ptr::null_mut())
            },
            MediusStatus::ErrInvalidArg
        );
    };
    let frames = unsafe { capi_frames(calls) };
    assert!(
        !frames.iter().any(|f| f.ty == medius::FrameType::Update),
        "a rejected target must not reach the wire"
    );
}

// --- Advanced control layer (§3.14): raw injection, control transfers, rewrite rules, descriptor patches ---

fn allowed_status() -> MediusImperfectStatus {
    MediusImperfectStatus {
        allowed: 1,
        over_capacity: 0,
        clone_imperfect: 0,
    }
}

fn native_frames_imperfect(f: impl FnOnce(&Device)) -> Vec<DecodedFrame> {
    let mock = MockBox::new().with_imperfect(true);
    let dev = Device::with_mock(mock.clone());
    f(&dev);
    mock.recorded_frames()
}

unsafe fn capi_frames_imperfect(f: impl FnOnce(*mut MediusDevice)) -> Vec<DecodedFrame> {
    let mock = medius_mock_new();
    unsafe { medius_mock_set_imperfect_status(mock, allowed_status()) };
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

fn assert_parity_imperfect(native: impl FnOnce(&Device), capi: impl FnOnce(*mut MediusDevice)) {
    let want = native_frames_imperfect(native);
    let got = unsafe { capi_frames_imperfect(capi) };
    assert_eq!(
        want, got,
        "C ABI advanced control layer frames differ from the native crate"
    );
}

// Eight args because a rewrite rule carries eight wire fields; a struct-of-args here would only move
// the same list one line up.
#[allow(clippy::too_many_arguments)]
fn c_rewrite(
    class: u8,
    id: u16,
    direction: u8,
    action: u8,
    offset: u16,
    match_bytes: &[u8],
    mask: &[u8],
    payload: &[u8],
) -> MediusRewriteRule {
    let mut r: MediusRewriteRule = unsafe { std::mem::zeroed() };
    r.class = class;
    r.id = id;
    r.direction = direction;
    r.action = action;
    r.offset = offset;
    r.match_len = match_bytes.len() as u16;
    r.mask_len = mask.len() as u16;
    r.payload_len = payload.len() as u16;
    r.match_bytes[..match_bytes.len()].copy_from_slice(match_bytes);
    r.mask[..mask.len()].copy_from_slice(mask);
    r.payload[..payload.len()].copy_from_slice(payload);
    r
}

fn c_patch(section: u8, cfg: u8, index: u8, offset: u16, bytes: &[u8]) -> MediusPatch {
    let mut p: MediusPatch = unsafe { std::mem::zeroed() };
    p.section = section;
    p.cfg = cfg;
    p.index = index;
    p.offset = offset;
    p.len = bytes.len() as u16;
    p.bytes[..bytes.len()].copy_from_slice(bytes);
    p
}

#[test]
fn dev_layer_commands_reach_the_wire_like_the_crate() {
    use medius::{Direction, Patch, PatchSection, RewriteAction, RewriteClass, RewriteRule};
    let big = vec![0xABu8; 40];
    let cbig = big.clone();
    assert_parity_imperfect(
        move |d| {
            d.raw(1, Direction::IN, &[0x00, 0x01, 0x02, 0x03]).unwrap();
            d.set_rewrite(
                &RewriteRule::new(RewriteClass::Emit, 1, Direction::IN, RewriteAction::Drop)
                    .matching(vec![0x01], vec![0xFF]),
            )
            .unwrap();
            d.set_rewrite(
                &RewriteRule::new(
                    RewriteClass::Control,
                    0,
                    Direction::Both,
                    RewriteAction::ReplyPatch,
                )
                .at_offset(4)
                .matching(vec![0x80, 0x06], vec![0xFF, 0xFF])
                .with_payload(big.clone()),
            )
            .unwrap();
            d.remove_rewrite(
                &RewriteRule::new(RewriteClass::Emit, 1, Direction::IN, RewriteAction::Drop)
                    .matching(vec![0x01], vec![0xFF]),
            )
            .unwrap();
            d.clear_rewrite().unwrap();
            d.set_patch(&Patch::new(PatchSection::Device, 8, [0x34, 0x12]))
                .unwrap();
            d.apply_patch().unwrap();
            d.clear_patch().unwrap();
        },
        move |dev| unsafe {
            let bytes = [0x00u8, 0x01, 0x02, 0x03];
            assert_eq!(
                medius_device_raw(
                    dev,
                    1,
                    MediusDirection::Positive as u8,
                    bytes.as_ptr(),
                    bytes.len()
                ),
                MediusStatus::Ok
            );
            let drop = c_rewrite(
                MediusRewriteClass::Emit as u8,
                1,
                MediusDirection::Positive as u8,
                MediusRewriteAction::Drop as u8,
                0,
                &[0x01],
                &[0xFF],
                &[],
            );
            assert_eq!(medius_device_set_rewrite(dev, &drop), MediusStatus::Ok);
            let reply = c_rewrite(
                MediusRewriteClass::Control as u8,
                0,
                MediusDirection::Both as u8,
                MediusRewriteAction::ReplyPatch as u8,
                4,
                &[0x80, 0x06],
                &[0xFF, 0xFF],
                &cbig,
            );
            assert_eq!(medius_device_set_rewrite(dev, &reply), MediusStatus::Ok);
            assert_eq!(medius_device_remove_rewrite(dev, &drop), MediusStatus::Ok);
            assert_eq!(medius_device_clear_rewrite(dev), MediusStatus::Ok);
            let patch = c_patch(MediusPatchSection::Device as u8, 0, 0, 8, &[0x34, 0x12]);
            assert_eq!(medius_device_set_patch(dev, &patch), MediusStatus::Ok);
            assert_eq!(medius_device_apply_patch(dev), MediusStatus::Ok);
            assert_eq!(medius_device_clear_patch(dev), MediusStatus::Ok);
        },
    );
}

#[test]
fn transfer_timeout_takes_its_own_reply_wait() {
    assert!(medius_default_transfer_timeout_ms() >= 800);
    let mock = medius_mock_new();
    unsafe {
        medius_mock_set_imperfect_status(mock, allowed_status());
        let data = [0x12u8, 0x01];
        medius_mock_set_transfer_reply(mock, 0x00, data.as_ptr(), data.len());
    }
    let mut dev: *mut MediusDevice = ptr::null_mut();
    assert_eq!(
        unsafe { medius_device_with_mock(mock, &mut dev) },
        MediusStatus::Ok
    );
    let setup = MediusSetup {
        request_type: 0x80,
        request: 0x06,
        value: 0x0100,
        index: 0x0000,
        length: 2,
    };
    let mut out: MediusTransferOutcome = unsafe { std::mem::zeroed() };
    assert_eq!(
        unsafe {
            medius_device_transfer_timeout(
                dev,
                0,
                setup,
                ptr::null(),
                0,
                medius_default_transfer_timeout_ms(),
                &mut out,
            )
        },
        MediusStatus::Ok
    );
    assert_eq!(out.status, MediusTransferStatus::Ok as u8);
    assert_eq!(out.len, 2);
    unsafe {
        medius_device_free(dev);
        medius_mock_free(mock);
    }
}

#[test]
fn transfer_roundtrips_the_devices_answer() {
    let want = {
        let mock = MockBox::new()
            .with_imperfect(true)
            .with_transfer_reply(0x00, &[0x12, 0x01, 0x10, 0x02]);
        let dev = Device::with_mock(mock);
        dev.transfer(0, medius::Setup::new(0x80, 0x06, 0x0100, 0x0000, 18), &[])
            .unwrap()
    };

    let mock = medius_mock_new();
    unsafe {
        medius_mock_set_imperfect_status(mock, allowed_status());
        let data = [0x12u8, 0x01, 0x10, 0x02];
        medius_mock_set_transfer_reply(mock, 0x00, data.as_ptr(), data.len());
    }
    let mut dev: *mut MediusDevice = ptr::null_mut();
    assert_eq!(
        unsafe { medius_device_with_mock(mock, &mut dev) },
        MediusStatus::Ok
    );
    let setup = MediusSetup {
        request_type: 0x80,
        request: 0x06,
        value: 0x0100,
        index: 0x0000,
        length: 18,
    };
    let mut out: MediusTransferOutcome = unsafe { std::mem::zeroed() };
    assert_eq!(
        unsafe { medius_device_transfer(dev, 0, setup, ptr::null(), 0, &mut out) },
        MediusStatus::Ok
    );
    assert_eq!(out.status, MediusTransferStatus::Ok as u8);
    assert_eq!(out.status, want.status.as_u8());
    assert_eq!(&out.data[..out.len as usize], want.data());
    unsafe {
        medius_device_free(dev);
        medius_mock_free(mock);
    }
}

#[test]
fn a_refused_transfer_carries_no_data() {
    // With the opt-in off the box answers REFUSED and no data, whatever a caller passes.
    let mock = medius_mock_new();
    let mut dev: *mut MediusDevice = ptr::null_mut();
    assert_eq!(
        unsafe { medius_device_with_mock(mock, &mut dev) },
        MediusStatus::Ok
    );
    let setup = MediusSetup {
        request_type: 0x80,
        request: 0x06,
        value: 0x0100,
        index: 0,
        length: 18,
    };
    let mut out: MediusTransferOutcome = unsafe { std::mem::zeroed() };
    assert_eq!(
        unsafe { medius_device_transfer(dev, 0, setup, ptr::null(), 0, &mut out) },
        MediusStatus::Ok
    );
    assert_eq!(out.status, MediusTransferStatus::Refused as u8);
    assert_eq!(out.len, 0);
    unsafe {
        medius_device_free(dev);
        medius_mock_free(mock);
    }
}

#[test]
fn a_rewrite_survives_the_query_roundtrip() {
    use medius::{Direction, RewriteAction, RewriteClass, RewriteRule};
    let payload = vec![0xABu8; 40];
    let (want_summary, want_entry) = {
        let mock = MockBox::new().with_imperfect(true);
        let dev = Device::with_mock(mock);
        let rule = RewriteRule::new(
            RewriteClass::Control,
            0,
            Direction::Both,
            RewriteAction::ReplyPatch,
        )
        .at_offset(258)
        .matching(vec![0x80, 0x06], vec![0xFF, 0xFF])
        .with_payload(payload.clone());
        dev.set_rewrite(&rule).unwrap();
        (
            dev.query_rewrite().unwrap(),
            dev.query_rewrite_entry(0).unwrap(),
        )
    };

    let mock = medius_mock_new();
    unsafe { medius_mock_set_imperfect_status(mock, allowed_status()) };
    let mut dev: *mut MediusDevice = ptr::null_mut();
    assert_eq!(
        unsafe { medius_device_with_mock(mock, &mut dev) },
        MediusStatus::Ok
    );
    let rule = c_rewrite(
        MediusRewriteClass::Control as u8,
        0,
        MediusDirection::Both as u8,
        MediusRewriteAction::ReplyPatch as u8,
        258,
        &[0x80, 0x06],
        &[0xFF, 0xFF],
        &payload,
    );
    assert_eq!(
        unsafe { medius_device_set_rewrite(dev, &rule) },
        MediusStatus::Ok
    );

    let mut table: MediusRewriteTable = unsafe { std::mem::zeroed() };
    assert_eq!(
        unsafe { medius_device_query_rewrite(dev, &mut table) },
        MediusStatus::Ok
    );
    assert_eq!(table.n as usize, want_summary.entries.len());
    assert_eq!(table.n, 1);
    assert_eq!(table.entries[0].class, MediusRewriteClass::Control as u8);
    assert_eq!(table.entries[0].offset, want_summary.entries[0].offset);
    assert_eq!(table.entries[0].offset, 258);
    assert_eq!(
        table.entries[0].payload_len,
        want_summary.entries[0].payload_len
    );

    let mut read: MediusRewriteRule = unsafe { std::mem::zeroed() };
    assert_eq!(
        unsafe { medius_device_query_rewrite_entry(dev, 0, &mut read) },
        MediusStatus::Ok
    );
    assert_eq!(read.action, MediusRewriteAction::ReplyPatch as u8);
    assert_eq!(read.offset, want_entry.offset);
    assert_eq!(read.offset, 258);
    assert_eq!(read.payload_len as usize, want_entry.payload.len());
    assert_eq!(
        &read.payload[..read.payload_len as usize],
        &want_entry.payload[..]
    );
    assert_eq!(
        &read.match_bytes[..read.match_len as usize],
        &want_entry.match_bytes[..]
    );
    unsafe {
        medius_device_free(dev);
        medius_mock_free(mock);
    }
}

#[test]
fn a_rewrite_match_past_the_limit_has_its_own_status() {
    let mock = medius_mock_new();
    unsafe { medius_mock_set_imperfect_status(mock, allowed_status()) };
    let mut dev: *mut MediusDevice = ptr::null_mut();
    assert_eq!(
        unsafe { medius_device_with_mock(mock, &mut dev) },
        MediusStatus::Ok
    );
    let rule = |match_len: u16, mask_len: u16| {
        let mut r = c_rewrite(
            MediusRewriteClass::HidIn as u8,
            2,
            MediusDirection::Positive as u8,
            MediusRewriteAction::Pass as u8,
            0,
            &[0x11; MEDIUS_MAX_REWRITE_MATCH],
            &[0xFF; MEDIUS_MAX_REWRITE_MATCH],
            &[],
        );
        r.match_len = match_len;
        r.mask_len = mask_len;
        r
    };
    let max = MEDIUS_MAX_REWRITE_MATCH as u16;
    let rewrites = || {
        unsafe { (*mock).inner.recorded_frames() }
            .into_iter()
            .filter(|f| f.ty == medius::FrameType::Rewrite)
            .count()
    };
    for (match_len, mask_len, status) in [
        (max + 1, max + 1, MediusStatus::ErrRewriteMatchTooLong),
        (u16::MAX, u16::MAX, MediusStatus::ErrRewriteMatchTooLong),
        // The mask length is checked first, as the crate does.
        (max + 1, max, MediusStatus::ErrRewriteMaskLength),
        (max, max + 1, MediusStatus::ErrRewriteMaskLength),
    ] {
        let r = rule(match_len, mask_len);
        assert_eq!(unsafe { medius_device_set_rewrite(dev, &r) }, status);
        assert_eq!(unsafe { medius_device_remove_rewrite(dev, &r) }, status);
    }
    assert_eq!(rewrites(), 0, "a refused rule put a frame on the wire");

    let full = rule(max, max);
    assert_eq!(
        unsafe { medius_device_set_rewrite(dev, &full) },
        MediusStatus::Ok
    );
    assert_eq!(rewrites(), 1);
    let mut read: MediusRewriteRule = unsafe { std::mem::zeroed() };
    assert_eq!(
        unsafe { medius_device_query_rewrite_entry(dev, 0, &mut read) },
        MediusStatus::Ok
    );
    assert_eq!(read.match_len, max);
    assert_eq!(read.match_bytes, full.match_bytes);
    assert_eq!(
        unsafe { medius_device_remove_rewrite(dev, &full) },
        MediusStatus::Ok
    );
    unsafe {
        medius_device_free(dev);
        medius_mock_free(mock);
    }
}

#[test]
fn a_patch_survives_the_query_roundtrip() {
    use medius::Patch;
    let bytes = vec![0xCDu8; 40];
    let (want_set, want_entry) = {
        let mock = MockBox::new().with_imperfect(true);
        let dev = Device::with_mock(mock);
        dev.set_patch(&Patch::in_interface(1, 2, 258, bytes.clone()))
            .unwrap();
        (
            dev.query_patches().unwrap(),
            dev.query_patch_entry(0).unwrap(),
        )
    };

    let mock = medius_mock_new();
    let mut dev: *mut MediusDevice = ptr::null_mut();
    assert_eq!(
        unsafe { medius_device_with_mock(mock, &mut dev) },
        MediusStatus::Ok
    );
    // set_patch is not gated on the opt-in; the box always stores it.
    let patch = c_patch(MediusPatchSection::Report as u8, 1, 2, 258, &bytes);
    assert_eq!(
        unsafe { medius_device_set_patch(dev, &patch) },
        MediusStatus::Ok
    );

    let mut set: MediusPatchSet = unsafe { std::mem::zeroed() };
    assert_eq!(
        unsafe { medius_device_query_patches(dev, &mut set) },
        MediusStatus::Ok
    );
    assert_eq!(set.n as usize, want_set.entries.len());
    assert_eq!(set.n, 1);
    assert_eq!(set.entries[0].section, MediusPatchSection::Report as u8);
    assert_eq!(set.entries[0].offset, 258);
    assert_eq!(set.entries[0].len, want_set.entries[0].len);

    let mut read: MediusPatch = unsafe { std::mem::zeroed() };
    assert_eq!(
        unsafe { medius_device_query_patch_entry(dev, 0, &mut read) },
        MediusStatus::Ok
    );
    assert_eq!(read.section, MediusPatchSection::Report as u8);
    assert_eq!(read.offset, 258);
    assert_eq!(read.len as usize, want_entry.bytes.len());
    assert_eq!(&read.bytes[..read.len as usize], &want_entry.bytes[..]);
    unsafe {
        medius_device_free(dev);
        medius_mock_free(mock);
    }
}

#[test]
fn raw_sends_with_the_opt_in_off() {
    let mock = medius_mock_new(); // imperfect off by default: the box drops the frame, the crate sends it
    let mut dev: *mut MediusDevice = ptr::null_mut();
    assert_eq!(
        unsafe { medius_device_with_mock(mock, &mut dev) },
        MediusStatus::Ok
    );
    let bytes = [0u8, 1];
    assert_eq!(
        unsafe {
            medius_device_raw(
                dev,
                1,
                MediusDirection::Positive as u8,
                bytes.as_ptr(),
                bytes.len(),
            )
        },
        MediusStatus::Ok
    );
    unsafe {
        medius_device_free(dev);
        medius_mock_free(mock);
    }
}

#[test]
fn the_gated_dev_layer_calls_are_refused_with_the_opt_in_off() {
    let mock = medius_mock_new(); // imperfect off by default
    let mut dev: *mut MediusDevice = ptr::null_mut();
    assert_eq!(
        unsafe { medius_device_with_mock(mock, &mut dev) },
        MediusStatus::Ok
    );
    let rule = c_rewrite(
        MediusRewriteClass::Emit as u8,
        1,
        MediusDirection::Positive as u8,
        MediusRewriteAction::Drop as u8,
        0,
        &[],
        &[],
        &[],
    );
    assert_eq!(
        unsafe { medius_device_set_rewrite(dev, &rule) },
        MediusStatus::ErrImperfectRequired
    );
    assert_eq!(
        unsafe { medius_device_apply_patch(dev) },
        MediusStatus::ErrImperfectRequired
    );
    unsafe {
        medius_device_free(dev);
        medius_mock_free(mock);
    }
}

#[test]
fn rewrite_validation_errors_have_their_own_status() {
    let mock = medius_mock_new();
    unsafe { medius_mock_set_imperfect_status(mock, allowed_status()) };
    let mut dev: *mut MediusDevice = ptr::null_mut();
    assert_eq!(
        unsafe { medius_device_with_mock(mock, &mut dev) },
        MediusStatus::Ok
    );
    // match and mask of different lengths.
    let mask_mismatch = c_rewrite(
        MediusRewriteClass::Emit as u8,
        1,
        MediusDirection::Positive as u8,
        MediusRewriteAction::Drop as u8,
        0,
        &[0x01, 0x02],
        &[0xFF],
        &[],
    );
    assert_eq!(
        unsafe { medius_device_set_rewrite(dev, &mask_mismatch) },
        MediusStatus::ErrRewriteMaskLength
    );
    // Drop is a report-only action; not admissible on the control class.
    let bad_action = c_rewrite(
        MediusRewriteClass::Control as u8,
        0,
        MediusDirection::Both as u8,
        MediusRewriteAction::Drop as u8,
        0,
        &[],
        &[],
        &[],
    );
    assert_eq!(
        unsafe { medius_device_set_rewrite(dev, &bad_action) },
        MediusStatus::ErrRewriteActionClass
    );
    // A bearing-relative direction is rejected for a rewrite rule.
    let relative = c_rewrite(
        MediusRewriteClass::Emit as u8,
        1,
        MediusDirection::With as u8,
        MediusRewriteAction::Drop as u8,
        0,
        &[],
        &[],
        &[],
    );
    assert_eq!(
        unsafe { medius_device_set_rewrite(dev, &relative) },
        MediusStatus::ErrRelativeDirection
    );
    // A 100-byte Replace on a report surface exceeds the 64-byte head the box holds.
    let too_big = c_rewrite(
        MediusRewriteClass::Emit as u8,
        1,
        MediusDirection::Positive as u8,
        MediusRewriteAction::Replace as u8,
        0,
        &[],
        &[],
        &[0u8; 100],
    );
    assert_eq!(
        unsafe { medius_device_set_rewrite(dev, &too_big) },
        MediusStatus::ErrRewritePayloadTooLarge
    );
    // An unknown class byte is refused before the wire.
    let bad_class = c_rewrite(0x77, 0, MediusDirection::Both as u8, 0, 0, &[], &[], &[]);
    assert_eq!(
        unsafe { medius_device_set_rewrite(dev, &bad_class) },
        MediusStatus::ErrInvalidArg
    );
    // So is an action byte: `ReplyReplace` is the last one the box names.
    let bad_action_byte = c_rewrite(
        MediusRewriteClass::HidIn as u8,
        2,
        MediusDirection::Positive as u8,
        MediusRewriteAction::ReplyReplace as u8 + 1,
        0,
        &[],
        &[],
        &[],
    );
    assert_eq!(
        unsafe { medius_device_set_rewrite(dev, &bad_action_byte) },
        MediusStatus::ErrInvalidArg
    );
    unsafe {
        medius_device_free(dev);
        medius_mock_free(mock);
    }
}

#[test]
fn raw_rejects_a_non_flow_direction_with_its_own_status() {
    let mock = medius_mock_new();
    unsafe { medius_mock_set_imperfect_status(mock, allowed_status()) };
    let mut dev: *mut MediusDevice = ptr::null_mut();
    assert_eq!(
        unsafe { medius_device_with_mock(mock, &mut dev) },
        MediusStatus::Ok
    );
    let bytes = [0u8, 1];
    // Both names two flows and the bearing-relative pair has none here; each carries its own status,
    // ahead of the opt-in, and an unknown direction byte is refused as an invalid argument.
    assert_eq!(
        unsafe {
            medius_device_raw(
                dev,
                1,
                MediusDirection::Both as u8,
                bytes.as_ptr(),
                bytes.len(),
            )
        },
        MediusStatus::ErrRawDirection
    );
    assert_eq!(
        unsafe {
            medius_device_raw(
                dev,
                1,
                MediusDirection::With as u8,
                bytes.as_ptr(),
                bytes.len(),
            )
        },
        MediusStatus::ErrRelativeDirection
    );
    assert_eq!(
        unsafe { medius_device_raw(dev, 1, 99, bytes.as_ptr(), bytes.len()) },
        MediusStatus::ErrInvalidArg
    );
    unsafe {
        medius_device_free(dev);
        medius_mock_free(mock);
    }
}

#[test]
fn the_new_health_bits_cross_the_boundary() {
    let mock = medius_mock_new();
    let mut set: MediusHealth = unsafe { std::mem::zeroed() };
    set.rewrite_on = 1;
    set.transform_on = 1; // patch_on left 0
    unsafe { medius_mock_set_health(mock, set) };
    let mut dev: *mut MediusDevice = ptr::null_mut();
    assert_eq!(
        unsafe { medius_device_with_mock(mock, &mut dev) },
        MediusStatus::Ok
    );
    let mut out: MediusHealth = unsafe { std::mem::zeroed() };
    assert_eq!(
        unsafe { medius_device_query_health(dev, &mut out) },
        MediusStatus::Ok
    );
    assert_eq!(out.rewrite_on, 1);
    assert_eq!(out.patch_on, 0);
    assert_eq!(out.transform_on, 1);
    unsafe {
        medius_device_free(dev);
        medius_mock_free(mock);
    }
}

#[test]
fn pan_parity() {
    assert_parity(
        |d| {
            d.pan(3).unwrap();
        },
        |dev| unsafe {
            assert_eq!(medius_device_pan(dev, 3), MediusStatus::Ok);
        },
    );
}

#[test]
fn pan_now_parity() {
    assert_parity(
        |d| {
            d.pan_now(-2).unwrap();
        },
        |dev| unsafe {
            assert_eq!(medius_device_pan_now(dev, -2), MediusStatus::Ok);
        },
    );
}

#[test]
fn move_axis_pan_parity() {
    assert_parity(
        |d| {
            d.move_axis(
                medius::Motion::Pan(4),
                medius::MoveTiming::Now,
                medius::PendingMotion::Keep,
            )
            .unwrap();
        },
        |dev| unsafe {
            assert_eq!(
                medius_device_move_axis(
                    dev,
                    medius_motion_pan(4),
                    MediusMoveTiming::Now as u8,
                    MediusPendingMotion::Keep as u8
                ),
                MediusStatus::Ok
            );
        },
    );
}

#[test]
fn transform_verbs_parity() {
    use medius::{Axis, Transform};
    assert_parity(
        |d| {
            d.transform_swap(Axis::X, Axis::Y).unwrap();
            d.transform_remap(Axis::X, Axis::Wheel).unwrap();
            let moved = Transform::remap(Axis::Wheel, Axis::Y);
            d.transform(&moved).unwrap();
            d.untransform(&moved).unwrap();
            d.clear_transforms().unwrap();
        },
        |dev| unsafe {
            assert_eq!(
                medius_device_transform_swap(dev, MediusAxis::X as u8, MediusAxis::Y as u8),
                MediusStatus::Ok
            );
            assert_eq!(
                medius_device_transform_remap(
                    dev,
                    medius_lock_target_axis(MediusLockTargetKind::X as u8),
                    medius_lock_target_axis(MediusLockTargetKind::Wheel as u8)
                ),
                MediusStatus::Ok
            );
            let moved = MediusTransform {
                op: MediusTransformOp::Remap as u8,
                source: medius_lock_target_axis(MediusLockTargetKind::Wheel as u8),
                dest: medius_lock_target_axis(MediusLockTargetKind::Y as u8),
            };
            assert_eq!(medius_device_transform(dev, &moved), MediusStatus::Ok);
            assert_eq!(medius_device_untransform(dev, &moved), MediusStatus::Ok);
            assert_eq!(medius_device_clear_transforms(dev), MediusStatus::Ok);
        },
    );
}

#[test]
fn a_transform_survives_the_query_roundtrip() {
    use medius::Axis;
    let want = {
        let mock = MockBox::new();
        let dev = Device::with_mock(mock);
        dev.transform_swap(Axis::X, Axis::Y).unwrap();
        dev.transform_remap(Axis::Wheel, Axis::Y).unwrap();
        dev.transform_remap(Axis::Y, Axis::X).unwrap();
        dev.query_transforms().unwrap()
    };

    let mock = medius_mock_new();
    let mut dev: *mut MediusDevice = ptr::null_mut();
    assert_eq!(
        unsafe { medius_device_with_mock(mock, &mut dev) },
        MediusStatus::Ok
    );
    unsafe {
        assert_eq!(
            medius_device_transform_swap(dev, MediusAxis::X as u8, MediusAxis::Y as u8),
            MediusStatus::Ok
        );
        assert_eq!(
            medius_device_transform_remap(
                dev,
                medius_lock_target_axis(MediusLockTargetKind::Wheel as u8),
                medius_lock_target_axis(MediusLockTargetKind::Y as u8)
            ),
            MediusStatus::Ok
        );
        assert_eq!(
            medius_device_transform_remap(
                dev,
                medius_lock_target_axis(MediusLockTargetKind::Y as u8),
                medius_lock_target_axis(MediusLockTargetKind::X as u8)
            ),
            MediusStatus::Ok
        );
    }

    let mut got: MediusTransforms = unsafe { std::mem::zeroed() };
    assert_eq!(
        unsafe { medius_device_query_transforms(dev, &mut got) },
        MediusStatus::Ok
    );
    assert_eq!(got.n as usize, want.entries.len());
    assert_eq!(got.n, 3);
    // Entry for entry, the C readback matches the native crate's.
    for (i, w) in want.entries.iter().enumerate() {
        assert_eq!(got.entries[i].op, w.op.as_u8());
    }
    assert_eq!(got.entries[0].op, MediusTransformOp::Swap as u8);
    assert_eq!(got.entries[0].source.kind, MediusLockTargetKind::X as u8);
    assert_eq!(got.entries[0].dest.kind, MediusLockTargetKind::Y as u8);
    assert_eq!(got.entries[1].op, MediusTransformOp::Remap as u8);
    assert_eq!(
        got.entries[1].source.kind,
        MediusLockTargetKind::Wheel as u8
    );
    assert_eq!(got.entries[1].dest.kind, MediusLockTargetKind::Y as u8);
    assert_eq!(got.entries[2].op, MediusTransformOp::Remap as u8);
    assert_eq!(got.entries[2].source.kind, MediusLockTargetKind::Y as u8);
    assert_eq!(got.entries[2].dest.kind, MediusLockTargetKind::X as u8);
    unsafe {
        medius_device_free(dev);
        medius_mock_free(mock);
    }
}

#[test]
fn mouse_caps_pan_crosses_the_boundary() {
    let mock = medius_mock_new();
    let mut caps: MediusMouseCaps = unsafe { std::mem::zeroed() };
    caps.n_buttons = 5;
    caps.has_x = 1;
    caps.has_y = 1;
    caps.has_wheel = 1;
    caps.pan = 1;
    caps.n_hid = 1;
    unsafe { medius_mock_set_mouse_caps(mock, caps) };
    let mut dev: *mut MediusDevice = ptr::null_mut();
    assert_eq!(
        unsafe { medius_device_with_mock(mock, &mut dev) },
        MediusStatus::Ok
    );
    let mut out: MediusCaps = unsafe { std::mem::zeroed() };
    assert_eq!(
        unsafe { medius_device_caps(dev, &mut out) },
        MediusStatus::Ok
    );
    assert_eq!(out.mouse.pan, 1);
    assert_eq!(out.mouse.has_wheel, 1);
    // A clone that declares pan holds a pan-axis transform, so pan is first-class end to end.
    assert_eq!(
        unsafe {
            medius_device_transform_remap(
                dev,
                medius_lock_target_axis(MediusLockTargetKind::Wheel as u8),
                medius_lock_target_axis(MediusLockTargetKind::Pan as u8),
            )
        },
        MediusStatus::Ok
    );
    let mut tf: MediusTransforms = unsafe { std::mem::zeroed() };
    assert_eq!(
        unsafe { medius_device_query_transforms(dev, &mut tf) },
        MediusStatus::Ok
    );
    assert_eq!(tf.n, 1);
    assert_eq!(tf.entries[0].op, MediusTransformOp::Remap as u8);
    assert_eq!(tf.entries[0].dest.kind, MediusLockTargetKind::Pan as u8);
    unsafe {
        medius_device_free(dev);
        medius_mock_free(mock);
    }
}

#[test]
fn a_rewrite_past_the_payload_pool_has_its_own_status() {
    assert_eq!(MediusStatus::ErrRewritePoolFull as i32, 35);
    assert_eq!(MEDIUS_REWRITE_PAYLOAD_POOL, 2048);
    let mock = medius_mock_new();
    unsafe { medius_mock_set_imperfect_status(mock, allowed_status()) };
    let mut dev: *mut MediusDevice = ptr::null_mut();
    assert_eq!(
        unsafe { medius_device_with_mock(mock, &mut dev) },
        MediusStatus::Ok
    );
    let answer = |id: u16, len: usize| {
        c_rewrite(
            MediusRewriteClass::Control as u8,
            id,
            MediusDirection::Both as u8,
            MediusRewriteAction::Answer as u8,
            0,
            &[],
            &[],
            &vec![0x5A; len],
        )
    };
    for id in 0..4 {
        assert_eq!(
            unsafe { medius_device_set_rewrite(dev, &answer(id, 512 - 11)) },
            MediusStatus::Ok
        );
    }
    assert_eq!(
        unsafe { medius_device_set_rewrite(dev, &answer(4, 45)) },
        MediusStatus::ErrRewritePoolFull
    );
    assert_eq!(
        unsafe { medius_device_set_rewrite(dev, &answer(4, 44)) },
        MediusStatus::Ok
    );
    unsafe {
        medius_device_free(dev);
        medius_mock_free(mock);
    }
}

#[test]
fn a_mock_restart_is_recovered_and_the_clip_reports_its_loss() {
    let mock = medius_mock_new();
    let mut dev: *mut MediusDevice = ptr::null_mut();
    assert_eq!(
        unsafe { medius_device_open_mock(mock, &mut dev) },
        MediusStatus::Ok
    );
    let clip = unsafe { clip_of(dev) };
    let builder = medius_clip_builder_new();
    unsafe {
        assert_eq!(medius_clip_builder_move(builder, 1, 0), MediusStatus::Ok);
        assert_eq!(medius_clip_append(clip, builder), MediusStatus::Ok);
        assert!(!medius_clip_lost(clip));
        medius_mock_restart(mock);
    }
    let mut counters: MediusCountersSnapshot = unsafe { std::mem::zeroed() };
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    while counters.restarts == 0 {
        assert!(std::time::Instant::now() < deadline, "no recovery");
        std::thread::sleep(Duration::from_millis(5));
        assert_eq!(
            unsafe { medius_device_counters(dev, &mut counters) },
            MediusStatus::Ok
        );
    }
    unsafe {
        assert!(medius_clip_lost(clip));
        assert_eq!(medius_clip_append(clip, builder), MediusStatus::Ok);
        assert!(!medius_clip_lost(clip));
        assert!(!medius_clip_lost(ptr::null()));
        medius_mock_restart(ptr::null_mut());
        medius_clip_builder_free(builder);
        medius_clip_free(clip);
        medius_device_free(dev);
        medius_mock_free(mock);
    }
}

#[test]
fn the_session_counter_reads_through_and_the_mock_moves_it() {
    let mock = medius_mock_new();
    let mut dev: *mut MediusDevice = ptr::null_mut();
    assert_eq!(
        unsafe { medius_device_with_mock(mock, &mut dev) },
        MediusStatus::Ok
    );
    let session = || {
        let mut s: MediusStats = unsafe { std::mem::zeroed() };
        assert_eq!(
            unsafe { medius_device_query_stats(dev, &mut s) },
            MediusStatus::Ok
        );
        s.session
    };
    assert_eq!(session(), 0);
    assert_eq!(
        unsafe { medius_device_move_rel(dev, 1, 0) },
        MediusStatus::Ok
    );
    unsafe { medius_mock_link_lost(mock) };
    assert_eq!(session(), 1);
    assert_eq!(
        unsafe { medius_device_move_rel(dev, 1, 0) },
        MediusStatus::Ok
    );
    unsafe {
        medius_mock_detach(mock, false);
        medius_mock_attach(mock);
    }
    assert_eq!(session(), 2);
    unsafe {
        medius_mock_link_lost(ptr::null_mut());
        medius_mock_detach(ptr::null_mut(), true);
        medius_mock_attach(ptr::null_mut());
        medius_device_free(dev);
        medius_mock_free(mock);
    }
}
