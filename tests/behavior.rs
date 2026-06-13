//! End-to-end library behavior through the **public** API + `MockBox` (feature `mock`).
//!
//! These exercise the device stack exactly as a downstream consumer would — connect/query/command,
//! the log channel, the handshake path, and thread lifecycle — against the scriptable fake box.
#![cfg(feature = "mock")]

use std::time::{Duration, Instant};

use medius::{Button, Device, Error, FrameType, Health, LogLevel, MockBox, Version};

#[test]
fn query_returns_configured_values_and_records_commands() {
    let mock = MockBox::new()
        .with_version(Version {
            proto_ver: 1,
            fw_major: 5,
            fw_minor: 6,
            fw_patch: 7,
        })
        .with_health(Health::from_flags(0x0F));
    let device = Device::with_mock(mock.clone());

    let v = device.query_version().unwrap();
    assert_eq!((v.fw_major, v.fw_minor, v.fw_patch), (5, 6, 7));
    let h = device.query_health().unwrap();
    assert!(h.link_up && h.mouse_attached && h.clone_configured && h.injection_active);

    device.press(Button::Left).unwrap();
    let frames = mock.recorded_frames();
    let button = frames
        .iter()
        .find(|f| f.ty == FrameType::Button)
        .expect("press recorded");
    assert_eq!(button.payload, vec![0, 1]); // id=Left(0), action=Press(1)
    assert!(mock.saw(FrameType::Button));
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
    mock.set_health(Health::from_flags(0x02)); // mouse_attached bit
    assert!(device.query_health().unwrap().mouse_attached);
}

#[test]
fn handshake_accepts_matching_proto_ver() {
    let device = Device::open_mock(MockBox::new()).expect("default proto_ver matches");
    assert_eq!(device.query_version().unwrap().proto_ver, 1);
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
    // Reader still alive on the surviving clone: a pushed log routes through.
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
