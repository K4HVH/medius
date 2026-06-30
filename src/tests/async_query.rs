#![cfg(all(feature = "async", feature = "mock"))]

use futures::executor::block_on;

use crate::{Device, Error, LogLevel, MockBox};

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
