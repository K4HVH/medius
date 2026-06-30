//! OPTION command + QUERY(OPTIONS) (§3.10 / §4.14): payload bytes, decoding through `parse_resp`, the
//! command frames, and the query roundtrips for both options (imperfect-clone, movement riding). Bytes
//! are pinned to the firmware wire format in `ctrl_proto.h`.

use std::time::Duration;

#[cfg(feature = "mock")]
use crate::protocol::FrameType;
use crate::protocol::command::{emit_pace_payload, imperfect_payload, move_ride_payload};
use crate::protocol::{Resp, parse_resp};
use crate::types::{EmitPace, EmitPaceStatus, ImperfectStatus};

#[test]
fn option_payload_bytes() {
    // OPTION(IMPERFECT): [id=0][allow]
    assert_eq!(imperfect_payload(true), [0, 1]);
    assert_eq!(imperfect_payload(false), [0, 0]);
    // OPTION(MOVE_RIDE): [id=1][timeout u16 LE ms]
    assert_eq!(move_ride_payload(5), [1, 5, 0]);
    assert_eq!(move_ride_payload(0), [1, 0, 0]);
    assert_eq!(move_ride_payload(1000), [1, 0xE8, 0x03]);
    // OPTION(EMIT): [id=2][mode][rate_hz u16 LE]
    assert_eq!(emit_pace_payload(0, 0), [2, 0, 0, 0]);
    assert_eq!(emit_pace_payload(1, 0), [2, 1, 0, 0]);
    assert_eq!(emit_pace_payload(2, 1000), [2, 2, 0xE8, 0x03]);
}

#[test]
fn decode_imperfect_through_parse_resp() {
    // RESP(OPTIONS, IMPERFECT): [what=9][id=0][allowed][over_capacity][clone_imperfect]
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
    assert!(parse_resp(&[9, 0, 0, 0]).is_none()); // needs 5 (what + id + 3 status bytes)
}

#[test]
fn decode_move_ride_through_parse_resp() {
    // RESP(OPTIONS, MOVE_RIDE): [what=9][id=1][timeout u16 LE]
    let Some(Resp::MovementRiding(w)) = parse_resp(&[9, 1, 5, 0]) else {
        panic!("expected MovementRiding");
    };
    assert_eq!(w, Some(Duration::from_millis(5)));
    let Some(Resp::MovementRiding(off)) = parse_resp(&[9, 1, 0, 0]) else {
        panic!("expected MovementRiding");
    };
    assert_eq!(off, None); // 0 ms = off
    assert!(parse_resp(&[9, 1, 0]).is_none()); // needs 4 (what + id + u16)
}

#[test]
fn decode_emit_pace_through_parse_resp() {
    // RESP(OPTIONS, EMIT): [what=9][id=2][mode][fixed_hz u16 LE][resolved_hz u16 LE]
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
    assert_eq!(learned, EmitPaceStatus::default()); // mode Learned, resolved 0
    assert!(parse_resp(&[9, 2, 0, 0, 0, 0]).is_none()); // needs 7 (what + id + mode + 2×u16)
    assert!(parse_resp(&[9, 2, 3, 0, 0, 0, 0]).is_none()); // unknown mode byte
}

#[test]
fn unknown_option_id_and_missing_id_are_none() {
    assert!(parse_resp(&[9, 0xFF, 0, 0]).is_none()); // unknown option id
    assert!(parse_resp(&[9]).is_none()); // OPTIONS selector with no id
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
    assert_eq!(frame.payload, vec![1, 1, 0]); // clamped to 1 ms (on, not off)
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
    assert_eq!(frame.payload, vec![1, 0xFF, 0xFF]); // [id=move_ride][u16::MAX LE]
}
