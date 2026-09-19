#![cfg(feature = "mock")]

use std::time::Duration;

use crate::{
    Button, CatchFilter, ClipAction, ClipBuilder, ClipPacketTrigger, ClipTrigger, Device,
    Direction, Edge, FrameType, MockBox, TrafficClass,
};

const PAST_ONE_CADENCE: Duration = Duration::from_millis(650);

#[test]
fn keepalive_reasserts_catch_while_subscribed() {
    let mock = MockBox::new();
    let device = Device::with_mock(mock.clone());
    let _stream = device.catch_events([CatchFilter::everything()]).unwrap();
    mock.clear_recorded();
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
    device.press(Button::LEFT).unwrap();
    mock.clear_recorded();
    std::thread::sleep(PAST_ONE_CADENCE);
    assert!(
        mock.saw(FrameType::Query),
        "keepalive must send a QUERY while a button is held (saw {} frames)",
        mock.recorded()
    );
    drop(device);
}

// A second of control silence clears the clip and its triggers on the box. A retained clip armed on a
// trigger is exactly the case where the host says nothing more: it waits for the button.
#[test]
fn keepalive_fires_while_a_clip_waits_on_a_trigger() {
    let mock = MockBox::new();
    let device = Device::with_mock(mock.clone());
    let clip = device.clip();
    clip.bind(ClipTrigger::new(
        Button::SIDE1,
        Edge::Press,
        ClipAction::Start,
    ))
    .unwrap();
    mock.clear_recorded();
    std::thread::sleep(PAST_ONE_CADENCE);
    assert!(
        mock.saw(FrameType::Query),
        "a bound trigger must hold the keepalive open (saw {} frames)",
        mock.recorded()
    );

    clip.clear_triggers().unwrap();
    let mut b = ClipBuilder::new();
    b.move_by(1, 0);
    clip.append(&b).unwrap();
    mock.clear_recorded();
    std::thread::sleep(PAST_ONE_CADENCE);
    assert!(mock.saw(FrameType::Query), "and so must a loaded clip");

    // A manual reapply runs on a live link, where the box still holds the clip.
    device.reapply().unwrap();
    mock.clear_recorded();
    std::thread::sleep(PAST_ONE_CADENCE);
    assert!(
        mock.saw(FrameType::Query),
        "a reapply must leave the loaded clip held"
    );

    clip.clear().unwrap();
    mock.clear_recorded();
    std::thread::sleep(PAST_ONE_CADENCE);
    assert!(
        !mock.saw(FrameType::Query),
        "with the clip cleared and nothing bound the keepalive idles"
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
    device.press(Button::LEFT).unwrap();
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

// Each clip call records what it sent, and the keepalive runs exactly while any of it stands.
#[test]
fn keepalive_follows_a_clip_setting_and_a_trigger_binding() {
    let mock = MockBox::new();
    let device = Device::with_mock(mock.clone());
    let clip = device.clip();
    let fires = || {
        mock.clear_recorded();
        std::thread::sleep(PAST_ONE_CADENCE);
        mock.saw(FrameType::Query)
    };

    clip.append(&ClipBuilder::new()).unwrap();
    assert!(!fires(), "an append of nothing loads nothing");

    clip.set_ride(true).unwrap();
    assert!(fires(), "a setting off its default is held");
    clip.set_ride(false).unwrap();
    assert!(!fires(), "and back on it is not");

    clip.bind(ClipTrigger::new(
        Button::SIDE1,
        Edge::Press,
        ClipAction::Start,
    ))
    .unwrap();
    assert!(fires(), "a bound trigger is held");
    clip.unbind(Button::SIDE1, Edge::Press).unwrap();
    assert!(!fires(), "and unbound it is not");

    // A packet trigger waits on the device the way an input trigger waits on a button.
    let packet = ClipPacketTrigger::new(TrafficClass::HidIn, 2, Direction::IN, ClipAction::Start)
        .matching([0x07, 0x20], [0xFF, 0x20]);
    clip.bind_packet(&packet).unwrap();
    assert!(fires(), "a bound packet trigger is held");
    clip.unbind_packet(&packet).unwrap();
    assert!(!fires(), "and unbound it is not");
    drop(device);
}
