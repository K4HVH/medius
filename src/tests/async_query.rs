#![cfg(all(feature = "async", feature = "mock"))]

use futures::executor::block_on;

use crate::{Action, Button, Device, Error, FrameType, LogLevel, MockBox, Version};

#[test]
fn async_query_returns_the_configured_version() {
    let mock = MockBox::new().with_version(Version {
        proto_ver: 2,
        fw_major: 1,
        fw_minor: 2,
        fw_patch: 3,
    });
    let device = Device::with_mock(mock).into_async();
    let v = block_on(device.query_version()).unwrap();
    assert_eq!((v.fw_major, v.fw_minor, v.fw_patch), (1, 2, 3));
}

#[test]
fn async_query_times_out_on_a_silent_box() {
    let device = Device::with_mock(MockBox::new().silent()).into_async();
    let err = block_on(device.query_version()).unwrap_err();
    assert!(matches!(err, Error::QueryTimeout), "got {err:?}");
}

#[test]
fn async_logs_recv_async_yields_pushed_lines() {
    let mock = MockBox::new();
    let device = Device::with_mock(mock.clone()).into_async();
    let logs = device.logs();
    mock.push_log(LogLevel::Warn, "overheating");
    let line = block_on(logs.recv_async()).unwrap();
    assert_eq!(
        (line.level, line.text.as_str()),
        (LogLevel::Warn, "overheating")
    );
}

#[test]
fn async_counters_reads_tx_from_the_shared_link() {
    let device = Device::with_mock(MockBox::new()).into_async();
    let before = device.counters().frames_tx;
    device.inject(Button::Left, Action::Press).unwrap();
    assert!(
        device.counters().frames_tx > before,
        "the async counters() view must see the sent frame on the shared link"
    );
}

#[test]
fn async_inject_routes_to_an_inject_frame() {
    let mock = MockBox::new();
    let device = Device::with_mock(mock.clone()).into_async();
    device.inject(Button::Left, Action::Press).unwrap();
    assert!(
        mock.recorded_frames()
            .iter()
            .any(|f| f.ty == FrameType::Inject && f.payload == vec![0, 0, 0, 1]),
        "inject(Left, Press) must emit INJECT [class=btn, id=Left, action=press]"
    );
}

#[test]
fn async_reapply_reemits_held_overrides_over_the_shared_link() {
    let mock = MockBox::new();
    let device = Device::with_mock(mock.clone()).into_async();
    device.inject(Button::Left, Action::Press).unwrap();
    mock.clear_recorded();
    device.reapply().unwrap();
    assert!(
        mock.recorded_frames()
            .iter()
            .any(|f| f.ty == FrameType::Inject && f.payload == vec![0, 0, 0, 1]),
        "reapply must re-emit the held Left-press from the shared desired store"
    );
}
