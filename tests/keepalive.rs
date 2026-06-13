//! Keepalive behavior through the public API + `MockBox` (feature `mock`).
//!
//! The keepalive thread must refresh held state **only while the desired state is non-idle**, and stay
//! **silent while idle** — the latter is the no-stuck safety: idle silence lets the firmware's 1000 ms
//! auto-clear release a button on a real host crash. A regression that fired while idle would defeat
//! that and silently pass a result-only check, so this asserts the *behavior* (frames sent or not).
//! Uses the fixed [`DEFAULT_KEEPALIVE_CADENCE`](medius::DEFAULT_KEEPALIVE_CADENCE) (500 ms).
#![cfg(feature = "mock")]

use std::time::Duration;

use medius::{Button, Device, FrameType, MockBox};

/// Comfortably longer than one keepalive cadence (500 ms), so at least one tick lands in the window.
const PAST_ONE_CADENCE: Duration = Duration::from_millis(650);

#[test]
fn keepalive_fires_while_a_button_is_held() {
    let mock = MockBox::new();
    let device = Device::with_mock(mock.clone());
    device.press(Button::Left).unwrap(); // non-idle desired state
    mock.clear_recorded(); // ignore the press frame; watch only what the keepalive sends next
    std::thread::sleep(PAST_ONE_CADENCE);
    assert!(
        mock.saw(FrameType::Query),
        "keepalive must send a QUERY while a button is held (saw {} frames)",
        mock.recorded()
    );
    drop(device);
}

#[test]
fn keepalive_is_silent_while_idle() {
    let mock = MockBox::new();
    let device = Device::with_mock(mock.clone());
    // Nothing held → idle. Expect total silence across a cadence window (the no-stuck safety).
    mock.clear_recorded();
    std::thread::sleep(PAST_ONE_CADENCE);
    assert_eq!(
        mock.recorded(),
        0,
        "keepalive must stay silent while idle so the firmware auto-clear can release on a real crash"
    );
    drop(device);
}

#[test]
fn keepalive_stops_after_the_device_is_dropped() {
    let mock = MockBox::new();
    let device = Device::with_mock(mock.clone());
    device.press(Button::Left).unwrap();
    std::thread::sleep(PAST_ONE_CADENCE); // keepalive fires at least once
    drop(device); // stops + joins the keepalive thread
    mock.clear_recorded();
    std::thread::sleep(PAST_ONE_CADENCE);
    assert_eq!(
        mock.recorded(),
        0,
        "no frames may be sent after the device is dropped (keepalive thread must have stopped)"
    );
}
