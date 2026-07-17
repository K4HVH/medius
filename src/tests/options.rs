//! OPTION command and QUERY(OPTIONS): payload bytes, `parse_resp` decoding, and query roundtrips, pinned to the `ctrl_proto.h` wire format.

use std::time::Duration;

#[cfg(feature = "mock")]
use crate::protocol::FrameType;
use crate::protocol::command::{
    emit_pace_payload, imperfect_payload, move_ride_payload, name_payload,
};
use crate::protocol::{Resp, parse_resp};
use crate::types::{EmitPace, EmitPaceStatus, ImperfectStatus};

#[test]
fn option_payload_bytes() {
    assert_eq!(imperfect_payload(true), [0, 1]);
    assert_eq!(imperfect_payload(false), [0, 0]);
    assert_eq!(move_ride_payload(5), [1, 5, 0]);
    assert_eq!(move_ride_payload(0), [1, 0, 0]);
    assert_eq!(move_ride_payload(1000), [1, 0xE8, 0x03]);
    assert_eq!(emit_pace_payload(0, 0), [2, 0, 0, 0]);
    assert_eq!(emit_pace_payload(1, 0), [2, 1, 0, 0]);
    assert_eq!(emit_pace_payload(2, 1000), [2, 2, 0xE8, 0x03]);
    assert_eq!(name_payload("AB"), vec![3, b'A', b'B']);
    assert_eq!(name_payload(""), vec![3]);
}

#[test]
fn decode_imperfect_through_parse_resp() {
    let Some(Resp::Imperfect(i)) = parse_resp(&[9, 0, 1, 1, 1]) else {
        panic!("expected Imperfect");
    };
    assert_eq!(
        i,
        ImperfectStatus {
            allowed: true,
            over_capacity: true,
            clone_imperfect: true
        }
    );
    let Some(Resp::Imperfect(none)) = parse_resp(&[9, 0, 0, 0, 0]) else {
        panic!("expected Imperfect");
    };
    assert_eq!(none, ImperfectStatus::default());
    assert!(parse_resp(&[9, 0, 0, 0]).is_none());
}

#[test]
fn decode_move_ride_through_parse_resp() {
    let Some(Resp::MovementRiding(w)) = parse_resp(&[9, 1, 5, 0]) else {
        panic!("expected MovementRiding");
    };
    assert_eq!(w, Some(Duration::from_millis(5)));
    let Some(Resp::MovementRiding(off)) = parse_resp(&[9, 1, 0, 0]) else {
        panic!("expected MovementRiding");
    };
    assert_eq!(off, None);
    assert!(parse_resp(&[9, 1, 0]).is_none());
}

#[test]
fn decode_emit_pace_through_parse_resp() {
    let Some(Resp::EmitPace(s)) = parse_resp(&[9, 2, 2, 0xF4, 0x01, 0xF4, 0x01]) else {
        panic!("expected EmitPace");
    };
    assert_eq!(
        s,
        EmitPaceStatus {
            mode: EmitPace::Fixed(500),
            resolved_hz: 500
        }
    );
    let Some(Resp::EmitPace(learned)) = parse_resp(&[9, 2, 0, 0, 0, 0, 0]) else {
        panic!("expected EmitPace");
    };
    assert_eq!(learned, EmitPaceStatus::default());
    assert!(parse_resp(&[9, 2, 0, 0, 0, 0]).is_none());
    assert!(parse_resp(&[9, 2, 3, 0, 0, 0, 0]).is_none());
}

#[cfg(feature = "mock")]
#[test]
fn mock_emit_pace_matches_firmware_snap() {
    use crate::{Device, EmitPace, EmitPaceStatus, MockBox};
    // The mock models firmware pacing: Fixed(400) snaps to 1000/3 = 333 Hz on the 1 ms frame clock
    // (not raw 400) and Fixed(2000) clamps to 1 kHz; a naive echo would diverge from hardware.
    let mock = MockBox::new().with_emit_pace(EmitPace::Fixed(400));
    let device = Device::with_mock(mock.clone());
    assert_eq!(
        device.query_emit_pace().unwrap(),
        EmitPaceStatus {
            mode: EmitPace::Fixed(400),
            resolved_hz: 333
        }
    );
    mock.set_emit_pace(EmitPace::Fixed(2000));
    assert_eq!(
        device.query_emit_pace().unwrap(),
        EmitPaceStatus {
            mode: EmitPace::Fixed(1000),
            resolved_hz: 1000
        }
    );
    mock.set_emit_pace(EmitPace::Learned);
    assert_eq!(
        device.query_emit_pace().unwrap(),
        EmitPaceStatus {
            mode: EmitPace::Learned,
            resolved_hz: 0
        }
    );
}

#[test]
fn unknown_option_id_and_missing_id_are_none() {
    assert!(parse_resp(&[9, 0xFF, 0, 0]).is_none());
    assert!(parse_resp(&[9]).is_none());
}

#[cfg(feature = "mock")]
#[test]
fn set_movement_riding_rounds_sub_ms_up_to_on() {
    use crate::{Device, MockBox};
    let mock = MockBox::new();
    let device = Device::with_mock(mock.clone());
    // a non-zero Some window under 1 ms must not silently round down to off
    device
        .set_movement_riding(Some(Duration::from_micros(500)))
        .unwrap();
    let frame = mock
        .recorded_frames()
        .into_iter()
        .find(|f| f.ty == FrameType::Option)
        .unwrap();
    assert_eq!(frame.payload, vec![1, 1, 0]);
}

#[cfg(feature = "mock")]
#[test]
fn set_movement_riding_saturates_at_u16_max() {
    use crate::{Device, MockBox};
    let mock = MockBox::new();
    let device = Device::with_mock(mock.clone());
    // a window past u16::MAX ms must saturate, not wrap
    device
        .set_movement_riding(Some(Duration::from_millis(100_000)))
        .unwrap();
    let frame = mock
        .recorded_frames()
        .into_iter()
        .find(|f| f.ty == FrameType::Option)
        .unwrap();
    assert_eq!(frame.payload, vec![1, 0xFF, 0xFF]);
}

#[cfg(feature = "mock")]
#[test]
fn set_name_sends_option_frame_and_clear_is_empty() {
    use crate::{Device, MockBox};
    let mock = MockBox::new();
    let device = Device::with_mock(mock.clone());
    device.set_name("Left PC").unwrap();
    let frame = mock
        .recorded_frames()
        .into_iter()
        .find(|f| f.ty == FrameType::Option)
        .unwrap();
    assert_eq!(frame.payload, b"\x03Left PC".to_vec());
    device.clear_name().unwrap();
    let cleared = mock
        .recorded_frames()
        .into_iter()
        .rfind(|f| f.ty == FrameType::Option)
        .unwrap();
    assert_eq!(cleared.payload, vec![3]);
}

#[cfg(feature = "mock")]
#[test]
fn set_name_sends_the_value_raw_for_the_box_to_sanitize() {
    use crate::{Device, MockBox};
    // set_name does no host-side validation: it sends the string as-is and the box sanitizes, so a
    // control byte rides through on the wire (the box drops it) rather than raising a host error.
    let mock = MockBox::new();
    let device = Device::with_mock(mock.clone());
    device.set_name("A\tB").unwrap();
    let frame = mock
        .recorded_frames()
        .into_iter()
        .find(|f| f.ty == FrameType::Option)
        .unwrap();
    assert_eq!(frame.payload, vec![3, b'A', b'\t', b'B']);
}
