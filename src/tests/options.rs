//! OPTION command + QUERY(OPTIONS) (§3.10 / §4.14): payload bytes, decoding through `parse_resp`, the
//! command frames, and the query roundtrips for both options (imperfect-clone, movement riding). Bytes
//! are pinned to the firmware wire format in `ctrl_proto.h`.

use std::time::Duration;

#[cfg(feature = "mock")]
use crate::protocol::FrameType;
use crate::protocol::command::{imperfect_payload, move_ride_payload};
use crate::protocol::{Resp, parse_resp};
use crate::types::ImperfectStatus;

#[test]
fn option_payload_bytes() {
    // OPTION(IMPERFECT): [id=0][allow]
    assert_eq!(imperfect_payload(true), [0, 1]);
    assert_eq!(imperfect_payload(false), [0, 0]);
    // OPTION(MOVE_RIDE): [id=1][timeout u16 LE ms]
    assert_eq!(move_ride_payload(5), [1, 5, 0]);
    assert_eq!(move_ride_payload(0), [1, 0, 0]);
    assert_eq!(move_ride_payload(1000), [1, 0xE8, 0x03]);
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
fn unknown_option_id_and_missing_id_are_none() {
    assert!(parse_resp(&[9, 0xFF, 0, 0]).is_none()); // unknown option id
    assert!(parse_resp(&[9]).is_none()); // OPTIONS selector with no id
}

#[cfg(feature = "mock")]
#[test]
fn allow_imperfect_clones_sends_an_option_frame() {
    use crate::{Device, MockBox};
    let mock = MockBox::new();
    let device = Device::with_mock(mock.clone());
    device.allow_imperfect_clones(true).unwrap();
    let frame = mock
        .recorded_frames()
        .into_iter()
        .find(|f| f.ty == FrameType::Option)
        .expect("an OPTION frame was recorded");
    assert_eq!(frame.payload, vec![0, 1]); // [id=imperfect][allow=1]
}

#[cfg(feature = "mock")]
#[test]
fn set_movement_riding_sends_an_option_frame() {
    use crate::{Device, MockBox};
    let mock = MockBox::new();
    let device = Device::with_mock(mock.clone());
    device
        .set_movement_riding(Some(Duration::from_millis(5)))
        .unwrap();
    device.set_movement_riding(None).unwrap();
    let frames: Vec<_> = mock
        .recorded_frames()
        .into_iter()
        .filter(|f| f.ty == FrameType::Option)
        .collect();
    assert_eq!(frames[0].payload, vec![1, 5, 0]); // [id=move_ride][5ms LE]
    assert_eq!(frames[1].payload, vec![1, 0, 0]); // off
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

#[cfg(feature = "mock")]
#[test]
fn imperfect_query_roundtrips_the_status() {
    use crate::{Device, MockBox};
    let status = ImperfectStatus {
        allowed: true,
        over_capacity: true,
        clone_imperfect: true,
    };
    let mock = MockBox::new().with_imperfect_status(status);
    let device = Device::with_mock(mock);
    assert_eq!(device.query_imperfect().unwrap(), status);
}

#[cfg(feature = "mock")]
#[test]
fn move_ride_query_roundtrips_the_window() {
    use crate::{Device, MockBox};
    let mock = MockBox::new().with_movement_riding(Some(Duration::from_millis(5)));
    let device = Device::with_mock(mock);
    assert_eq!(
        device.query_movement_riding().unwrap(),
        Some(Duration::from_millis(5))
    );
    // default is off
    let off = Device::with_mock(MockBox::new());
    assert_eq!(off.query_movement_riding().unwrap(), None);
}

#[cfg(all(feature = "async", feature = "mock"))]
#[test]
fn async_option_queries_roundtrip() {
    use crate::{Device, MockBox};
    use futures::executor::block_on;
    let status = ImperfectStatus {
        allowed: true,
        over_capacity: true,
        clone_imperfect: true,
    };
    let mock = MockBox::new()
        .with_imperfect_status(status)
        .with_movement_riding(Some(Duration::from_millis(8)));
    let device = Device::with_mock(mock).into_async();
    assert_eq!(block_on(device.query_imperfect()).unwrap(), status);
    assert_eq!(
        block_on(device.query_movement_riding()).unwrap(),
        Some(Duration::from_millis(8))
    );
}
