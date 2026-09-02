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
            d.inject(medius::Button::Right, medius::Action::Press)
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
            d.press(medius::Button::Left).unwrap();
            d.release(medius::Button::Left).unwrap();
            d.force_release(medius::Button::Left).unwrap();
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
            d.lock(medius::Button::Side1, medius::Direction::Positive)
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
    // The payload bytes, derived from ctrl_proto.h: [class][id u16 LE][direction][scale].
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
            vec![3, 0, 0, 4, 40],  // AXIS X, AGAINST, 40%
            vec![3, 0, 0, 3, 130], // AXIS X, WITH, 130%
            vec![3, 1, 0, 3, 130], // AXIS Y, WITH, 130%
            vec![4, 35, 0, 1],     // OPTION(BEARING), 35 ms, VECTOR
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
    // `MediusLockEntry.direction` is a `uint8_t` the caller fills in through `medius_mock_set_locks`,
    // and Python has always handed it a raw byte. The setter has no status to return, so the entry is
    // dropped rather than read as whichever direction the byte resembles: a lock the host believes
    // in and the box never took is the failure this prevents.
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
    // `MediusCatchFilter.direction` is a `uint8_t` the caller fills in, and Python has always handed
    // it a raw byte. A filter helper has no status to return, so the byte rides the struct and the
    // subscription refuses it, rather than the box being handed whichever direction its low bits
    // resemble, or a stream that never yields.
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
                medius_usage_button(MediusButton::Left as u8),
                medius_usage_key(0x04),
            ];
            let actions = [MediusAction::Press as u8, MediusAction::Press as u8];
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
    status.state = MediusClipState::Playing as u8;
    status.free = 512;
    status.total = 40;
    status.played = 16;
    status.ticks = 99;
    status.underruns = 2;
    status.seq_gaps = 1;
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
    let left = medius_usage_button(MediusButton::Left as u8);
    let bad_usage = MediusUsage { kind: BAD, id: 3 };
    let bad_motion = MediusMotion {
        kind: BAD,
        dx: 1,
        dy: 1,
        wheel: 0,
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
        ("clip frame action", unsafe {
            medius_clip_builder_frame(builder, 0, 0, 0, [left].as_ptr(), [BAD].as_ptr(), 1)
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
