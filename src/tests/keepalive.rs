#![cfg(feature = "mock")]

use std::time::Duration;

use crate::{Button, CatchMask, Device, FrameType, MockBox};

const PAST_ONE_CADENCE: Duration = Duration::from_millis(650);

#[test]
fn keepalive_reasserts_catch_while_subscribed() {
    let mock = MockBox::new();
    let device = Device::with_mock(mock.clone());
    let _stream = device.catch_events(CatchMask::all()).unwrap();
    mock.clear_recorded(); // ignore the initial subscribe frame
    std::thread::sleep(PAST_ONE_CADENCE);
    assert!(
        mock.saw(FrameType::Catch),
        "keepalive must re-send CATCH while subscribed (restores the mask after a device-side clear, \
         and feeds the silence timer); saw {} frames",
        mock.recorded()
    );
    drop(device);
}

#[test]
fn keepalive_fires_while_a_button_is_held() {
    let mock = MockBox::new();
    let device = Device::with_mock(mock.clone());
    device.press(Button::Left).unwrap();
    mock.clear_recorded();
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
    std::thread::sleep(PAST_ONE_CADENCE);
    drop(device);
    mock.clear_recorded();
    std::thread::sleep(PAST_ONE_CADENCE);
    assert_eq!(
        mock.recorded(),
        0,
        "no frames may be sent after the device is dropped (keepalive thread must have stopped)"
    );
}
