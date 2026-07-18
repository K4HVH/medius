//! Buffered clip playback (§3.11 / §4.15): entry-stream encoder, CLIP_CTRL/CLIP_SET/CLIP_TRIGGER frames, and QUERY(CLIP) decode, pinned to the firmware wire.

use crate::protocol::command::{clip_op_payload, clip_set_payload, clip_trigger_payload};
use crate::protocol::opcode::{
    CLIP_OP_CLEAR, CLIP_OP_FINALIZE, CLIP_OP_STOP, CLIP_SET_LOOP, CLIP_TRIG_F_CONSUME,
    CLIP_TRIG_F_PRESENT,
};
use crate::protocol::{Resp, parse_resp};
use crate::types::{
    Action, Blanket, Button, ClipAction, ClipBuilder, ClipSettings, ClipState, ClipStatus,
    ClipTrigger, Edge, Key, MediaKey, Usage,
};

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
fn clip_command_payload_bytes() {
    assert_eq!(clip_op_payload(CLIP_OP_STOP), [1]);
    assert_eq!(clip_op_payload(CLIP_OP_CLEAR), [6]);
    assert_eq!(clip_op_payload(CLIP_OP_FINALIZE), [7]);
    assert_eq!(clip_set_payload(CLIP_SET_LOOP, 1), [1, 1]);
    // CLIP_TRIGGER: [class][id u16 LE][edge][action][flags]
    let flags = CLIP_TRIG_F_PRESENT | CLIP_TRIG_F_CONSUME;
    assert_eq!(clip_trigger_payload(1, 0x3A, 1, 0, flags), [1, 0x3A, 0x00, 1, 0, 3]);
    assert_eq!(clip_trigger_payload(0xFF, 0xFFFF, 0, 0, 0), [0xFF, 0xFF, 0xFF, 0, 0, 0]);
}

#[test]
fn decode_clip_status_and_settings_from_one_frame() {
    let p = [
        10u8, 1,
        0x00, 0x01, 0x00, 0x00, // free 256
        0x0A, 0x00, 0x00, 0x00, // total 10
        0x05, 0x00, 0x00, 0x00, // played 5
        0xC8, 0x00, 0x00, 0x00, // ticks 200
        0x03, 0x00, // underruns 3
        0x01, 0x00, // overruns 1
        0x02, 0x00, // seq_gaps 2
        0x02, // held_n
        0x00, 0x04, 0x00, // Button::Side2
        0x01, 0xE1, 0x00, // Key 0xE1
        0x05, // autolock Aim|Buttons
        0x06, // flags retain|finalized
        0x01, // n_trig
        0x01, 0x3A, 0x00, 0x01, 0x00, 0x01, // KEY 0x3A Press Start consume
    ];
    let Some(Resp::Clip(s)) = parse_resp(&p) else {
        panic!("expected Clip");
    };
    assert_eq!(
        s,
        ClipStatus {
            state: ClipState::Playing,
            free: 256,
            total: 10,
            played: 5,
            ticks: 200,
            underruns: 3,
            overruns: 1,
            seq_gaps: 2,
            held: vec![Usage::from(Button::Side2), Usage::from(Key::new(0xE1))],
        }
    );
    let cfg = ClipSettings::from_payload(&p).unwrap();
    assert_eq!(
        cfg,
        ClipSettings {
            autolock: vec![Blanket::Aim, Blanket::Buttons],
            loop_: false,
            retain: true,
            finalized: true,
            triggers: vec![
                ClipTrigger::new(Key::new(0x3A), Edge::Press, ClipAction::Start).consume()
            ],
        }
    );
    assert!(parse_resp(&p[..24]).is_none()); // 24 bytes < 25 (no held count)
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
    assert_eq!(ClipState::from_u8(1), Some(ClipState::Playing));
    assert_eq!(ClipState::from_u8(2), Some(ClipState::Paused));
    assert_eq!(ClipState::from_u8(3), Some(ClipState::Faulted));
    assert_eq!(ClipState::from_u8(4), None);
}

#[cfg(feature = "mock")]
#[test]
fn clip_command_frames_carry_the_right_bytes() {
    use crate::protocol::FrameType;
    use crate::{Device, MockBox};

    let mock = MockBox::new();
    let device = Device::with_mock(mock.clone());
    let clip = device.clip();

    clip.set_retain(true).unwrap();
    clip.set_autolock(&[Blanket::Aim, Blanket::Buttons]).unwrap();
    clip.set_loop(true).unwrap();
    clip.start().unwrap();
    clip.pause().unwrap();
    clip.resume().unwrap();
    clip.restart().unwrap();
    clip.toggle().unwrap();
    clip.stop().unwrap();
    clip.clear().unwrap();
    clip.finalize().unwrap();
    clip.bind(ClipTrigger::new(Key::new(0x3A), Edge::Press, ClipAction::Start))
        .unwrap();
    clip.bind(ClipTrigger::new(Button::Right, Edge::Release, ClipAction::Toggle).consume())
        .unwrap();
    clip.unbind(Key::new(0x3A), Edge::Press).unwrap();
    clip.clear_triggers().unwrap();

    let by = |ty: FrameType| -> Vec<Vec<u8>> {
        mock.recorded_frames()
            .into_iter()
            .filter(|f| f.ty == ty)
            .map(|f| f.payload)
            .collect()
    };
    assert_eq!(
        by(FrameType::ClipSet),
        vec![vec![2, 1], vec![0, 0x05], vec![1, 1]]
    );
    assert_eq!(
        by(FrameType::ClipCtrl),
        vec![vec![0], vec![2], vec![3], vec![4], vec![5], vec![1], vec![6], vec![7]]
    );
    assert_eq!(
        by(FrameType::ClipTrigger),
        vec![
            vec![1, 0x3A, 0x00, 1, 0, 1],       // bind KEY 0x3A Press Start (present)
            vec![0, 0x01, 0x00, 2, 5, 3],       // bind Button::Right Release Toggle (present|consume)
            vec![1, 0x3A, 0x00, 1, 0, 0],       // unbind KEY 0x3A Press (present=0)
            vec![0xFF, 0xFF, 0xFF, 0, 0, 0],    // clear-all sentinel
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
        assert_eq!(payload.len() % 5, 0, "each frame holds whole 5-byte entries");
        reassembled.extend_from_slice(payload);
    }
    assert_eq!(reassembled, b.as_bytes(), "no bytes lost or reordered");
}

#[cfg(feature = "mock")]
#[test]
fn clip_status_and_config_roundtrip_through_the_mock() {
    use crate::{Device, MockBox};

    let status = ClipStatus {
        state: ClipState::Faulted,
        free: 1024,
        total: 64,
        played: 8,
        ticks: 12,
        underruns: 1,
        overruns: 2,
        seq_gaps: 1,
        held: vec![Usage::from(Button::Left), Usage::from(MediaKey::new(0x00E9))],
    };
    let settings = ClipSettings {
        autolock: vec![Blanket::Aim],
        loop_: true,
        retain: true,
        finalized: false,
        triggers: vec![
            ClipTrigger::new(Button::Right, Edge::Both, ClipAction::Toggle),
            ClipTrigger::new(Key::new(0x3A), Edge::Release, ClipAction::Stop).consume(),
        ],
    };
    let mock = MockBox::new()
        .with_clip_status(status.clone())
        .with_clip_settings(settings.clone());
    let device = Device::with_mock(mock.clone());
    assert_eq!(device.clip().query_status().unwrap(), status);
    assert_eq!(device.clip().query_config().unwrap(), settings);

    mock.set_clip_status(ClipStatus {
        state: ClipState::Idle,
        ..ClipStatus::default()
    });
    assert_eq!(device.clip().query_status().unwrap().state, ClipState::Idle);
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
