//! Buffered clip playback (§3.11 / §4.15): the entry-stream encoder (pinned to the firmware `clip_entry.h`
//! wire), the `CLIP_CTRL` / `CLIP_APPEND` command frames, entry-boundary chunking, and the `QUERY(CLIP)`
//! decode + roundtrip. Bytes are pinned to the firmware so the host and box can't drift.

use crate::protocol::command::{clip_arm_payload, clip_cfg_payload, clip_op_payload};
use crate::protocol::opcode::{CLIP_OP_CONFIG, CLIP_OP_DISARM, CLIP_OP_START, CLIP_OP_STOP};
use crate::protocol::{Resp, parse_resp};
use crate::types::{Action, Button, ClipBuilder, ClipState, ClipStatus};

#[test]
fn clip_builder_encodes_entries_to_the_firmware_wire() {
    // gap: [tag=0][count u16 LE]
    let mut b = ClipBuilder::new();
    b.gap(10);
    assert_eq!(b.as_bytes(), &[0x00, 0x0A, 0x00]);
    assert_eq!(b.len(), 1);

    // gap(0) is a no-op (consumes no tick, emits nothing)
    let mut z = ClipBuilder::new();
    z.gap(0);
    assert!(z.is_empty());
    assert_eq!(z.as_bytes(), &[] as &[u8]);

    // move: [flags=XY(1)][dx i16 LE][dy i16 LE]
    let mut m = ClipBuilder::new();
    m.move_by(5, -3);
    assert_eq!(m.as_bytes(), &[0x01, 0x05, 0x00, 0xFD, 0xFF]);

    // wheel: [flags=WHEEL(2)][wheel i16 LE]
    let mut w = ClipBuilder::new();
    w.wheel(2);
    assert_eq!(w.as_bytes(), &[0x02, 0x02, 0x00]);

    // press left: [flags=EDGES(4)][n=1][class=INJ_BTN(0)][id u16 LE][action=PRESS(1)]
    let mut p = ClipBuilder::new();
    p.press(Button::Left);
    assert_eq!(p.as_bytes(), &[0x04, 0x01, 0x00, 0x00, 0x00, 0x01]);

    // release (soft-release) right: id 1, action SOFTREL(0)
    let mut r = ClipBuilder::new();
    r.release(Button::Right);
    assert_eq!(r.as_bytes(), &[0x04, 0x01, 0x00, 0x01, 0x00, 0x00]);

    // a full frame: motion + wheel + a two-edge tuple, flags = XY|WHEEL|EDGES
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
            0x07, // XY|WHEEL|EDGES
            0x01, 0x00, 0x02, 0x00, // dx=1 dy=2
            0xFF, 0xFF, // wheel=-1
            0x02, // n=2 edges
            0x00, 0x00, 0x00, 0x01, // btn left press
            0x00, 0x00, 0x00, 0x02, // btn left forcerel
        ]
    );

    // an all-zero content frame with no edges still emits a report (zero XY tick, never a gap tag)
    let mut empty = ClipBuilder::new();
    empty.frame(0, 0, 0, &[]);
    assert_eq!(empty.as_bytes(), &[0x01, 0x00, 0x00, 0x00, 0x00]);
}

#[test]
fn clip_ctrl_payload_bytes() {
    // START: [op=0][cfg]; cfg bit0 = auto-lock
    assert_eq!(clip_cfg_payload(CLIP_OP_START, false), [0, 0]);
    assert_eq!(clip_cfg_payload(CLIP_OP_START, true), [0, 1]);
    // CONFIG: [op=4][cfg]
    assert_eq!(clip_cfg_payload(CLIP_OP_CONFIG, true), [4, 1]);
    // ARM_CATCH: [op=2][cond_class][cond_id u16 LE]
    assert_eq!(clip_arm_payload(0, 0xFFFF), [2, 0, 0xFF, 0xFF]);
    assert_eq!(clip_arm_payload(0, 1), [2, 0, 1, 0]);
    // STOP / DISARM: [op]
    assert_eq!(clip_op_payload(CLIP_OP_STOP), [1]);
    assert_eq!(clip_op_payload(CLIP_OP_DISARM), [3]);
}

#[test]
fn decode_clip_status_through_parse_resp() {
    // RESP(CLIP): [what=10][state][free u32][used u32][ticks u32][underruns u16][overruns u16]
    // [seq_gaps u16][held u8]
    let p = [
        10u8, 2, // playing
        0x00, 0x01, 0x00, 0x00, // free = 256
        0x0A, 0x00, 0x00, 0x00, // used = 10
        0xC8, 0x00, 0x00, 0x00, // ticks = 200
        0x03, 0x00, // underruns = 3
        0x01, 0x00, // overruns = 1
        0x02, 0x00, // seq_gaps = 2
        0x01, // held
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
            held: 1,
        }
    );
    assert!(parse_resp(&p[..20]).is_none()); // 20 bytes < 21
    let mut bad = p;
    bad[1] = 9; // out-of-range state
    assert!(parse_resp(&bad).is_none());
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
    use crate::{Device, MockBox};

    let mock = MockBox::new();
    let device = Device::with_mock(mock.clone());
    let clip = device.clip();

    clip.start().unwrap();
    clip.start_autolock().unwrap();
    clip.config(true).unwrap();
    clip.config(false).unwrap();
    clip.arm_catch(None).unwrap();
    clip.arm_catch(Some(Button::Right)).unwrap();
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
            vec![0, 0],             // start
            vec![0, 1],             // start_autolock
            vec![4, 1],             // config(true)
            vec![4, 0],             // config(false)
            vec![2, 0, 0xFF, 0xFF], // arm any
            vec![2, 0, 1, 0],       // arm right
            vec![3],                // disarm
            vec![1],                // stop
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
        held: 0,
    };
    let mock = MockBox::new().with_clip_status(status);
    let device = Device::with_mock(mock.clone());
    assert_eq!(device.clip().status().unwrap(), status);

    // in-place update (e.g. the ring draining) is reflected on the next query
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
