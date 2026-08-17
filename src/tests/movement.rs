//! `MOVE` payload bytes and the per-command movement-riding override, pinned to the `ctrl_proto.h` wire format.

use crate::protocol::command::{move_cursor_payload, move_wheel_payload};
use crate::protocol::opcode::{MV_F_DISCARD, MV_F_FLUSH, MV_F_NOW};
use crate::types::{MoveTiming, PendingMotion};

#[test]
fn move_payload_bytes() {
    assert_eq!(move_cursor_payload(10, -3, 0), [0, 10, 0, 0xFD, 0xFF, 0]);
    assert_eq!(move_cursor_payload(0, 0, 0), [0, 0, 0, 0, 0, 0]);
    assert_eq!(
        move_cursor_payload(-32768, 32767, 0),
        [0, 0x00, 0x80, 0xFF, 0x7F, 0]
    );
    assert_eq!(move_wheel_payload(-1, 0), [1, 0xFF, 0xFF, 0]);
    assert_eq!(move_wheel_payload(300, 0), [1, 0x2C, 0x01, 0]);
}

#[test]
fn move_flag_bytes() {
    assert_eq!(MoveTiming::default(), MoveTiming::Ride);
    assert_eq!(PendingMotion::default(), PendingMotion::Keep);
    assert_eq!(move_cursor_payload(1, 1, MV_F_NOW)[5], 0x01);
    assert_eq!(move_cursor_payload(1, 1, MV_F_FLUSH)[5], 0x02);
    assert_eq!(move_cursor_payload(1, 1, MV_F_DISCARD)[5], 0x04);
    assert_eq!(move_cursor_payload(1, 1, MV_F_NOW | MV_F_FLUSH)[5], 0x03);
    assert_eq!(move_wheel_payload(1, MV_F_NOW)[3], 0x01);
}

#[cfg(feature = "mock")]
#[test]
fn move_verbs_send_the_flags_they_promise() {
    use crate::protocol::FrameType;
    use crate::types::Motion;
    use crate::{Device, MockBox};

    let mock = MockBox::new();
    let device = Device::with_mock(mock.clone());
    mock.clear_recorded();

    device.move_rel(7, -2).unwrap();
    device.move_rel_now(7, -2).unwrap();
    device.wheel(3).unwrap();
    device.wheel_now(3).unwrap();
    device.flush_motion().unwrap();
    device.discard_motion().unwrap();
    device
        .move_axis(
            Motion::Cursor { dx: 5, dy: 5 },
            MoveTiming::Now,
            PendingMotion::Flush,
        )
        .unwrap();

    device
        .move_axis(Motion::Wheel(4), MoveTiming::Ride, PendingMotion::Flush)
        .unwrap();
    device
        .move_axis(Motion::Wheel(0), MoveTiming::Now, PendingMotion::Discard)
        .unwrap();

    let sent: Vec<Vec<u8>> = mock
        .recorded_frames()
        .into_iter()
        .filter(|f| f.ty == FrameType::Move)
        .map(|f| f.payload)
        .collect();
    assert_eq!(
        sent,
        vec![
            vec![0, 7, 0, 0xFE, 0xFF, 0x00], // move_rel: rides, hoard kept
            vec![0, 7, 0, 0xFE, 0xFF, 0x01], // move_rel_now: NOW
            vec![1, 3, 0, 0x00],             // wheel: rides
            vec![1, 3, 0, 0x01],             // wheel_now: NOW
            vec![0, 0, 0, 0, 0, 0x02],       // flush_motion: zero delta, FLUSH
            vec![0, 0, 0, 0, 0, 0x04],       // discard_motion: zero delta, DISCARD
            vec![0, 5, 0, 5, 0, 0x03],       // move_axis: NOW | FLUSH
            vec![1, 4, 0, 0x02],             // the wheel entry point takes the pending flags too
            vec![1, 0, 0, 0x05],             // ...and NOW | DISCARD
        ]
    );
}
