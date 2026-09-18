//! Buffered clip playback (§3.11 / §4.15): entry-stream encoder, CLIP_CTRL/CLIP_SET/CLIP_TRIGGER frames, and QUERY(CLIP) decode, pinned to the firmware wire.

use crate::device::clip::encode_chunks;
use crate::error::Error;
use crate::protocol::command::{clip_op_payload, clip_set_payload, clip_trigger_payload};
use crate::protocol::opcode::{
    CLIP_OP_CLEAR, CLIP_OP_FINALIZE, CLIP_OP_STOP, CLIP_SET_LOOP, CLIP_SET_RIDE,
    CLIP_TRIG_F_CONSUME, CLIP_TRIG_F_PRESENT,
};
use crate::protocol::{Resp, parse_resp};
use crate::types::{
    Blanket, Button, CLIP_EDGES_MAX, CLIP_ENTRY_MAX, CLIP_RAW_MAX, ClipAction, ClipBuilder,
    ClipFrame, ClipSettings, ClipState, ClipStatus, ClipTrigger, Direction, Edge, Key, MediaKey,
    Setup, Usage,
};

// The whole stream as one piece, the way the firmware's ring holds it.
fn bytes(b: &ClipBuilder) -> Vec<u8> {
    encode_chunks(b, usize::MAX).unwrap().concat()
}

#[test]
fn clip_builder_encodes_entries_to_the_firmware_wire() {
    let mut b = ClipBuilder::new();
    b.gap(10);
    assert_eq!(bytes(&b), &[0x00, 0x0A, 0x00]);
    assert_eq!(b.len(), 1);

    let mut z = ClipBuilder::new();
    z.gap(0);
    assert!(z.is_empty());
    assert_eq!(bytes(&z), &[] as &[u8]);

    let mut m = ClipBuilder::new();
    m.move_by(5, -3);
    assert_eq!(bytes(&m), &[0x01, 0x05, 0x00, 0xFD, 0xFF]);

    let mut w = ClipBuilder::new();
    w.wheel(2);
    assert_eq!(bytes(&w), &[0x02, 0x02, 0x00]);

    let mut p = ClipBuilder::new();
    p.press(Button::LEFT);
    assert_eq!(bytes(&p), &[0x04, 0x01, 0x00, 0x00, 0x00, 0x01]);

    let mut r = ClipBuilder::new();
    r.release(Button::RIGHT);
    assert_eq!(bytes(&r), &[0x04, 0x01, 0x00, 0x01, 0x00, 0x00]);

    let mut f = ClipBuilder::new();
    f.frame(
        ClipFrame::new()
            .move_by(1, 2)
            .wheel(-1)
            .press(Button::LEFT)
            .force_release(Button::LEFT),
    );
    assert_eq!(
        bytes(&f),
        &[
            0x07, 0x01, 0x00, 0x02, 0x00, 0xFF, 0xFF, 0x02, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00,
            0x00, 0x02,
        ]
    );

    // an all-zero content frame with no edges still emits a report (zero XY tick, never a gap tag)
    let mut empty = ClipBuilder::new();
    empty.frame(ClipFrame::new());
    assert_eq!(bytes(&empty), &[0x01, 0x00, 0x00, 0x00, 0x00]);
}
// The byte vectors are tests/host/test_clip_entry.c's in the firmware repo: the two encoders agree on
// every one, or a clip built here faults the box.
#[test]
fn clip_frame_encodes_pan_raw_and_transfers_to_the_firmware_wire() {
    // Pan sits with the motion fields, ahead of the edges, whatever its flag bit says.
    let mut p = ClipBuilder::new();
    p.frame(ClipFrame::new().wheel(2).pan(-2).press(Button::new(9)));
    assert_eq!(
        bytes(&p),
        &[0x0E, 0x02, 0x00, 0xFE, 0xFF, 0x01, 0x00, 0x09, 0x00, 0x01]
    );
    let mut only = ClipBuilder::new();
    only.pan(7);
    assert_eq!(bytes(&only), &[0x08, 0x07, 0x00]);

    let mut r = ClipBuilder::new();
    r.frame(
        ClipFrame::new()
            .move_by(1, 0)
            .raw(1, Direction::IN, [0xAA, 0xBB, 0xCC])
            .raw(2, Direction::OUT, []),
    );
    assert_eq!(
        bytes(&r),
        &[
            0x11, 0x01, 0x00, 0x00, 0x00, 0x02, 0x01, 0x01, 0x03, 0x00, 0xAA, 0xBB, 0xCC, 0x02,
            0x02, 0x00, 0x00,
        ]
    );
    // The endpoint is a number, not an address: bit 7 is the direction's to carry.
    let mut masked = ClipBuilder::new();
    masked.raw(0x81, Direction::IN, [0x00]);
    assert_eq!(bytes(&masked)[2], 0x01);

    let mut x = ClipBuilder::new();
    x.frame(
        ClipFrame::new()
            .transfer(0, Setup::new(0x21, 0x09, 0x0300, 0, 2), [0x11, 0x22])
            .transfer(3, Setup::new(0xA1, 0x01, 0x0300, 0, 0x5A), []),
    );
    assert_eq!(
        bytes(&x),
        &[
            0x20, 0x02, 0x00, 0x21, 0x09, 0x00, 0x03, 0x00, 0x00, 0x02, 0x00, 0x11, 0x22, 0x03,
            0xA1, 0x01, 0x00, 0x03, 0x00, 0x00, 0x5A, 0x00,
        ]
    );

    let mut all = ClipBuilder::new();
    all.frame(
        ClipFrame::new()
            .move_by(1, 2)
            .wheel(3)
            .pan(4)
            .press(MediaKey::new(0x1234))
            .raw(5, Direction::IN, [0x7E])
            .transfer(0, Setup::new(0x80, 0x06, 0x0100, 0, 18), []),
    );
    assert_eq!(
        bytes(&all),
        &[
            0x3F, 0x01, 0x00, 0x02, 0x00, 0x03, 0x00, 0x04, 0x00, 0x01, 0x02, 0x34, 0x12, 0x01,
            0x01, 0x05, 0x01, 0x01, 0x00, 0x7E, 0x01, 0x00, 0x80, 0x06, 0x00, 0x01, 0x00, 0x00,
            0x12, 0x00,
        ]
    );
}

#[test]
fn a_frame_the_box_would_fault_on_is_refused_before_anything_is_encoded() {
    let refused = |f: ClipFrame| {
        let mut b = ClipBuilder::new();
        b.move_by(1, 1).frame(f);
        encode_chunks(&b, 512).unwrap_err()
    };
    let mut edges = ClipFrame::new();
    for i in 0..=CLIP_EDGES_MAX {
        edges = edges.press(Button::new(i as u8));
    }
    assert!(matches!(
        refused(edges),
        Error::ClipFrameCount {
            what: "edges",
            count: 9,
            limit: 8
        }
    ));
    let mut raws = ClipFrame::new();
    for _ in 0..=CLIP_RAW_MAX {
        raws = raws.raw(1, Direction::IN, [0]);
    }
    assert!(matches!(
        refused(raws),
        Error::ClipFrameCount {
            what: "raw reports",
            count: 9,
            limit: 8
        }
    ));
    // Eight of each is a full frame, and it encodes.
    let mut full = ClipFrame::new();
    for i in 0..CLIP_EDGES_MAX {
        full = full.press(Button::new(i as u8));
    }
    for _ in 0..CLIP_RAW_MAX {
        full = full.raw(1, Direction::IN, [0]);
    }
    let mut at_the_limit = ClipBuilder::new();
    at_the_limit.frame(full);
    assert_eq!(bytes(&at_the_limit).len(), 1 + (1 + 8 * 4) + (1 + 8 * 5));
    // 506 raw bytes fill an entry exactly; one more does not fit a CLIP_APPEND.
    let mut fits = ClipBuilder::new();
    fits.raw(2, Direction::OUT, vec![0u8; 506]);
    assert_eq!(bytes(&fits).len(), CLIP_ENTRY_MAX);
    assert!(matches!(
        refused(ClipFrame::new().raw(2, Direction::OUT, vec![0u8; 507])),
        Error::ClipFrameTooLong { len: 513 }
    ));
    // The length reported is the whole frame's, whichever field carried it past the cap.
    assert!(matches!(
        refused(
            ClipFrame::new()
                .move_by(1, 1)
                .raw(2, Direction::OUT, vec![0u8; 600])
        ),
        Error::ClipFrameTooLong { len: 610 }
    ));
    assert!(matches!(
        refused(ClipFrame::new().raw(1, Direction::Both, [0])),
        Error::RawDirection { .. }
    ));
    assert!(matches!(
        refused(ClipFrame::new().raw(1, Direction::With, [0])),
        Error::RelativeDirection { .. }
    ));
    // A transfer item's length is its setup packet's: wLength bytes of OUT data, none for an IN request.
    assert!(matches!(
        refused(ClipFrame::new().transfer(0, Setup::new(0x21, 9, 0, 0, 4), [1])),
        Error::ClipTransferData { want: 4, got: 1 }
    ));
    assert!(matches!(
        refused(ClipFrame::new().transfer(0, Setup::new(0xA1, 1, 0, 0, 4), [1, 2, 3, 4])),
        Error::ClipTransferData { want: 0, got: 4 }
    ));
}

#[test]
fn clip_command_payload_bytes() {
    assert_eq!(clip_op_payload(CLIP_OP_STOP), [1]);
    assert_eq!(clip_op_payload(CLIP_OP_CLEAR), [6]);
    assert_eq!(clip_op_payload(CLIP_OP_FINALIZE), [7]);
    assert_eq!(clip_set_payload(CLIP_SET_LOOP, 1), [1, 1]);
    assert_eq!(clip_set_payload(CLIP_SET_RIDE, 1), [3, 1]);
    // CLIP_TRIGGER: [class][id u16 LE][edge][action][flags]
    let flags = CLIP_TRIG_F_PRESENT | CLIP_TRIG_F_CONSUME;
    assert_eq!(
        clip_trigger_payload(1, 0x3A, 1, 0, flags),
        [1, 0x3A, 0x00, 1, 0, 3]
    );
    assert_eq!(
        clip_trigger_payload(0xFF, 0xFFFF, 0, 0, 0),
        [0xFF, 0xFF, 0xFF, 0, 0, 0]
    );
}

#[test]
fn decode_clip_status_and_settings_from_one_frame() {
    let p = [
        10u8, 1, 0x00, 0x01, 0x00, 0x00, // free 256
        0x0A, 0x00, 0x00, 0x00, // total 10
        0x05, 0x00, 0x00, 0x00, // played 5
        0xC8, 0x00, 0x00, 0x00, // ticks 200
        0x03, 0x00, // underruns 3
        0x01, 0x00, // overruns 1
        0x02, 0x00, // seq_gaps 2
        0x07, 0x00, // xfers 7
        0x01, 0x01, // xfer_errs 257
        0x09, 0x00, // gated 9
        0x02, // held_n
        0x00, 0x04, 0x00, // Button::SIDE2
        0x01, 0xE1, 0x00, // Key 0xE1
        0x05, // autolock Aim|Buttons
        0x0E, // flags retain|finalized|ride
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
            xfers: 7,
            xfer_errs: 257,
            gated: 9,
            held: vec![Usage::from(Button::SIDE2), Usage::from(Key::new(0xE1))],
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
            ride: true,
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
            Usage::from(Button::LEFT),
            Usage::from(Key::new(0x04)),
            Usage::from(MediaKey::new(0x00E9)),
        ],
        ..Default::default()
    };
    assert!(s.is_held(Button::LEFT));
    assert!(s.is_held(Key::new(0x04)));
    assert!(s.is_held(MediaKey::new(0x00E9)));
    assert!(!s.is_held(Button::RIGHT));
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
    clip.set_autolock(&[Blanket::Aim, Blanket::Buttons])
        .unwrap();
    clip.set_loop(true).unwrap();
    clip.set_ride(true).unwrap();
    clip.start().unwrap();
    clip.pause().unwrap();
    clip.resume().unwrap();
    clip.restart().unwrap();
    clip.toggle().unwrap();
    clip.stop().unwrap();
    clip.clear().unwrap();
    clip.finalize().unwrap();
    clip.bind(ClipTrigger::new(
        Key::new(0x3A),
        Edge::Press,
        ClipAction::Start,
    ))
    .unwrap();
    clip.bind(ClipTrigger::new(Button::RIGHT, Edge::Release, ClipAction::Toggle).consume())
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
        vec![vec![2, 1], vec![0, 0x05], vec![1, 1], vec![3, 1]]
    );
    assert_eq!(
        by(FrameType::ClipCtrl),
        vec![
            vec![0],
            vec![2],
            vec![3],
            vec![4],
            vec![5],
            vec![1],
            vec![6],
            vec![7]
        ]
    );
    assert_eq!(
        by(FrameType::ClipTrigger),
        vec![
            vec![1, 0x3A, 0x00, 1, 0, 1],    // bind KEY 0x3A Press Start (present)
            vec![0, 0x01, 0x00, 2, 5, 3],    // bind Button::RIGHT Release Toggle (present|consume)
            vec![1, 0x3A, 0x00, 1, 0, 0],    // unbind KEY 0x3A Press (present=0)
            vec![0xFF, 0xFF, 0xFF, 0, 0, 0], // clear-all sentinel
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
    assert_eq!(reassembled, bytes(&b), "no bytes lost or reordered");
}
#[cfg(feature = "mock")]
#[test]
fn clip_append_never_splits_an_entry_and_sends_nothing_when_one_is_refused() {
    use crate::protocol::FrameType;
    use crate::protocol::opcode::MAX_PAYLOAD;
    use crate::{Device, MockBox};

    let mock = MockBox::new();
    let device = Device::with_mock(mock.clone());
    let appends = || {
        mock.recorded_frames()
            .into_iter()
            .filter(|f| f.ty == FrameType::ClipAppend)
            .map(|f| f.payload)
            .collect::<Vec<_>>()
    };

    // Entries of very different sizes: a piece ends where an entry ends, however they fall.
    let mut b = ClipBuilder::new();
    for i in 0..6u8 {
        b.move_by(1, 0).raw(2, Direction::OUT, vec![i; 300]);
    }
    device.clip().append(&b).unwrap();
    let frames = appends();
    assert_eq!(frames.concat(), bytes(&b));
    // A limit of one byte puts every entry in a piece of its own, which is where the boundaries are.
    let mut ends = std::collections::BTreeSet::new();
    let mut at = 0;
    for entry in encode_chunks(&b, 1).unwrap() {
        at += entry.len();
        ends.insert(at);
    }
    assert_eq!(ends.len(), 12);
    let mut at = 0;
    for f in &frames {
        assert!(f.len() <= MAX_PAYLOAD);
        at += f.len();
        assert!(
            ends.contains(&at),
            "a piece ends {at} bytes in, inside an entry"
        );
    }
    assert_eq!(
        frames.len(),
        6,
        "two 306-byte raw entries never share a piece"
    );

    // A bad entry at the END of a long clip: nothing at all goes out, not the good pieces before it.
    let sent = appends().len();
    let mut bad = ClipBuilder::new();
    for _ in 0..200 {
        bad.move_by(1, 0);
    }
    bad.raw(1, Direction::Both, [0]);
    assert!(device.clip().append(&bad).is_err());
    assert_eq!(appends().len(), sent);
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
        xfers: 0x0708,
        xfer_errs: 0x090A,
        gated: 0x0B0C,
        held: vec![
            Usage::from(Button::LEFT),
            Usage::from(MediaKey::new(0x00E9)),
        ],
    };
    let settings = ClipSettings {
        autolock: vec![Blanket::Aim],
        loop_: true,
        retain: true,
        finalized: false,
        ride: true,
        triggers: vec![
            ClipTrigger::new(Button::RIGHT, Edge::Both, ClipAction::Toggle),
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

// A streaming host paces its appends against `ClipStatus::free`, so the length it holds against it has
// to be the length the ring takes.
#[test]
fn byte_len_is_the_length_the_stream_encodes_to() {
    let get = Setup::new(0x80, 6, 0x0100, 0, 18);
    let set = Setup::new(0x21, 9, 0x0300, 0, 2);
    let frames = [
        ClipFrame::new(),
        ClipFrame::new().move_by(0, -1),
        ClipFrame::new().wheel(1),
        ClipFrame::new().pan(-1),
        ClipFrame::new().wheel(1).pan(1),
        ClipFrame::new().press(Button::LEFT).release(Key::A),
        ClipFrame::new().raw(1, Direction::IN, [1, 2, 3]),
        ClipFrame::new()
            .raw(1, Direction::IN, [])
            .raw(2, Direction::OUT, [9]),
        ClipFrame::new().transfer(0, get, []),
        ClipFrame::new()
            .transfer(0, set, [4, 1])
            .transfer(0, get, []),
        ClipFrame::new()
            .move_by(3, 4)
            .wheel(1)
            .pan(2)
            .press(Button::LEFT)
            .raw(2, Direction::OUT, [7; 40])
            .transfer(0, set, [4, 1]),
    ];
    let mut all = ClipBuilder::new();
    for f in frames {
        let mut one = ClipBuilder::new();
        one.frame(f.clone());
        assert_eq!(f.byte_len(), bytes(&one).len(), "{f:?}");
        all.frame(f).gap(7);
    }
    assert_eq!(all.byte_len(), bytes(&all).len());
    assert_eq!(ClipBuilder::new().byte_len(), 0);
}

// A reply whose length is not the one its own counts describe is some other layout, and reading the
// counters out of it would hand back another field's bytes.
#[test]
fn a_clip_reply_of_another_shape_is_refused() {
    let mut good = vec![0u8; 31];
    good[0] = 10;
    good.extend_from_slice(&[0, 0, 0]);
    assert!(ClipStatus::from_payload(&good).is_some());
    assert!(ClipSettings::from_payload(&good).is_some());

    let mut long = good.clone();
    long.push(0);
    let mut short = good.clone();
    short.pop();
    // A shorter prefix carrying two triggers: 25 + 3 + 12 bytes.
    let mut older = vec![0u8; 25];
    older[0] = 10;
    older.extend_from_slice(&[0, 0, 2]);
    older.extend_from_slice(&[0, 3, 0, 1, 0, 0, 1, 4, 0, 1, 0, 0]);
    // A trigger count past the box's set.
    let mut many = vec![0u8; 31];
    many[0] = 10;
    many.extend_from_slice(&[0, 0, 9]);
    many.extend_from_slice(&[0u8; 54]);
    for bad in [long, short, older, many] {
        assert!(ClipStatus::from_payload(&bad).is_none(), "{bad:02X?}");
        assert!(ClipSettings::from_payload(&bad).is_none(), "{bad:02X?}");
    }
}
