//! CATCH command (§3.9): payload bytes, the `CatchMask` / `InputReport` / `CatchState` types, the
//! `EVENT` decode through `parse_resp` and the reader, the HEALTH `catch_on` bit, and the
//! `EventStream` lifecycle. Bytes are pinned to the firmware wire format in `ctrl_proto.h`.

#[cfg(feature = "mock")]
use crate::protocol::FrameType;
use crate::protocol::command::catch_payload;
use crate::protocol::opcode::{CATCH_BUTTONS, CATCH_MOTION, CATCH_WHEEL};
use crate::protocol::{Resp, parse_resp};
use crate::types::{Button, CatchMask, CatchState, Health, InputReport};

#[test]
fn catch_payload_bytes() {
    assert_eq!(catch_payload(CatchMask::all().bits()), [0x07]);
    assert_eq!(catch_payload(0), [0x00]);
}

#[test]
fn catch_mask_class_bits_and_ops() {
    assert_eq!(CatchMask::MOTION.bits(), CATCH_MOTION);
    assert_eq!(CatchMask::WHEEL.bits(), CATCH_WHEEL);
    assert_eq!(CatchMask::BUTTONS.bits(), CATCH_BUTTONS);
    assert_eq!(CatchMask::all().bits(), 0x07);
    assert!(CatchMask::empty().is_empty());

    let m = CatchMask::MOTION | CatchMask::BUTTONS;
    assert_eq!(m.bits(), 0x05);
    assert!(m.contains(CatchMask::MOTION));
    assert!(m.contains(CatchMask::BUTTONS));
    assert!(!m.contains(CatchMask::WHEEL));
    assert_eq!(
        CatchMask::MOTION | CatchMask::WHEEL | CatchMask::BUTTONS,
        CatchMask::all()
    );

    // Bits outside the valid mask are dropped.
    assert_eq!(CatchMask::from_bits_truncate(0xFF), CatchMask::all());
    assert_eq!(CatchMask::from_bits_truncate(0xF0), CatchMask::empty());
}

#[test]
fn input_report_decodes_snapshot() {
    // buttons left+side1 (0x09), dx=+300, dy=-50, wheel=-1
    let r = InputReport::from_payload(&[0x09, 0x2C, 0x01, 0xCE, 0xFF, 0xFF, 0xFF]).unwrap();
    assert_eq!(r.buttons, 0x09);
    assert_eq!(r.dx, 300);
    assert_eq!(r.dy, -50);
    assert_eq!(r.wheel, -1);
    assert!(r.is_pressed(Button::Left));
    assert!(r.is_pressed(Button::Side1));
    assert!(!r.is_pressed(Button::Right));
    assert!(!r.is_pressed(Button::Side2));
}

#[test]
fn input_report_truncated_is_none() {
    assert!(InputReport::from_payload(&[0, 0, 0, 0, 0, 0]).is_none()); // needs 7
}

#[test]
fn catch_state_decodes_mask_and_drops() {
    // what=7, mask=BUTTONS (0x04), dropped=0x01020304 (LE)
    let c = CatchState::from_payload(&[7, 0x04, 0x04, 0x03, 0x02, 0x01]).unwrap();
    assert_eq!(c.mask, CatchMask::BUTTONS);
    assert_eq!(c.dropped, 0x01020304);
    assert!(CatchState::from_payload(&[7, 0, 0, 0, 0]).is_none()); // needs 6
}

#[test]
fn decode_catch_through_parse_resp() {
    let Some(Resp::Catch(c)) = parse_resp(&[7, 0x07, 0, 0, 0, 0]) else {
        panic!("expected Catch");
    };
    assert_eq!(c.mask, CatchMask::all());
    assert_eq!(c.dropped, 0);
}

#[test]
fn health_catch_on_bit_roundtrips() {
    let h = Health::from_flags(0x40);
    assert!(h.catch_on);
    assert!(!h.lock_on && !h.link_up);
    assert_eq!(h.to_flags(), 0x40);
    // survives a full round-trip with every defined bit set
    assert_eq!(Health::from_flags(0x7F).to_flags(), 0x7F);
}

#[cfg(feature = "mock")]
#[test]
fn catch_events_sends_a_catch_frame_with_the_mask() {
    use crate::{CatchMask, Device, MockBox};
    let mock = MockBox::new();
    let device = Device::with_mock(mock.clone());
    let _stream = device
        .catch_events(CatchMask::MOTION | CatchMask::BUTTONS)
        .unwrap();
    let frames = mock.recorded_frames();
    let catch = frames
        .iter()
        .find(|f| f.ty == FrameType::Catch)
        .expect("a CATCH frame was recorded");
    assert_eq!(catch.payload, vec![0x05]);
}

#[cfg(feature = "mock")]
#[test]
fn dropping_the_stream_unsubscribes() {
    use crate::{CatchMask, Device, MockBox};
    let mock = MockBox::new();
    let device = Device::with_mock(mock.clone());
    {
        let _stream = device.catch_events(CatchMask::all()).unwrap();
    } // stream dropped here -> CATCH(0)
    let catch_frames: Vec<_> = mock
        .recorded_frames()
        .into_iter()
        .filter(|f| f.ty == FrameType::Catch)
        .collect();
    assert_eq!(catch_frames.first().unwrap().payload, vec![0x07]); // subscribe
    assert_eq!(catch_frames.last().unwrap().payload, vec![0x00]); // unsubscribe
}

#[cfg(feature = "mock")]
#[test]
fn pushed_event_arrives_on_the_stream() {
    use crate::{Button, CatchMask, Device, InputReport, MockBox};
    use std::time::Duration;
    let mock = MockBox::new();
    let device = Device::with_mock(mock.clone());
    let stream = device.catch_events(CatchMask::all()).unwrap();
    mock.push_event(
        0,
        InputReport {
            buttons: 0x08,
            dx: 5,
            dy: -7,
            wheel: 1,
        },
    );
    let r = stream
        .recv_timeout(Duration::from_secs(1))
        .expect("event delivered");
    assert_eq!((r.dx, r.dy, r.wheel), (5, -7, 1));
    assert!(r.is_pressed(Button::Side1));
}

#[cfg(feature = "mock")]
#[test]
fn query_catch_roundtrips_mask_and_drops() {
    use crate::{CatchMask, CatchState, Device, MockBox};
    // wheel + buttons (0x06), 5 box-side drops
    let catch = CatchState::from_payload(&[7, 0x06, 0x05, 0x00, 0x00, 0x00]).unwrap();
    let mock = MockBox::new().with_catch_state(catch);
    let device = Device::with_mock(mock);
    let got = device.query_catch().unwrap();
    assert_eq!(got.mask, CatchMask::WHEEL | CatchMask::BUTTONS);
    assert_eq!(got.dropped, 5);
}
