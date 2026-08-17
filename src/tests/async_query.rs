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
