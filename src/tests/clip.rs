//! Buffered clip playback (§3.11 / §4.15): entry-stream encoder, CLIP_CTRL/CLIP_APPEND frames, and QUERY(CLIP) decode, pinned to the firmware wire.

use crate::protocol::command::{clip_arm_payload, clip_op_payload, clip_start_payload};
use crate::protocol::opcode::{CLIP_OP_DISARM, CLIP_OP_STOP};
use crate::protocol::{Resp, parse_resp};
use crate::types::{Action, Button, ClipBuilder, ClipState, ClipStatus, Key, MediaKey, Usage};

#[test]
fn clip_builder_encodes_entries_to_the_firmware_wire() {
    let mut b = ClipBuilder::new();
    b.gap(10);
    assert_eq!(b.as_bytes(), &[0x00, 0x0A, 0x00]);
    assert_eq!(b.len(), 1);

    let mut z = ClipBuilder::new();
    z.gap(0);
    assert!(z.is_empty());
    assert_eq!(z.as_bytes(), &[] as &[u8]);

    let mut m = ClipBuilder::new();
    m.move_by(5, -3);
    assert_eq!(m.as_bytes(), &[0x01, 0x05, 0x00, 0xFD, 0xFF]);

    let mut w = ClipBuilder::new();
    w.wheel(2);
    assert_eq!(w.as_bytes(), &[0x02, 0x02, 0x00]);

    let mut p = ClipBuilder::new();
    p.press(Button::Left);
    assert_eq!(p.as_bytes(), &[0x04, 0x01, 0x00, 0x00, 0x00, 0x01]);

    let mut r = ClipBuilder::new();
    r.release(Button::Right);
    assert_eq!(r.as_bytes(), &[0x04, 0x01, 0x00, 0x01, 0x00, 0x00]);

    let mut f = ClipBuilder::new();
    f.frame(
        1,
        2,
        -1,
        &[
            (Button::Left.into(), Action::Press),
            (Button::Left.into(), Action::ForceRelease),
        ],
    );
    assert_eq!(
        f.as_bytes(),
        &[
            0x07,
            0x01, 0x00, 0x02, 0x00,
            0xFF, 0xFF,
            0x02,
            0x00, 0x00, 0x00, 0x01,
            0x00, 0x00, 0x00, 0x02,
        ]
    );

    // an all-zero content frame with no edges still emits a report (zero XY tick, never a gap tag)
    let mut empty = ClipBuilder::new();
    empty.frame(0, 0, 0, &[]);
    assert_eq!(empty.as_bytes(), &[0x01, 0x00, 0x00, 0x00, 0x00]);
}

#[test]
fn clip_ctrl_payload_bytes() {
    assert_eq!(clip_start_payload(0), [0, 0]);
    assert_eq!(clip_start_payload(0x1F), [0, 0x1F]);
    assert_eq!(clip_arm_payload(0, 0xFFFF, 0), [2, 0, 0xFF, 0xFF, 0]);
    assert_eq!(clip_arm_payload(1, 0x04, 0x0C), [2, 1, 0x04, 0, 0x0C]);
    assert_eq!(
        clip_arm_payload(0xFF, 0xFFFF, 0x1F),
        [2, 0xFF, 0xFF, 0xFF, 0x1F]
    );
    assert_eq!(clip_op_payload(CLIP_OP_STOP), [1]);
    assert_eq!(clip_op_payload(CLIP_OP_DISARM), [3]);
}

#[test]
fn decode_clip_status_through_parse_resp() {
    let p = [
        10u8, 2,
        0x00, 0x01, 0x00, 0x00,
        0x0A, 0x00, 0x00, 0x00,
        0xC8, 0x00, 0x00, 0x00,
        0x03, 0x00,
        0x01, 0x00,
        0x02, 0x00,
        0x02,
        0x00, 0x04, 0x00,
        0x01, 0xE1, 0x00,
    ];
    let Some(Resp::Clip(s)) = parse_resp(&p) else {
        panic!("expected Clip");
    };
    assert_eq!(
        s,
        ClipStatus {
            state: ClipState::Playing,
            free: 256,
            used: 10,
            ticks: 200,
            underruns: 3,
            overruns: 1,
            seq_gaps: 2,
            held: vec![Usage::from(Button::Side2), Usage::from(Key::new(0xE1))],
        }
    );
    assert!(parse_resp(&p[..20]).is_none()); // 20 bytes < 21 (no count byte)
    let mut bad = p;
    bad[1] = 9; // out-of-range state
    assert!(parse_resp(&bad).is_none());
}

#[test]
fn clip_status_held_is_field_generic() {
    // held is one class-tagged usage list: buttons, keys, and media reported the same way.
    let s = ClipStatus {
        held: vec![
            Usage::from(Button::Left),
            Usage::from(Key::new(0x04)),
            Usage::from(MediaKey::new(0x00E9)),
        ],
        ..Default::default()
    };
    assert!(s.is_held(Button::Left));
    assert!(s.is_held(Key::new(0x04)));
    assert!(s.is_held(MediaKey::new(0x00E9)));
    assert!(!s.is_held(Button::Right));
    assert!(!s.is_held(Key::new(0x05)));
}

#[test]
fn clip_state_from_u8() {
    assert_eq!(ClipState::from_u8(0), Some(ClipState::Idle));
    assert_eq!(ClipState::from_u8(1), Some(ClipState::Armed));
    assert_eq!(ClipState::from_u8(2), Some(ClipState::Playing));
    assert_eq!(ClipState::from_u8(3), Some(ClipState::Faulted));
    assert_eq!(ClipState::from_u8(4), None);
}

#[cfg(feature = "mock")]
#[test]
fn clip_control_frames_carry_the_right_bytes() {
    use crate::protocol::FrameType;
    use crate::types::{Blanket, ClipConfig, Key, MediaKey};
    use crate::{Device, MockBox};

    let mock = MockBox::new();
    let device = Device::with_mock(mock.clone());
    let clip = device.clip();

    let all = ClipConfig::new().autolock(Blanket::ALL);
    let aim_btn = ClipConfig::new().autolock(&[Blanket::Aim, Blanket::Buttons]);

    clip.start(&ClipConfig::new()).unwrap();
    clip.start(&all).unwrap();
    clip.start(&aim_btn).unwrap();
    clip.arm_catch(Button::Right, &ClipConfig::new()).unwrap();
    clip.arm_catch(Key::A, &ClipConfig::new().autolock(&[Blanket::Keys]))
        .unwrap();
    clip.arm_catch(MediaKey::new(0xCD), &ClipConfig::new())
        .unwrap();
    clip.arm_catch_any(&all).unwrap();
    clip.disarm().unwrap();
    clip.stop().unwrap();

    let ctrl: Vec<Vec<u8>> = mock
        .recorded_frames()
        .into_iter()
        .filter(|f| f.ty == FrameType::ClipCtrl)
        .map(|f| f.payload)
        .collect();
    assert_eq!(
        ctrl,
        vec![
            vec![0, 0],
            vec![0, 0x1F],
            vec![0, 0x05],
            vec![2, 0, 1, 0, 0],
            vec![2, 1, 0x04, 0, 0x08],
            vec![2, 2, 0xCD, 0, 0],
            vec![2, 0xFF, 0xFF, 0xFF, 0x1F],
            vec![3],
            vec![1],
        ]
    );
}

#[cfg(feature = "mock")]
#[test]
fn clip_append_chunks_on_entry_boundaries_with_incrementing_seq() {
    use crate::protocol::FrameType;
    use crate::protocol::opcode::MAX_PAYLOAD;
    use crate::{Device, MockBox};

    let mock = MockBox::new();
    let device = Device::with_mock(mock.clone());
    let clip = device.clip();

    // 200 five-byte move entries = 1000 bytes > MAX_PAYLOAD(512): must split into whole-entry frames.
    let mut b = ClipBuilder::new();
    for _ in 0..200 {
        b.move_by(1, 0);
    }
    clip.append(&b).unwrap();

    let frames: Vec<(u8, Vec<u8>)> = mock
        .recorded_frames()
        .into_iter()
        .filter(|f| f.ty == FrameType::ClipAppend)
        .map(|f| (f.seq, f.payload))
        .collect();

    assert!(frames.len() >= 2, "should split: {} frames", frames.len());
    let mut reassembled = Vec::new();
    for (i, (seq, payload)) in frames.iter().enumerate() {
        assert_eq!(*seq, i as u8, "append seq increments contiguously");
        assert!(payload.len() <= MAX_PAYLOAD, "each frame fits the wire");
        assert_eq!(
            payload.len() % 5,
            0,
            "each frame holds whole 5-byte entries"
        );
        reassembled.extend_from_slice(payload);
    }
    assert_eq!(reassembled, b.as_bytes(), "no bytes lost or reordered");
}

#[cfg(feature = "mock")]
#[test]
fn clip_status_roundtrips_through_the_mock() {
    use crate::{Device, MockBox};

    let status = ClipStatus {
        state: ClipState::Faulted,
        free: 1024,
        used: 64,
        ticks: 12,
        underruns: 1,
        overruns: 2,
        seq_gaps: 1,
        held: vec![
            Usage::from(Button::Left),
            Usage::from(MediaKey::new(0x00E9)),
        ],
    };
    let mock = MockBox::new().with_clip_status(status.clone());
    let device = Device::with_mock(mock.clone());
    assert_eq!(device.clip().status().unwrap(), status);

    mock.set_clip_status(ClipStatus {
        state: ClipState::Idle,
        ..ClipStatus::default()
    });
    assert_eq!(device.clip().status().unwrap().state, ClipState::Idle);
}

#[cfg(feature = "mock")]
#[test]
fn empty_append_sends_nothing() {
    use crate::protocol::FrameType;
    use crate::{Device, MockBox};

    let mock = MockBox::new();
    let device = Device::with_mock(mock.clone());
    device.clip().append(&ClipBuilder::new()).unwrap();
    assert!(
        !mock
            .recorded_frames()
            .into_iter()
            .any(|f| f.ty == FrameType::ClipAppend)
    );
}
