#![cfg(feature = "mock")]

use std::time::{Duration, Instant};

use crate::{
    Button, Device, DeviceInfo, DeviceKind, Error, FrameType, Health, LogLevel, MockBox,
    RebootTarget, Version,
};

#[test]
fn device_info_and_version_mac_round_trip_through_the_mock() {
    let mock = MockBox::new()
        .with_version(Version {
            proto_ver: 2,
            fw_major: 2,
            fw_minor: 3,
            fw_patch: 0,
            mac: [0x5A, 0x4E, 0x00, 0x11, 0x1e, 0x28],
            name: "Left PC".to_string(),
        })
        .with_device_info(DeviceInfo {
            vid: 0x1532,
            pid: 0x0072,
            bcd_device: 0x0200,
            bcd_usb: 0x0200,
            has_serial: true,
            has_bos: false,
            kind: DeviceKind::Mouse,
            product: "Razer Mamba Elite".to_string(),
        });
    let device = Device::with_mock(mock);

    let v = device.query_version().unwrap();
    assert_eq!(v.mac_hex(), "5a4e00111e28");
    assert_eq!(v.name, "Left PC"); // the name rides RESP(VERSION) beside the MAC

    let info = device.device_info().unwrap();
    assert_eq!((info.vid, info.pid), (0x1532, 0x0072));
    assert_eq!(info.kind, DeviceKind::Mouse);
    assert_eq!(info.product, "Razer Mamba Elite");
    assert!(info.has_serial && !info.has_bos);
}

#[test]
fn pushed_logs_reach_the_logs_channel_in_order() {
    let mock = MockBox::new();
    let device = Device::with_mock(mock.clone());
    let rx = device.logs();

    mock.push_log(LogLevel::Warn, "overheating");
    mock.push_log(LogLevel::Info, "recovered");

    let a = rx.recv_timeout(Duration::from_secs(1)).unwrap();
    let b = rx.recv_timeout(Duration::from_secs(1)).unwrap();
    assert_eq!((a.level, a.text.as_str()), (LogLevel::Warn, "overheating"));
    assert_eq!((b.level, b.text.as_str()), (LogLevel::Info, "recovered"));
}

#[test]
fn set_health_updates_subsequent_queries() {
    let mock = MockBox::new();
    let device = Device::with_mock(mock.clone());
    assert!(!device.query_health().unwrap().mouse_attached);
    mock.set_health(Health::from_flags(0x02));
    assert!(device.query_health().unwrap().mouse_attached);
}

#[test]
fn handshake_rejects_wrong_proto_ver() {
    let mock = MockBox::new().with_version(Version {
        proto_ver: 9,
        fw_major: 0,
        fw_minor: 0,
        fw_patch: 0,
        mac: [0; 6],
        name: String::new(),
    });
    let err = Device::open_mock(mock).unwrap_err();
    assert!(matches!(err, Error::BadProtoVer { got: 9 }), "got {err:?}");
}

#[test]
fn handshake_on_silent_box_is_no_reply() {
    let err = Device::open_mock(MockBox::new().silent()).unwrap_err();
    assert!(matches!(err, Error::NoReply), "got {err:?}");
}

#[test]
fn dropping_the_last_clone_joins_threads_without_hanging() {
    let device = Device::with_mock(MockBox::new());
    device.move_rel(1, 1).unwrap();
    let start = Instant::now();
    drop(device);
    assert!(
        start.elapsed() < Duration::from_millis(500),
        "drop took too long: {:?}",
        start.elapsed()
    );
}

#[test]
fn a_clone_keeps_the_reader_alive_until_the_last_drop() {
    let mock = MockBox::new();
    let device = Device::with_mock(mock.clone());
    let clone = device.clone();
    drop(device);
    let rx = clone.logs();
    mock.push_log(LogLevel::Info, "still here");
    assert_eq!(
        rx.recv_timeout(Duration::from_secs(1)).unwrap().text,
        "still here"
    );
}

#[test]
fn device_is_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Device>();
}

#[test]
fn reapply_re_emits_only_held_overrides() {
    let mock = MockBox::new();
    let device = Device::with_mock(mock.clone());
    device.press(Button::Left).unwrap();
    device.force_release(Button::Side1).unwrap();
    device.press(Button::Middle).unwrap();
    device.release(Button::Middle).unwrap();
    mock.clear_recorded();

    device.reapply().unwrap();
    let buttons: Vec<Vec<u8>> = mock
        .recorded_frames()
        .iter()
        .filter(|f| f.ty == FrameType::Inject)
        .map(|f| f.payload.clone())
        .collect();
    // INJECT [class=btn][id u16][action]: Left press, Side1 force-release.
    assert_eq!(buttons, vec![vec![0, 0, 0, 1], vec![0, 3, 0, 2]]);
    drop(device);
}

#[test]
fn reapply_re_emits_held_locks_but_not_released_ones() {
    use crate::{Axis, Blanket, Direction, Key};
    let mock = MockBox::new();
    let device = Device::with_mock(mock.clone());
    device.lock(Axis::X, Direction::Positive).unwrap();
    device.lock(Key::A, Direction::Both).unwrap();
    device.lock_all(Blanket::Keys, Direction::Both).unwrap();
    device.unlock(Key::A, Direction::Both).unwrap();
    mock.clear_recorded();

    device.reapply().unwrap();
    let locks: Vec<Vec<u8>> = mock
        .recorded_frames()
        .iter()
        .filter(|f| f.ty == FrameType::Lock)
        .map(|f| f.payload.clone())
        .collect();
    // Only the two still-held locks, each re-asserted at the scale it was set to; key A is gone.
    // Ordered by the desired-set key (class,id,dir): the KEY blanket (1, 0xFFFF, both) before the
    // AXIS X+ (3, 0, pos).
    assert_eq!(locks, vec![vec![1, 0xFF, 0xFF, 0, 0], vec![3, 0, 0, 1, 0]]);
    drop(device);
}

#[test]
fn reapply_re_emits_a_scale_at_its_own_value() {
    use crate::{Axis, Direction};
    let mock = MockBox::new();
    let device = Device::with_mock(mock.clone());
    device.scale(Axis::X, Direction::Against, 40).unwrap();
    device.scale(Axis::Y, Direction::With, 130).unwrap();
    mock.clear_recorded();

    device.reapply().unwrap();
    let locks: Vec<Vec<u8>> = mock
        .recorded_frames()
        .iter()
        .filter(|f| f.ty == FrameType::Lock)
        .map(|f| f.payload.clone())
        .collect();
    // A weighing comes back weighing, not blocked: re-sending these as a blanket lock would turn a
    // 40% damp into a dead axis across a reconnect the user never saw.
    assert_eq!(locks, vec![vec![3, 0, 0, 4, 40], vec![3, 1, 0, 3, 130]]);
    drop(device);
}

#[test]
fn unlocking_both_forgets_the_relative_scales_too() {
    use crate::{Axis, Direction};
    let mock = MockBox::new();
    let device = Device::with_mock(mock.clone());
    device.scale(Axis::X, Direction::Against, 40).unwrap();
    device.scale(Axis::X, Direction::Positive, 60).unwrap();
    device.unlock(Axis::X, Direction::Both).unwrap();
    mock.clear_recorded();

    device.reapply().unwrap();
    let locks: Vec<Vec<u8>> = mock
        .recorded_frames()
        .iter()
        .filter(|f| f.ty == FrameType::Lock)
        .map(|f| f.payload.clone())
        .collect();
    // Both sweeps the whole target on the box, so the shadow must sweep it too. Re-sending the 40%
    // here would restore a weighing the caller had already cleared.
    assert!(
        locks.is_empty(),
        "expected nothing re-asserted, got {locks:?}"
    );
    drop(device);
}

#[test]
fn releasing_one_sign_of_a_both_lock_is_not_undone_by_a_reapply() {
    use crate::{Axis, Direction};
    let mock = MockBox::new();
    let device = Device::with_mock(mock.clone());
    device.lock(Axis::X, Direction::Both).unwrap();
    device.unlock(Axis::X, Direction::Negative).unwrap();
    mock.clear_recorded();

    device.reapply().unwrap();
    let locks: Vec<Vec<u8>> = mock
        .recorded_frames()
        .iter()
        .filter(|f| f.ty == FrameType::Lock)
        .map(|f| f.payload.clone())
        .collect();
    // Both wrote two slots and the unlock cleared one of them, so only the positive sign is still
    // held. Re-sending the Both would re-block a direction the caller released.
    assert_eq!(locks, vec![vec![3, 0, 0, 1, 0]]);
    drop(device);
}

#[test]
fn releasing_one_button_of_a_blanket_is_not_undone_by_a_reapply() {
    use crate::{Blanket, Button, Direction};
    let mock = MockBox::new();
    let device = Device::with_mock(mock.clone());
    device.lock_all(Blanket::Buttons, Direction::Both).unwrap();
    device.unlock(Button::Left, Direction::Both).unwrap();
    mock.clear_recorded();

    device.reapply().unwrap();
    let ids: Vec<u16> = mock
        .recorded_frames()
        .iter()
        .filter(|f| f.ty == FrameType::Lock)
        .map(|f| u16::from_le_bytes([f.payload[1], f.payload[2]]))
        .collect();
    // The box has no button-blanket state: it wrote the five rows, so a release of one leaves four.
    assert_eq!(ids, vec![1, 2, 3, 4]);
    drop(device);
}

#[test]
fn a_scale_a_one_bit_class_cannot_hold_is_not_held_here_either() {
    use crate::{Button, Direction};
    let mock = MockBox::new();
    let device = Device::with_mock(mock.clone());
    device
        .scale(Button::Left, Direction::Positive, 150)
        .unwrap();
    mock.clear_recorded();

    device.reapply().unwrap();
    // 150% truncates to a pass on the box, which is an unlock, so there is nothing to hold open the
    // keepalive and nothing to re-assert.
    assert!(!mock.saw(FrameType::Lock));
    drop(device);
}

#[test]
fn reboot_emits_the_target_byte() {
    let mock = MockBox::new();
    let device = Device::with_mock(mock.clone());
    for target in [
        RebootTarget::DeviceRun,
        RebootTarget::HostRun,
        RebootTarget::DeviceDownload,
        RebootTarget::HostDownload,
    ] {
        device.reboot(target).unwrap();
    }
    let reboots: Vec<u8> = mock
        .recorded_frames()
        .iter()
        .filter(|f| f.ty == FrameType::RebootDl)
        .map(|f| f.payload[0])
        .collect();
    assert_eq!(reboots, vec![2, 3, 0, 1]);
    drop(device);
}
