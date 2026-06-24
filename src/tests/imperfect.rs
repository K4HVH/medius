//! IMPERFECT command + QUERY (§3.10 / §4.14): payload bytes, `ImperfectStatus` decoding through
//! `parse_resp`, the command frame, and the query roundtrip. Bytes are pinned to the firmware wire
//! format in `ctrl_proto.h`.

#[cfg(feature = "mock")]
use crate::protocol::FrameType;
use crate::protocol::command::imperfect_payload;
use crate::protocol::{Resp, parse_resp};
use crate::types::ImperfectStatus;

#[test]
fn imperfect_payload_bytes() {
    assert_eq!(imperfect_payload(true), [1]);
    assert_eq!(imperfect_payload(false), [0]);
}

#[test]
fn decode_imperfect_through_parse_resp() {
    // [what=9][allowed][over_capacity][clone_imperfect]
    let Some(Resp::Imperfect(i)) = parse_resp(&[9, 1, 1, 1]) else {
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
    let Some(Resp::Imperfect(none)) = parse_resp(&[9, 0, 0, 0]) else {
        panic!("expected Imperfect");
    };
    assert_eq!(none, ImperfectStatus::default());
    assert!(parse_resp(&[9, 0, 0]).is_none()); // needs 4
}

#[cfg(feature = "mock")]
#[test]
fn set_imperfect_allowed_sends_an_imperfect_frame() {
    use crate::{Device, MockBox};
    let mock = MockBox::new();
    let device = Device::with_mock(mock.clone());
    device.set_imperfect_allowed(true).unwrap();
    let frame = mock
        .recorded_frames()
        .into_iter()
        .find(|f| f.ty == FrameType::Imperfect)
        .expect("an IMPERFECT frame was recorded");
    assert_eq!(frame.payload, vec![1]);
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
    assert_eq!(device.imperfect().unwrap(), status);
}

#[cfg(all(feature = "async", feature = "mock"))]
#[test]
fn async_imperfect_query_roundtrips_the_status() {
    use crate::{Device, MockBox};
    use futures::executor::block_on;
    let status = ImperfectStatus {
        allowed: true,
        over_capacity: true,
        clone_imperfect: true,
    };
    let mock = MockBox::new().with_imperfect_status(status);
    let device = Device::with_mock(mock).into_async();
    assert_eq!(block_on(device.imperfect()).unwrap(), status);
}
