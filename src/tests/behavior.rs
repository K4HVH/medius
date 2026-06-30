#![cfg(feature = "mock")]

use std::time::{Duration, Instant};

use crate::{Button, Device, Error, FrameType, Health, LogLevel, MockBox, RebootTarget, Version};

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
    device.soft_release(Button::Middle).unwrap();
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
    use crate::{Blanket, Key, LockDirection, LockTarget};
    let mock = MockBox::new();
    let device = Device::with_mock(mock.clone());
    device.lock(LockTarget::X, LockDirection::Positive).unwrap();
    device.lock_key(Key::A, LockDirection::Both).unwrap();
    device.lock_all(Blanket::Keys).unwrap();
    device.unlock_key(Key::A, LockDirection::Both).unwrap(); // released -> must not reappear
    mock.clear_recorded();

    device.reapply().unwrap();
    let locks: Vec<Vec<u8>> = mock
        .recorded_frames()
        .iter()
        .filter(|f| f.ty == FrameType::Lock)
        .map(|f| f.payload.clone())
        .collect();
    // Only the two still-held locks, each re-asserted with state=1; key A is gone.
    assert_eq!(locks, vec![vec![0, 0, 0, 1, 1], vec![3, 0, 0, 0, 1]]);
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
