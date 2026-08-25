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

#[test]
fn async_movement_verbs_send_the_same_frames_as_the_sync_ones() {
    use crate::protocol::FrameType;
    use crate::{Motion, MoveTiming, PendingMotion};

    let moves = |f: &dyn Fn(&crate::AsyncDevice)| -> Vec<Vec<u8>> {
        let mock = MockBox::new();
        let device = Device::with_mock(mock.clone()).into_async();
        f(&device);
        mock.recorded_frames()
            .into_iter()
            .filter(|fr| fr.ty == FrameType::Move)
            .map(|fr| fr.payload)
            .collect()
    };
    // The async surface delegates, so what is worth pinning is that each verb still reaches the wire
    // with its own flags rather than another verb's.
    assert_eq!(
        moves(&|d| {
            d.move_rel_now(7, -2).unwrap();
            d.wheel_now(3).unwrap();
            d.flush_motion().unwrap();
            d.discard_motion().unwrap();
            d.move_axis(Motion::Wheel(1), MoveTiming::Now, PendingMotion::Flush)
                .unwrap();
        }),
        vec![
            vec![0, 7, 0, 0xFE, 0xFF, 0x01],
            vec![1, 3, 0, 0x01],
            vec![0, 0, 0, 0, 0, 0x02],
            vec![0, 0, 0, 0, 0, 0x04],
            vec![1, 1, 0, 0x03],
        ]
    );
}

#[test]
fn async_clip_set_ride_sends_the_ride_id() {
    use crate::protocol::FrameType;

    let mock = MockBox::new();
    let device = Device::with_mock(mock.clone()).into_async();
    device.clip().set_ride(true).unwrap();
    let sent: Vec<Vec<u8>> = mock
        .recorded_frames()
        .into_iter()
        .filter(|f| f.ty == FrameType::ClipSet)
        .map(|f| f.payload)
        .collect();
    assert_eq!(sent, vec![vec![3, 1]]);
}

#[test]
fn async_scale_verbs_send_the_same_frames_as_the_sync_ones() {
    use crate::protocol::FrameType;
    use crate::protocol::opcode::{
        LOCK_CLS_AXIS, LOCK_CLS_MEDIA, LOCK_DIR_AGAINST, LOCK_DIR_BOTH, LOCK_DIR_WITH,
        LOCK_SCALE_BLOCK,
    };
    use crate::{Axis, Blanket, Direction, MediaKey};

    let mock = MockBox::new();
    let device = Device::with_mock(mock.clone()).into_async();
    device.scale(Axis::X, Direction::Against, 40).unwrap();
    device
        .scale_axis(Axis::Wheel, Direction::With, 130)
        .unwrap();
    device.scale_all(Blanket::Aim, Direction::Both, 50).unwrap();
    device.lock(MediaKey::MUTE, Direction::PRESS).unwrap();
    let sent: Vec<Vec<u8>> = mock
        .recorded_frames()
        .into_iter()
        .filter(|f| f.ty == FrameType::Lock)
        .map(|f| f.payload)
        .collect();
    assert_eq!(
        sent,
        vec![
            vec![LOCK_CLS_AXIS, 0, 0, LOCK_DIR_AGAINST, 40],
            vec![LOCK_CLS_AXIS, 2, 0, LOCK_DIR_WITH, 130],
            vec![LOCK_CLS_AXIS, 0, 0, LOCK_DIR_BOTH, 50],
            vec![LOCK_CLS_AXIS, 1, 0, LOCK_DIR_BOTH, 50],
            vec![LOCK_CLS_MEDIA, 0xE2, 0x00, LOCK_DIR_BOTH, LOCK_SCALE_BLOCK],
        ]
    );
}

#[test]
fn async_refuses_a_relative_direction_the_box_would_drop() {
    use crate::{Blanket, Button, Direction};
    let device = Device::with_mock(MockBox::new()).into_async();
    assert!(matches!(
        device.lock(Button::Left, Direction::With),
        Err(Error::RelativeDirection { .. })
    ));
    assert!(matches!(
        device.scale_all(Blanket::Keys, Direction::Against, 40),
        Err(Error::RelativeDirection { .. })
    ));
    assert!(
        device
            .scale_axis(crate::Axis::Y, Direction::With, 60)
            .is_ok()
    );
}

#[test]
fn async_bearing_round_trips_through_the_mock() {
    use crate::protocol::FrameType;
    use crate::types::{Bearing, BearingMode};
    use std::time::Duration;

    let mock = MockBox::new();
    let device = Device::with_mock(mock.clone()).into_async();
    device
        .set_bearing(Some(Duration::from_millis(35)), BearingMode::Vector)
        .unwrap();
    let sent: Vec<Vec<u8>> = mock
        .recorded_frames()
        .into_iter()
        .filter(|f| f.ty == FrameType::Option)
        .map(|f| f.payload)
        .collect();
    assert_eq!(sent, vec![vec![4, 35, 0, 1]]);
    assert_eq!(
        block_on(device.query_bearing()).unwrap(),
        Bearing {
            window: Some(Duration::from_millis(35)),
            mode: BearingMode::Vector,
        }
    );
}
