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
        0x00, // n_pkt
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
            packet_triggers: vec![],
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
    use crate::types::{ClipPacketTrigger, ClipPacketTriggerEntry, TrafficClass};
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
        packet_triggers: vec![
            ClipPacketTriggerEntry {
                trigger: ClipPacketTrigger::new(
                    TrafficClass::Control,
                    0,
                    Direction::Both,
                    ClipAction::Pause,
                ),
                hits: 0,
            },
            ClipPacketTriggerEntry {
                trigger: ClipPacketTrigger::new(
                    TrafficClass::VendorInterrupt,
                    3,
                    Direction::OUT,
                    ClipAction::Restart,
                )
                .matching([0x10, 0xFF, 0x05], [0xFF, 0xFF, 0x0F])
                .consume()
                .once_per_run(2),
                hits: 0x1234,
            },
        ],
    };
    // The consuming trigger needs the opt-in held when it is scripted.
    let mock = MockBox::new()
        .with_imperfect(true)
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
    good.extend_from_slice(&[0, 0, 0, 0]);
    assert!(ClipStatus::from_payload(&good).is_some());
    assert!(ClipSettings::from_payload(&good).is_some());

    let mut long = good.clone();
    long.push(0);
    // The config section ending at its input triggers, with no packet trigger count after them.
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
    many.push(0);
    for bad in [long, short, older, many] {
        assert!(ClipStatus::from_payload(&bad).is_none(), "{bad:02X?}");
        assert!(ClipSettings::from_payload(&bad).is_none(), "{bad:02X?}");
    }
}

// Packet triggers: `CLIP_TRIGGER` with a traffic class, and the list that closes `RESP(CLIP)`.
// The byte vectors are the firmware's: tests/host/test_clip_ptrig.c and tools/medius.py pin the same.
mod packet_trigger {
    use crate::device::clip::{validate_packet_key, validate_packet_trigger};
    use crate::error::Error;
    use crate::protocol::command::clip_packet_trigger_payload;
    use crate::protocol::opcode::{CLIP_PKT_TRIG_MAX, CLIP_TRIG_F_PRESENT};
    use crate::types::{
        Button, ClipAction, ClipPacketTrigger, ClipPacketTriggerEntry, ClipSettings, ClipStatus,
        ClipTrigger, Direction, Edge, Key, TrafficClass, Usage,
    };

    const M: [u8; 2] = [0x07, 0x20];
    const K: [u8; 2] = [0xFF, 0x20];
    const BIND: [u8; 12] = [
        0x04, 0x02, 0x00, 0x01, 0x00, 0x07, 0x01, 0x02, 0x07, 0x20, 0xFF, 0x20,
    ];
    const REMOVE: [u8; 12] = [
        0x04, 0x02, 0x00, 0x01, 0x00, 0x00, 0x00, 0x02, 0x07, 0x20, 0xFF, 0x20,
    ];
    const ENTRY: [u8; 14] = [
        0x04, 0x02, 0x01, 0x01, 0x05, 0x06, 0x01, 0x02, 0xFF, 0xFF, 0x07, 0x20, 0xFF, 0x20,
    ];

    fn held() -> ClipPacketTrigger {
        ClipPacketTrigger::new(TrafficClass::HidIn, 2, Direction::IN, ClipAction::Start)
            .matching(M, K)
    }

    // RESP(CLIP) holding two usages and one input trigger, then `entries` as its packet trigger list.
    fn reply(entries: &[Vec<u8>]) -> Vec<u8> {
        let mut p = vec![0u8; 31];
        p[0] = 10;
        p[30] = 2;
        p.extend_from_slice(&[0x00, 0x04, 0x00, 0x01, 0xE1, 0x00]); // Button::SIDE2, Key 0xE1
        p.extend_from_slice(&[0x05, 0x02, 0x01]); // autolock Aim|Buttons, retain, one input trigger
        p.extend_from_slice(&[0x01, 0x3A, 0x00, 0x01, 0x00, 0x01]); // KEY 0x3A Press Start consume
        p.push(entries.len() as u8);
        for e in entries {
            p.extend_from_slice(e);
        }
        p
    }

    // Entry `i` of a full list: a different class, id, verb and match length each, in a flow the
    // class carries.
    fn nth(i: u8) -> (Vec<u8>, ClipPacketTriggerEntry) {
        let (class, flow) = [
            (TrafficClass::HidIn, Direction::IN),
            (TrafficClass::HidOut, Direction::OUT),
            (TrafficClass::VendorInterrupt, Direction::OUT),
            (TrafficClass::VendorBulk, Direction::IN),
            (TrafficClass::Control, Direction::OUT),
            (TrafficClass::Emit, Direction::IN),
        ][i as usize % 6];
        let action = ClipAction::from_u8(i % 6).unwrap();
        let (m, k) = (vec![i; i as usize], vec![0xF0 | i; i as usize]);
        let mut wire = vec![
            class.as_u8(),
            i,
            0x01,
            flow.as_u8(),
            action.as_u8(),
            0,
            0,
            i,
            i,
            0x02,
        ];
        wire.extend_from_slice(&m);
        wire.extend_from_slice(&k);
        let trigger = ClipPacketTrigger::new(class, 0x0100 | i as u16, flow, action).matching(m, k);
        (
            wire,
            ClipPacketTriggerEntry {
                trigger,
                hits: 0x0200 | i as u16,
            },
        )
    }

    #[test]
    fn the_encoder_writes_the_wire_vectors() {
        assert_eq!(
            clip_packet_trigger_payload(4, 2, 1, 0, 0x07, 1, &M, &K),
            BIND
        );
        assert_eq!(
            clip_packet_trigger_payload(4, 2, 1, 0, 0, 0, &M, &K),
            REMOVE
        );
        // With no match the frame is its eight header bytes.
        assert_eq!(
            clip_packet_trigger_payload(8, 0xFFFF, 0, 5, 1, 0, &[], &[]),
            [0x08, 0xFF, 0xFF, 0x00, 0x05, 0x01, 0x00, 0x00]
        );
    }

    #[test]
    fn the_builders_set_the_flag_bits_the_box_reads() {
        assert_eq!(held().flags(), 0);
        assert_eq!(held().consume().flags(), 0x02);
        assert_eq!(held().once_per_run(1).flags(), 0x04);
        assert_eq!(held().consume().once_per_run(1).flags(), 0x06);
        assert_eq!(held().once_per_run(1).selector_len, 1);
        assert_eq!(ClipPacketTrigger::ANY_ID, 0xFFFF);
    }

    #[test]
    fn a_reply_entry_decodes_to_the_trigger_that_set_it() {
        let cfg = ClipSettings::from_payload(&reply(&[ENTRY.to_vec()])).unwrap();
        let want = ClipPacketTrigger::new(
            TrafficClass::HidIn,
            0x0102,
            Direction::IN,
            ClipAction::Toggle,
        )
        .matching(M, K)
        .consume()
        .once_per_run(1);
        assert_eq!(
            cfg.packet_triggers,
            vec![ClipPacketTriggerEntry {
                trigger: want,
                hits: 0xFFFF
            }]
        );
        // The entry is the command that set it with `hits` spliced in after the header.
        let t = &cfg.packet_triggers[0].trigger;
        let command = clip_packet_trigger_payload(
            t.class.as_u8(),
            t.id,
            t.direction.as_u8(),
            t.action.as_u8(),
            CLIP_TRIG_F_PRESENT | t.flags(),
            t.selector_len,
            &t.match_bytes,
            &t.mask,
        );
        let mut replayed = ENTRY[..8].to_vec();
        replayed[5] |= CLIP_TRIG_F_PRESENT;
        replayed.extend_from_slice(&ENTRY[10..]);
        assert_eq!(command, replayed);
    }

    #[test]
    fn settings_decode_with_no_one_and_eight_packet_triggers() {
        for n in [0u8, 1, 8] {
            let (wire, want): (Vec<_>, Vec<_>) = (0..n).map(nth).unzip();
            let p = reply(&wire);
            let cfg = ClipSettings::from_payload(&p).unwrap();
            assert_eq!(cfg.packet_triggers, want, "{n} packet triggers");
            // What sits ahead of the list reads the same whatever follows it.
            assert_eq!(
                cfg.triggers,
                vec![ClipTrigger::new(Key::new(0x3A), Edge::Press, ClipAction::Start).consume()]
            );
            assert!(cfg.retain && !cfg.loop_);
            let status = ClipStatus::from_payload(&p).unwrap();
            assert_eq!(
                status.held,
                vec![Usage::from(Button::SIDE2), Usage::from(Key::new(0xE1))]
            );
        }
    }

    // The list is part of the shape: a reply is read only when its length is the one its counts and
    // each entry's own match length describe.
    #[test]
    fn a_packet_trigger_list_of_another_shape_is_refused() {
        let refused = |p: &[u8], why: &str| {
            assert!(ClipStatus::from_payload(p).is_none(), "{why}");
            assert!(ClipSettings::from_payload(p).is_none(), "{why}");
        };
        let two = reply(&[nth(3).0, nth(2).0]);
        assert!(ClipSettings::from_payload(&two).is_some());

        let mut trailing = two.clone();
        trailing.push(0);
        refused(&trailing, "a byte after the last entry");
        refused(&two[..two.len() - 1], "the last mask byte cut off");
        let list = reply(&[]).len();
        refused(&two[..list], "a count of two and no entries");
        refused(&two[..list + 5], "cut inside an entry's header");
        refused(&two[..list + 10], "cut where the first match begins");
        refused(&two[..list - 1], "cut ahead of the count");

        let mut counts_three = two.clone();
        counts_three[list - 1] = 3;
        refused(&counts_three, "a count past the entries present");
        let mut counts_one = two.clone();
        counts_one[list - 1] = 1;
        refused(&counts_one, "an entry past the count");

        let nine: Vec<Vec<u8>> = (0..=CLIP_PKT_TRIG_MAX as u8).map(|_| nth(0).0).collect();
        assert!(ClipSettings::from_payload(&reply(&nine[..8])).is_some());
        refused(&reply(&nine), "a count past the box's set");

        let mut wide = vec![0x04, 0x02, 0x00, 0x01, 0x00, 0x00, 0x00, 16, 0x00, 0x00];
        wide.extend_from_slice(&[0u8; 32]);
        assert!(ClipSettings::from_payload(&reply(&[wide.clone()])).is_some());
        wide[7] = 17;
        wide.extend_from_slice(&[0u8; 2]);
        refused(&reply(&[wide]), "a match past the box's compare length");
    }

    // An entry naming a class, direction or verb this crate has no name for is left out, and the ones
    // around it still read.
    #[test]
    fn an_entry_the_crate_has_no_names_for_is_skipped() {
        let with = |at: usize, v: u8| {
            let mut e = nth(1).0;
            e[at] = v;
            e
        };
        let p = reply(&[
            nth(0).0,
            with(0, 3),  // an input class
            with(0, 10), // the bus class
            with(0, 11), // a clip's transfers
            with(0, 12),
            with(3, 3), // a bearing-relative direction
            with(3, 5),
            with(4, 6), // CLEAR is a host verb
            nth(2).0,
        ]);
        // Nine entries is past the set, so drop one that would have decoded.
        assert!(ClipSettings::from_payload(&p).is_none());
        let p = reply(&[
            nth(0).0,
            with(0, 3),
            with(0, 10),
            with(0, 11),
            with(3, 3),
            with(3, 5),
            with(4, 6),
            nth(2).0,
        ]);
        let cfg = ClipSettings::from_payload(&p).unwrap();
        assert_eq!(cfg.packet_triggers, vec![nth(0).1, nth(2).1]);
    }

    fn reason(t: &ClipPacketTrigger) -> &'static str {
        match validate_packet_trigger(t) {
            Err(Error::ClipPacketTrigger { reason }) => reason,
            other => panic!("not a packet trigger refusal: {other:?}"),
        }
    }

    #[test]
    fn what_the_box_takes_passes() {
        let on = |class, id, direction| {
            ClipPacketTrigger::new(class, id, direction, ClipAction::Start).matching(M, K)
        };
        for (class, flow) in [
            (TrafficClass::HidIn, Direction::IN),
            (TrafficClass::HidOut, Direction::OUT),
            (TrafficClass::VendorInterrupt, Direction::IN),
            (TrafficClass::VendorInterrupt, Direction::OUT),
            (TrafficClass::VendorBulk, Direction::IN),
            (TrafficClass::VendorBulk, Direction::OUT),
            (TrafficClass::Emit, Direction::IN),
        ] {
            assert!(validate_packet_trigger(&on(class, 2, flow)).is_ok());
            assert!(validate_packet_trigger(&on(class, 2, flow).consume()).is_ok());
            assert!(validate_packet_trigger(&on(class, 2, flow).consume().once_per_run(1)).is_ok());
        }
        // Every packet, from anywhere: legal with neither flag.
        assert!(validate_packet_trigger(&on(TrafficClass::Control, 0, Direction::IN)).is_ok());
        let any = ClipPacketTrigger::ANY_ID;
        assert!(validate_packet_trigger(&on(TrafficClass::HidIn, any, Direction::Both)).is_ok());
        assert!(
            validate_packet_trigger(&on(TrafficClass::HidIn, any, Direction::Both).consume())
                .is_ok()
        );
        // With no selector every packet at the address is in the run.
        assert!(validate_packet_trigger(&held().once_per_run(0)).is_ok());
        assert!(validate_packet_trigger(&held().matching([7; 16], [0xFF; 16])).is_ok());
        assert!(
            validate_packet_trigger(&held().matching([7; 16], [0xFF; 16]).once_per_run(15)).is_ok()
        );
        let bare = ClipPacketTrigger::new(TrafficClass::Emit, 1, Direction::IN, ClipAction::Toggle);
        assert!(validate_packet_trigger(&bare).is_ok());
    }

    #[test]
    fn each_refusal_names_its_own_fault() {
        for class in [TrafficClass::Bus, TrafficClass::ClipTransfer] {
            let t = ClipPacketTrigger::new(class, 0, Direction::IN, ClipAction::Start);
            assert!(reason(&t).contains("surface packets cross"));
        }
        assert!(reason(&held().matching([7; 17], [0xFF; 17])).contains("at most 16"));
        assert!(reason(&held().matching([7, 8], [0xFF])).contains("one length"));
        assert!(reason(&held().matching([7], [0xFF, 0xFF])).contains("one length"));

        let control =
            ClipPacketTrigger::new(TrafficClass::Control, 0, Direction::IN, ClipAction::Start)
                .matching(M, K);
        assert!(reason(&control.clone().consume()).contains("Control transfer"));
        assert!(reason(&control.once_per_run(1)).contains("one stream"));

        let every_id = ClipPacketTrigger {
            id: ClipPacketTrigger::ANY_ID,
            ..held()
        };
        assert!(reason(&every_id.once_per_run(1)).contains("one stream"));
        let either_flow = ClipPacketTrigger {
            direction: Direction::Both,
            ..held()
        };
        assert!(reason(&either_flow.once_per_run(1)).contains("one stream"));

        assert!(reason(&held().once_per_run(2)).contains("match bytes past its selector"));
        assert!(reason(&held().once_per_run(3)).contains("match bytes past its selector"));
        let no_match =
            ClipPacketTrigger::new(TrafficClass::HidIn, 2, Direction::IN, ClipAction::Start);
        assert!(reason(&no_match.once_per_run(0)).contains("match bytes past its selector"));

        let stray_selector = ClipPacketTrigger {
            selector_len: 1,
            ..held()
        };
        assert!(reason(&stray_selector).contains("only with once_per_run"));

        for direction in [Direction::With, Direction::Against] {
            let t = ClipPacketTrigger {
                direction,
                ..held()
            };
            assert!(matches!(
                validate_packet_trigger(&t),
                Err(Error::RelativeDirection {
                    what: "clip packet trigger",
                    ..
                })
            ));
        }
    }

    // A trigger no packet can match is refused, and so is the key of one on a removal.
    #[test]
    fn a_match_bit_outside_its_mask_is_refused() {
        for (m, k) in [
            (vec![0x07, 0x21], vec![0xFF, 0x20]),
            (vec![0x01], vec![0x00]),
            (vec![0x00, 0x80], vec![0xFF, 0x7F]),
        ] {
            let t = held().matching(m, k);
            assert!(reason(&t).contains("match bit outside its mask"), "{t:?}");
            assert!(matches!(
                validate_packet_key(&t),
                Err(Error::ClipPacketTrigger { .. })
            ));
        }
        // A masked-out byte with no match bit, and a mask wider than the match, are both fine.
        assert!(validate_packet_trigger(&held().matching([0x07, 0x00], [0xFF, 0x00])).is_ok());
        assert!(validate_packet_trigger(&held().matching([0x00, 0x00], [0xFF, 0xFF])).is_ok());
    }

    #[test]
    fn a_direction_the_class_never_carries_is_refused() {
        let on = |class, direction| {
            ClipPacketTrigger::new(class, 1, direction, ClipAction::Start).matching(M, K)
        };
        for (class, direction) in [
            (TrafficClass::HidIn, Direction::OUT),
            (TrafficClass::Emit, Direction::OUT),
            (TrafficClass::HidOut, Direction::IN),
        ] {
            let t = on(class, direction);
            assert!(
                reason(&t).contains("direction its class never carries"),
                "{t:?}"
            );
            assert!(validate_packet_key(&t).is_err(), "{t:?}");
        }
        for class in [
            TrafficClass::HidIn,
            TrafficClass::HidOut,
            TrafficClass::VendorInterrupt,
            TrafficClass::VendorBulk,
            TrafficClass::Control,
            TrafficClass::Emit,
        ] {
            assert!(
                validate_packet_trigger(&on(class, Direction::Both)).is_ok(),
                "{class:?}"
            );
        }
    }

    // The flows a packet can have: the mock's packet path and the refusal above read one table.
    #[test]
    fn a_packet_flows_in_or_out_across_a_surface_that_carries_it() {
        use Direction::{Against, Both, With};
        let (i, o) = (Direction::IN, Direction::OUT);
        for (class, carried) in [
            (TrafficClass::HidIn, vec![i]),
            (TrafficClass::HidOut, vec![o]),
            (TrafficClass::VendorInterrupt, vec![i, o]),
            (TrafficClass::VendorBulk, vec![i, o]),
            (TrafficClass::Control, vec![i, o]),
            (TrafficClass::Emit, vec![i]),
            (TrafficClass::Bus, vec![]),
            (TrafficClass::ClipTransfer, vec![]),
        ] {
            for direction in [Both, i, o, With, Against] {
                assert_eq!(
                    ClipPacketTrigger::class_carries(class, direction),
                    carried.contains(&direction),
                    "{class:?} {direction:?}"
                );
            }
        }
    }

    #[test]
    fn a_run_condition_with_no_masked_bit_is_refused() {
        let blind = held().matching([0x07, 0x00], [0xFF, 0x00]);
        assert!(
            validate_packet_trigger(&blind).is_ok(),
            "fine on every packet"
        );
        assert!(reason(&blind.clone().once_per_run(1)).contains("masked bit past its selector"));
        let all_clear = held().matching([0x00, 0x00], [0x00, 0x00]);
        assert!(reason(&all_clear.once_per_run(0)).contains("masked bit past its selector"));
        // One bit anywhere past the selector is a condition.
        let late = held().matching([0x07, 0x00, 0x20], [0xFF, 0x00, 0x20]);
        assert!(validate_packet_trigger(&late.clone().once_per_run(1)).is_ok());
        assert!(validate_packet_trigger(&late.once_per_run(2)).is_ok());
        // With no selector the report ID byte is itself the condition.
        assert!(validate_packet_trigger(&blind.once_per_run(0)).is_ok());
    }

    #[cfg(feature = "mock")]
    mod on_a_mock {
        use super::*;
        use crate::protocol::FrameType;
        use crate::{Device, MockBox};

        fn sent(mock: &MockBox) -> Vec<Vec<u8>> {
            mock.recorded_frames()
                .into_iter()
                .filter(|f| f.ty == FrameType::ClipTrigger)
                .map(|f| f.payload)
                .collect()
        }

        fn triggers(device: &Device) -> Vec<ClipPacketTriggerEntry> {
            device.clip().query_config().unwrap().packet_triggers
        }

        #[test]
        fn bind_and_unbind_send_the_wire_vectors() {
            let mock = MockBox::new();
            let device = Device::with_mock(mock.clone());
            let clip = device.clip();
            let t = held().consume().once_per_run(1);
            clip.bind_packet(&t).unwrap();
            clip.unbind_packet(&t).unwrap();
            clip.clear_triggers().unwrap();
            // A removal names the key alone: the verb, the flags and the selector go as zeros.
            let toggle = ClipPacketTrigger {
                action: ClipAction::Toggle,
                ..t
            };
            clip.unbind_packet(&toggle).unwrap();
            assert_eq!(
                sent(&mock),
                vec![
                    BIND.to_vec(),
                    REMOVE.to_vec(),
                    vec![0xFF, 0xFF, 0xFF, 0, 0, 0], // one sentinel clears both kinds
                    REMOVE.to_vec(),
                ]
            );
        }

        #[test]
        fn a_refused_trigger_sends_nothing_and_holds_nothing() {
            let mock = MockBox::new();
            let device = Device::with_mock(mock.clone());
            let clip = device.clip();
            let bus = ClipPacketTrigger::new(TrafficClass::Bus, 0, Direction::IN, ClipAction::Stop);
            for bad in [
                bus.clone(),
                held().matching([7; 17], [0xFF; 17]),
                held().matching([7], []),
                held().once_per_run(2),
                ClipPacketTrigger {
                    direction: Direction::With,
                    ..held()
                },
            ] {
                assert!(clip.bind_packet(&bad).is_err(), "{bad:?}");
            }
            assert!(sent(&mock).is_empty());
            assert!(device.link.desired().lock().is_idle());

            // A removal names a key, so only the key is checked.
            let flagged = ClipPacketTrigger {
                class: TrafficClass::Control,
                consume: true,
                selector_len: 9,
                ..held()
            };
            clip.unbind_packet(&flagged).unwrap();
            assert_eq!(sent(&mock).len(), 1);
            for bad_key in [
                bus,
                held().matching([7; 17], [0xFF; 17]),
                held().matching([7], []),
                ClipPacketTrigger {
                    direction: Direction::Against,
                    ..held()
                },
            ] {
                assert!(clip.unbind_packet(&bad_key).is_err(), "{bad_key:?}");
            }
            assert_eq!(sent(&mock).len(), 1);
        }

        #[test]
        fn a_bound_trigger_reads_back_and_unbinds_by_its_key() {
            let mock = MockBox::new().with_imperfect(true);
            let device = Device::with_mock(mock);
            let clip = device.clip();
            let run = held().consume().once_per_run(1);
            let plain = ClipPacketTrigger::new(
                TrafficClass::Control,
                ClipPacketTrigger::ANY_ID,
                Direction::Both,
                ClipAction::Pause,
            );
            clip.bind_packet(&run).unwrap();
            clip.bind_packet(&plain).unwrap();
            let read: Vec<_> = triggers(&device).into_iter().map(|e| e.trigger).collect();
            assert_eq!(read, vec![run.clone(), plain.clone()]);

            // Another mask, match length, direction or id is another key.
            for other in [
                held().matching(M, [0xFF, 0xFF]),
                held().matching([0x07], [0xFF]),
                ClipPacketTrigger {
                    direction: Direction::Both,
                    ..held()
                },
                ClipPacketTrigger { id: 3, ..held() },
                ClipPacketTrigger {
                    class: TrafficClass::Emit,
                    ..held()
                },
            ] {
                clip.unbind_packet(&other).unwrap();
            }
            assert_eq!(triggers(&device).len(), 2);
            // The key is all a removal reads: the verb and flags need not match.
            clip.unbind_packet(&held()).unwrap();
            let read: Vec<_> = triggers(&device).into_iter().map(|e| e.trigger).collect();
            assert_eq!(read, vec![plain]);
        }

        // How many packet triggers the mock answers with, read off the wire: the decode leaves out
        // an entry it has no names for.
        fn held_on_the_wire(device: &Device, mock: &MockBox) -> u8 {
            device.clip().query_config().unwrap();
            mock.replied_frames().pop().unwrap().payload[34]
        }

        // The three triggers no packet can match: the crate sends nothing, and the mock refuses the
        // frame when one reaches it.
        #[test]
        fn a_match_bit_outside_its_mask_reaches_neither_the_wire_nor_the_table() {
            let mock = MockBox::new();
            let device = Device::with_mock(mock.clone());
            let clip = device.clip();
            let stray = held().matching([0x07, 0x21], K);
            assert!(clip.bind_packet(&stray).is_err());
            assert!(clip.unbind_packet(&stray).is_err());
            assert!(sent(&mock).is_empty());
            assert!(device.link.desired().lock().is_idle());

            let frame = clip_packet_trigger_payload;
            let onto_the_link = |p: Vec<u8>| device.link.send(FrameType::ClipTrigger, &p).unwrap();
            onto_the_link(frame(4, 5, 1, 0, 1, 0, &[0x07, 0x21], &K));
            onto_the_link(frame(4, 5, 1, 0, 1, 0, &[0x00, 0x01], &[0x00, 0x00]));
            assert_eq!(held_on_the_wire(&device, &mock), 0);
            // The same address with the stray bit cleared is held as sent.
            onto_the_link(frame(4, 5, 1, 0, 1, 0, &M, &K));
            let read = triggers(&device);
            assert_eq!(read.len(), 1);
            assert_eq!(
                (&read[0].trigger.match_bytes[..], &read[0].trigger.mask[..]),
                (&M[..], &K[..])
            );
        }

        #[test]
        fn a_direction_the_class_never_carries_reaches_neither_the_wire_nor_the_table() {
            let mock = MockBox::new();
            let device = Device::with_mock(mock.clone());
            let clip = device.clip();
            for (class, direction) in [
                (TrafficClass::HidIn, Direction::OUT),
                (TrafficClass::Emit, Direction::OUT),
                (TrafficClass::HidOut, Direction::IN),
            ] {
                let t = ClipPacketTrigger::new(class, 5, direction, ClipAction::Start);
                assert!(clip.bind_packet(&t).is_err(), "{t:?}");
                assert!(clip.unbind_packet(&t).is_err(), "{t:?}");
            }
            assert!(sent(&mock).is_empty());
            assert!(device.link.desired().lock().is_idle());

            let frame = clip_packet_trigger_payload;
            let onto_the_link = |p: Vec<u8>| device.link.send(FrameType::ClipTrigger, &p).unwrap();
            onto_the_link(frame(4, 5, 2, 0, 1, 0, &[], &[])); // HID_IN, OUT
            onto_the_link(frame(9, 5, 2, 0, 1, 0, &[], &[])); // EMIT, OUT
            onto_the_link(frame(5, 5, 1, 0, 1, 0, &[], &[])); // HID_OUT, IN
            assert_eq!(held_on_the_wire(&device, &mock), 0);
            onto_the_link(frame(5, 5, 2, 0, 1, 0, &[], &[])); // HID_OUT, OUT
            onto_the_link(frame(6, 5, 2, 0, 1, 0, &[], &[])); // VEND_INTR, OUT
            onto_the_link(frame(6, 5, 1, 0, 1, 0, &[], &[]));
            onto_the_link(frame(4, 5, 1, 0, 1, 0, &[], &[])); // HID_IN, IN
            onto_the_link(frame(9, 5, 1, 0, 1, 0, &[], &[]));
            onto_the_link(frame(4, 5, 0, 0, 1, 0, &[], &[])); // both flows, on any class
            onto_the_link(frame(5, 5, 0, 0, 1, 0, &[], &[]));
            onto_the_link(frame(9, 5, 0, 0, 1, 0, &[], &[]));
            assert_eq!(held_on_the_wire(&device, &mock), 8);
        }

        #[test]
        fn a_run_condition_with_no_masked_bit_reaches_neither_the_wire_nor_the_table() {
            let mock = MockBox::new();
            let device = Device::with_mock(mock.clone());
            let clip = device.clip();
            let blind = held().matching([0x07, 0x00], [0xFF, 0x00]);
            assert!(clip.bind_packet(&blind.clone().once_per_run(1)).is_err());
            assert!(sent(&mock).is_empty());
            assert!(device.link.desired().lock().is_idle());

            let frame = clip_packet_trigger_payload;
            let onto_the_link = |p: Vec<u8>| device.link.send(FrameType::ClipTrigger, &p).unwrap();
            onto_the_link(frame(4, 2, 1, 0, 0x05, 1, &[0x07, 0x00], &[0xFF, 0x00]));
            onto_the_link(frame(4, 2, 1, 0, 0x05, 0, &[0x00, 0x00], &[0x00, 0x00]));
            assert_eq!(held_on_the_wire(&device, &mock), 0);
            // On every packet it is a trigger like any other, and a run refused onto its key leaves it.
            clip.bind_packet(&blind).unwrap();
            onto_the_link(frame(4, 2, 1, 1, 0x05, 1, &[0x07, 0x00], &[0xFF, 0x00]));
            let read = triggers(&device);
            assert_eq!(read.len(), 1);
            assert_eq!(read[0].trigger, blind);
            // One masked bit past the selector is a condition.
            onto_the_link(frame(
                4,
                3,
                1,
                0,
                0x05,
                1,
                &[0x07, 0x00, 0x20],
                &[0xFF, 0x00, 0x20],
            ));
            assert_eq!(held_on_the_wire(&device, &mock), 2);
        }

        // A packet travels IN or OUT across a surface that carries that flow. Anything else is no
        // packet, whatever trigger would have taken it.
        #[test]
        fn the_mock_runs_only_a_packet_that_can_exist() {
            let mock = MockBox::new().with_imperfect(true);
            let device = Device::with_mock(mock.clone());
            let clip = device.clip();
            let either =
                |class, action| ClipPacketTrigger::new(class, 1, Direction::Both, action).consume();
            clip.bind_packet(&either(TrafficClass::VendorInterrupt, ClipAction::Start))
                .unwrap();
            clip.bind_packet(&either(TrafficClass::HidIn, ClipAction::Stop))
                .unwrap();
            clip.bind_packet(&either(TrafficClass::HidOut, ClipAction::Pause))
                .unwrap();
            for (class, direction) in [
                (TrafficClass::VendorInterrupt, Direction::Both),
                (TrafficClass::VendorInterrupt, Direction::With),
                (TrafficClass::VendorInterrupt, Direction::Against),
                (TrafficClass::HidIn, Direction::OUT),
                (TrafficClass::HidIn, Direction::Both),
                (TrafficClass::HidOut, Direction::IN),
            ] {
                assert_eq!(
                    mock.clip_packet(class, 1, direction, &[]),
                    (None, false),
                    "{class:?} {direction:?}"
                );
            }
            let hits: Vec<u16> = triggers(&device).iter().map(|e| e.hits).collect();
            assert_eq!(hits, [0, 0, 0]);

            let vint = TrafficClass::VendorInterrupt;
            assert_eq!(
                mock.clip_packet(vint, 1, Direction::IN, &[]),
                (Some(ClipAction::Start), true)
            );
            assert_eq!(
                mock.clip_packet(vint, 1, Direction::OUT, &[]),
                (Some(ClipAction::Start), true)
            );
            assert_eq!(
                mock.clip_packet(TrafficClass::HidIn, 1, Direction::IN, &[]),
                (Some(ClipAction::Stop), true)
            );
            assert_eq!(
                mock.clip_packet(TrafficClass::HidOut, 1, Direction::OUT, &[]),
                (Some(ClipAction::Pause), true)
            );
            let hits: Vec<u16> = triggers(&device).iter().map(|e| e.hits).collect();
            assert_eq!(hits, [2, 1, 1]);
        }

        #[test]
        fn every_surface_holds_a_trigger() {
            let device = Device::with_mock(MockBox::new());
            let surfaces = [
                TrafficClass::HidIn,
                TrafficClass::HidOut,
                TrafficClass::VendorInterrupt,
                TrafficClass::VendorBulk,
                TrafficClass::Control,
                TrafficClass::Emit,
            ];
            for class in surfaces {
                let t = ClipPacketTrigger::new(class, 1, Direction::Both, ClipAction::Toggle);
                device.clip().bind_packet(&t).unwrap();
            }
            let read: Vec<_> = triggers(&device)
                .into_iter()
                .map(|e| e.trigger.class)
                .collect();
            assert_eq!(read, surfaces);
        }

        // Past the crate's own check, straight onto the link, where the mock refuses what the box does.
        #[test]
        fn the_mock_refuses_what_the_box_does() {
            let mock = MockBox::new().with_imperfect(true);
            let device = Device::with_mock(mock.clone());
            let kept = held();
            device.clip().bind_packet(&kept).unwrap();
            let frame = clip_packet_trigger_payload;
            let mut cut = frame(4, 3, 1, 0, 1, 0, &M, &K);
            cut.pop();
            let mut header_only = frame(4, 3, 1, 0, 1, 0, &[], &[]);
            header_only.pop();
            for bad in [
                frame(3, 3, 1, 0, 1, 0, &M, &K),         // an input class
                frame(10, 0, 1, 0, 1, 0, &M, &K),        // the bus class
                frame(11, 0, 1, 0, 1, 0, &M, &K),        // a clip's transfers
                frame(0xFF, 0xFFFF, 0, 0, 1, 0, &M, &K), // no packet is class ANY
                frame(4, 3, 3, 0, 1, 0, &M, &K),         // a bearing-relative direction
                frame(4, 3, 1, 0, 1, 0, &[7; 17], &[0xFF; 17]),
                frame(4, 3, 1, 6, 1, 0, &M, &K), // CLEAR is a host verb
                frame(4, 3, 1, 0, 0x09, 0, &M, &K), // a reserved flag
                frame(8, 0, 1, 0, 0x03, 0, &M, &K), // consume on CONTROL
                frame(8, 0, 1, 0, 0x05, 1, &M, &K), // RUN on CONTROL
                frame(4, 0xFFFF, 1, 0, 0x05, 1, &M, &K), // RUN on every id
                frame(4, 3, 0, 0, 0x05, 1, &M, &K), // RUN on both flows
                frame(4, 3, 1, 0, 0x05, 2, &M, &K), // the selector is the whole match
                frame(4, 3, 1, 0, 0x05, 3, &M, &K),
                frame(4, 3, 1, 0, 0x05, 0, &[], &[]), // RUN with nothing to match
                frame(4, 3, 1, 0, 0x01, 1, &M, &K),   // a selector without RUN
                cut,                                  // a mask cut short
                header_only,
                frame(4, 2, 1, 6, 1, 0, &M, &K), // onto the held key: it stays
                frame(4, 2, 1, 1, 0x05, 2, &M, &K),
            ] {
                device.link.send(FrameType::ClipTrigger, &bad).unwrap();
            }
            let read = triggers(&device);
            assert_eq!(read.len(), 1, "{read:?}");
            assert_eq!(read[0].trigger, kept);
            // The count on the wire too: the decode leaves out an entry it has no names for, so a
            // stored relative direction would not show in `read`.
            let reply = mock.replied_frames().pop().unwrap().payload;
            assert_eq!((reply[34], reply.len()), (1, 35 + 10 + 4));
        }

        // Consuming a packet is dropping traffic, which needs `OPTION(IMPERFECT)`. Watching one does not.
        #[test]
        fn a_consuming_trigger_needs_the_opt_in_and_goes_with_it() {
            let mock = MockBox::new();
            let device = Device::with_mock(mock);
            let clip = device.clip();
            let watching = held();
            let consuming = ClipPacketTrigger { id: 3, ..held() }.consume();
            clip.bind_packet(&consuming).unwrap();
            clip.bind_packet(&watching).unwrap();
            let read: Vec<_> = triggers(&device).into_iter().map(|e| e.trigger).collect();
            assert_eq!(read, vec![watching.clone()], "refused with the opt-in off");
            // An overwrite that adds consume is refused too, and the trigger it named stays with its
            // own action and flags: the read-back differs from what was bound.
            let rebound = ClipPacketTrigger {
                action: ClipAction::Stop,
                ..watching.clone().consume()
            };
            clip.bind_packet(&rebound).unwrap();
            let read = triggers(&device);
            assert_eq!(read.len(), 1);
            assert_eq!(read[0].trigger, watching);
            assert_ne!(read[0].trigger, rebound);

            device.allow_imperfect_clones(true).unwrap();
            clip.bind_packet(&consuming).unwrap();
            assert_eq!(triggers(&device).len(), 2);
            device.allow_imperfect_clones(false).unwrap();
            let read: Vec<_> = triggers(&device).into_iter().map(|e| e.trigger).collect();
            assert_eq!(read, vec![watching]);
        }

        // The crate stops holding what the box drops when the opt-in goes off, and puts it back when
        // the frame never went out.
        #[test]
        fn the_opt_in_going_off_leaves_only_the_watching_triggers_held() {
            use crate::transport::Disconnected;
            use std::sync::Arc;

            let mock = MockBox::new().with_imperfect(true);
            let device = Device::with_mock(mock.clone());
            let clip = device.clip();
            let idle = || device.link.desired().lock().is_idle();
            let watching = held();
            let consuming = ClipPacketTrigger { id: 3, ..held() }.consume();
            clip.bind_packet(&consuming).unwrap();
            clip.bind_packet(&watching).unwrap();

            device.allow_imperfect_clones(false).unwrap();
            let read: Vec<_> = triggers(&device).into_iter().map(|e| e.trigger).collect();
            assert_eq!(read, vec![watching.clone()]);
            assert!(!idle(), "the watching trigger is held");
            clip.unbind_packet(&watching).unwrap();
            assert!(idle(), "and nothing else is");

            // Only consuming triggers: the opt-out leaves nothing to keep alive.
            device.allow_imperfect_clones(true).unwrap();
            clip.bind_packet(&consuming).unwrap();
            assert!(!idle());
            device.allow_imperfect_clones(false).unwrap();
            assert!(idle());

            // An opt-out that never reached the box leaves the consuming trigger held, as the box
            // still holds it.
            device.allow_imperfect_clones(true).unwrap();
            clip.bind_packet(&consuming).unwrap();
            device.link.transport_slot().swap(Arc::new(Disconnected));
            assert!(device.allow_imperfect_clones(false).is_err());
            assert!(!idle());
            device.link.transport_slot().swap(mock.transport());
            clip.unbind_packet(&consuming).unwrap();
            assert!(idle());
        }

        // A scripted set is held as a box holds it: bound in order, under the opt-in as scripted.
        #[test]
        fn a_script_holds_only_what_the_box_would_take() {
            let entry = |trigger: ClipPacketTrigger| ClipPacketTriggerEntry { trigger, hits: 5 };
            let script = |imperfect: bool, t: ClipPacketTrigger| {
                let mock =
                    MockBox::new()
                        .with_imperfect(imperfect)
                        .with_clip_settings(ClipSettings {
                            packet_triggers: vec![entry(held()), entry(t)],
                            ..ClipSettings::default()
                        });
                let device = Device::with_mock(mock.clone());
                // The count on the wire as well: the decode leaves out an entry it has no names for.
                let on_the_wire = held_on_the_wire(&device, &mock);
                let read = triggers(&device);
                assert_eq!(on_the_wire as usize, read.len());
                read
            };
            let other = ClipPacketTrigger { id: 3, ..held() };
            let control =
                ClipPacketTrigger::new(TrafficClass::Control, 0, Direction::IN, ClipAction::Start);
            let blind = other.clone().matching([0x07, 0x00], [0xFF, 0x00]);
            for (imperfect, refused) in [
                (
                    true,
                    ClipPacketTrigger {
                        direction: Direction::OUT,
                        ..other.clone()
                    },
                ),
                (
                    true,
                    ClipPacketTrigger::new(
                        TrafficClass::HidOut,
                        1,
                        Direction::IN,
                        ClipAction::Stop,
                    ),
                ),
                (true, other.clone().matching([0x07, 0x21], K)),
                (true, blind.clone().once_per_run(1)),
                (true, control.clone().consume()),
                (false, other.clone().consume()),
                (
                    true,
                    ClipPacketTrigger {
                        class: TrafficClass::Bus,
                        ..other.clone()
                    },
                ),
                (true, other.clone().matching([0x07, 0x20], [0xFF])),
            ] {
                let read = script(imperfect, refused.clone());
                assert_eq!(read, vec![entry(held())], "{refused:?}");
            }
            // The shapes the box takes are held, with their scripted count.
            for taken in [other.consume(), control, blind] {
                let read = script(true, taken.clone());
                assert_eq!(read, vec![entry(held()), entry(taken)]);
            }
        }

        #[test]
        fn a_scripted_opt_out_drops_the_consuming_triggers() {
            let consuming = ClipPacketTrigger { id: 3, ..held() }.consume();
            let scripted = ClipSettings {
                packet_triggers: [held(), consuming]
                    .map(|trigger| ClipPacketTriggerEntry { trigger, hits: 0 })
                    .to_vec(),
                ..ClipSettings::default()
            };
            let read = |mock: &MockBox| {
                let held = triggers(&Device::with_mock(mock.clone()));
                held.into_iter().map(|e| e.trigger).collect::<Vec<_>>()
            };
            let mock = MockBox::new()
                .with_imperfect(true)
                .with_clip_settings(scripted.clone());
            assert_eq!(read(&mock).len(), 2);
            mock.set_imperfect_status(crate::ImperfectStatus::default());
            assert_eq!(read(&mock), vec![held()]);

            let mock = MockBox::new()
                .with_imperfect(true)
                .with_clip_settings(scripted.clone())
                .with_imperfect_status(crate::ImperfectStatus::default());
            assert_eq!(read(&mock), vec![held()]);
            let mock = MockBox::new()
                .with_imperfect(true)
                .with_clip_settings(scripted)
                .with_imperfect(false);
            assert_eq!(read(&mock), vec![held()]);
        }

        // A removal names a key. The box reads nothing else of the frame, so a removal whose verb,
        // flags and selector are not zero still removes.
        #[test]
        fn a_removal_reads_only_the_key() {
            let mock = MockBox::new();
            let device = Device::with_mock(mock);
            device.clip().bind_packet(&held()).unwrap();
            let removal = clip_packet_trigger_payload(4, 2, 1, 5, 0x0E, 3, &M, &K);
            device.link.send(FrameType::ClipTrigger, &removal).unwrap();
            assert!(triggers(&device).is_empty());
        }

        // An overwrite that changes the selector alone is an overwrite: the count and the run start
        // again.
        #[test]
        fn an_overwrite_of_the_selector_alone_restarts_the_count_and_the_run() {
            let mock = MockBox::new();
            let device = Device::with_mock(mock.clone());
            let clip = device.clip();
            let run = ClipPacketTrigger {
                class: TrafficClass::VendorInterrupt,
                ..held()
            };
            let fired = || {
                mock.clip_packet(
                    TrafficClass::VendorInterrupt,
                    2,
                    Direction::IN,
                    &[0x07, 0x20],
                )
                .0
            };
            clip.bind_packet(&run.clone().once_per_run(1)).unwrap();
            assert_eq!(fired(), Some(ClipAction::Start));
            assert_eq!(fired(), None);
            clip.bind_packet(&run.once_per_run(0)).unwrap();
            let read = triggers(&device);
            assert_eq!(read[0].trigger.selector_len, 0);
            assert_eq!(read[0].hits, 0);
            assert_eq!(fired(), Some(ClipAction::Start));
        }

        // A named direction outranks Both by less than one masked bit.
        #[test]
        fn one_more_masked_bit_beats_a_named_direction() {
            let mock = MockBox::new();
            let device = Device::with_mock(mock.clone());
            let on = |direction, action| {
                ClipPacketTrigger::new(TrafficClass::VendorInterrupt, 1, direction, action)
            };
            let named = on(Direction::IN, ClipAction::Start).matching([0x07], [0xFF]);
            let wider = on(Direction::Both, ClipAction::Stop).matching([0x07, 0x00], [0xFF, 0x01]);
            device.clip().bind_packet(&named).unwrap();
            device.clip().bind_packet(&wider).unwrap();
            let head = [0x07, 0x00];
            assert_eq!(
                mock.clip_packet(TrafficClass::VendorInterrupt, 1, Direction::IN, &head)
                    .0,
                Some(ClipAction::Stop)
            );
        }

        // Removing a trigger keeps the order of the rest, which is both the read-back order and the
        // tie-break between equally specific triggers.
        #[test]
        fn a_removal_keeps_the_order_of_the_rest() {
            let mock = MockBox::new();
            let device = Device::with_mock(mock.clone());
            let clip = device.clip();
            let bit = |b: u8, action| {
                ClipPacketTrigger::new(TrafficClass::VendorInterrupt, 1, Direction::IN, action)
                    .matching([b], [b])
            };
            let (a, b, c, d) = (
                bit(0x01, ClipAction::Start),
                bit(0x02, ClipAction::Stop),
                bit(0x04, ClipAction::Pause),
                bit(0x08, ClipAction::Resume),
            );
            for t in [&a, &b, &c, &d] {
                clip.bind_packet(t).unwrap();
            }
            clip.unbind_packet(&b).unwrap();
            let read: Vec<_> = triggers(&device).into_iter().map(|e| e.trigger).collect();
            assert_eq!(read, vec![a, c, d]);
            assert_eq!(
                mock.clip_packet(TrafficClass::VendorInterrupt, 1, Direction::IN, &[0x0C])
                    .0,
                Some(ClipAction::Pause)
            );
        }

        #[test]
        fn the_box_holds_eight_triggers_and_a_pool_of_match_bytes() {
            let mock = MockBox::new();
            let device = Device::with_mock(mock);
            let clip = device.clip();
            let wide = |id: u16, action| {
                ClipPacketTrigger::new(TrafficClass::HidIn, id, Direction::IN, action)
                    .matching([0x55; 16], [0xFF; 16])
            };
            let bare = |id: u16| {
                ClipPacketTrigger::new(TrafficClass::HidIn, id, Direction::IN, ClipAction::Start)
            };
            for id in 0..7 {
                clip.bind_packet(&wide(id, ClipAction::Start)).unwrap();
            }
            assert_eq!(triggers(&device).len(), 7); // 7 x 16 = 112
            clip.bind_packet(&bare(7).matching([0x55], [0xFF])).unwrap();
            assert_eq!(triggers(&device).len(), 7, "the pool is spent");
            clip.bind_packet(&bare(7)).unwrap();
            assert_eq!(triggers(&device).len(), 8);
            clip.bind_packet(&bare(8)).unwrap();
            assert_eq!(triggers(&device).len(), 8, "and so is the table");
            // An overwrite takes no pool bytes.
            clip.bind_packet(&wide(3, ClipAction::Stop)).unwrap();
            let read = triggers(&device);
            assert_eq!(read.len(), 8);
            assert_eq!(read[3].trigger, wide(3, ClipAction::Stop));
            clip.unbind_packet(&wide(0, ClipAction::Start)).unwrap();
            clip.bind_packet(&wide(8, ClipAction::Start)).unwrap();
            let read = triggers(&device);
            assert_eq!(read.len(), 8, "room again");
            assert_eq!(read[7].trigger, wide(8, ClipAction::Start));
        }

        // pkt_match_score's order, case for case with test_clip_ptrig.c, on a class that carries both
        // flows.
        #[test]
        fn the_most_specific_trigger_wins_a_packet() {
            let mock = MockBox::new();
            let device = Device::with_mock(mock.clone());
            let clip = device.clip();
            let on = |id, direction, action| {
                ClipPacketTrigger::new(TrafficClass::VendorInterrupt, id, direction, action)
            };
            let fired = |id: u16, head: &[u8]| {
                mock.clip_packet(TrafficClass::VendorInterrupt, id, Direction::IN, head)
                    .0
            };
            let any = ClipPacketTrigger::ANY_ID;
            let held_report = [0x07, 0x21];

            assert_eq!(fired(2, &held_report), None);
            clip.bind_packet(&on(any, Direction::Both, ClipAction::Start))
                .unwrap();
            assert_eq!(fired(2, &held_report), Some(ClipAction::Start));
            clip.bind_packet(&on(2, Direction::Both, ClipAction::Stop))
                .unwrap();
            assert_eq!(fired(2, &held_report), Some(ClipAction::Stop)); // an exact id beats the wildcard
            assert_eq!(fired(3, &held_report), Some(ClipAction::Start));
            clip.bind_packet(&on(2, Direction::IN, ClipAction::Pause))
                .unwrap();
            assert_eq!(fired(2, &held_report), Some(ClipAction::Pause)); // a named direction beats both
            clip.bind_packet(&on(2, Direction::Both, ClipAction::Resume).matching([0x07], [0xFF]))
                .unwrap();
            assert_eq!(fired(2, &held_report), Some(ClipAction::Resume)); // masked bits beat a direction
            clip.bind_packet(&on(2, Direction::Both, ClipAction::Restart).matching([0x06], [0xFE]))
                .unwrap();
            assert_eq!(fired(2, &held_report), Some(ClipAction::Resume)); // eight bits beat seven
            assert_eq!(fired(2, &[0x06]), Some(ClipAction::Restart));
            // A masked wildcard still loses to a bare exact id: the id ranks above the bits.
            clip.bind_packet(
                &on(any, Direction::IN, ClipAction::Toggle).matching([0x09; 16], [0xFF; 16]),
            )
            .unwrap();
            assert_eq!(fired(2, &[0x09; 16]), Some(ClipAction::Pause));
            assert_eq!(fired(5, &[0x09; 16]), Some(ClipAction::Toggle));

            // Another class, another flow and a head shorter than the match are no candidates.
            assert_eq!(
                mock.clip_packet(TrafficClass::Emit, 2, Direction::IN, &held_report),
                (None, false)
            );
            assert_eq!(
                mock.clip_packet(TrafficClass::VendorInterrupt, 2, Direction::OUT, &[0x06])
                    .0,
                Some(ClipAction::Restart),
                "a trigger on both flows takes either"
            );
            assert_eq!(
                mock.clip_packet(TrafficClass::VendorInterrupt, 2, Direction::OUT, &[])
                    .0,
                Some(ClipAction::Stop),
                "the IN trigger is no candidate for an OUT packet"
            );

            let hits: Vec<u16> = triggers(&device).iter().map(|e| e.hits).collect();
            assert_eq!(
                hits,
                vec![2, 2, 2, 2, 2, 1],
                "each win is charged to its winner"
            );
        }

        #[test]
        fn the_earlier_of_two_equally_specific_triggers_wins() {
            let mock = MockBox::new();
            let device = Device::with_mock(mock.clone());
            let on = |action, m: u8, k: u8| {
                ClipPacketTrigger::new(TrafficClass::HidIn, 2, Direction::IN, action)
                    .matching([m], [k])
            };
            device
                .clip()
                .bind_packet(&on(ClipAction::Start, 0x07, 0x0F))
                .unwrap();
            device
                .clip()
                .bind_packet(&on(ClipAction::Stop, 0x20, 0xF0))
                .unwrap();
            assert_eq!(
                mock.clip_packet(TrafficClass::HidIn, 2, Direction::IN, &[0x27]),
                (Some(ClipAction::Start), false)
            );
            let hits: Vec<u16> = triggers(&device).iter().map(|e| e.hits).collect();
            assert_eq!(hits, vec![1, 0]);
        }

        // once_per_run, case for case with test_clip_ptrig.c: the selector keeps another report ID
        // on the same interface out of the run.
        #[test]
        fn a_run_fires_once_and_its_selector_picks_the_stream() {
            let mock = MockBox::new();
            let device = Device::with_mock(mock.clone());
            // On a vendor endpoint, where a packet in the other flow is a packet.
            let run = ClipPacketTrigger {
                class: TrafficClass::VendorInterrupt,
                ..held()
            };
            device.clip().bind_packet(&run.once_per_run(1)).unwrap();
            let fired =
                |class, id, direction, head: &[u8]| mock.clip_packet(class, id, direction, head).0;
            let vint = TrafficClass::VendorInterrupt;
            let (down, up, other) = ([0x07, 0x21], [0x07, 0x01], [0x03, 0xFF]);
            let start = Some(ClipAction::Start);

            assert_eq!(fired(vint, 2, Direction::IN, &down), start);
            assert_eq!(fired(vint, 2, Direction::IN, &down), None);
            assert_eq!(fired(vint, 2, Direction::IN, &other), None); // fails the selector
            assert_eq!(fired(vint, 2, Direction::IN, &down), None); // so the run goes on
            assert_eq!(fired(vint, 5, Direction::IN, &up), None); // another endpoint
            assert_eq!(fired(vint, 2, Direction::OUT, &up), None); // another flow
            assert_eq!(fired(TrafficClass::Emit, 2, Direction::IN, &up), None); // another class
            assert_eq!(fired(vint, 2, Direction::IN, &down), None);
            assert_eq!(fired(vint, 2, Direction::IN, &up), None); // the run ends here
            assert_eq!(fired(vint, 2, Direction::IN, &down), start);
            assert_eq!(fired(vint, 2, Direction::IN, &down[..1]), None); // selector passes, no condition byte
            assert_eq!(fired(vint, 2, Direction::IN, &down), start);
            assert_eq!(triggers(&device)[0].hits, 6, "wins, not firings");
        }

        #[test]
        fn a_shadowed_trigger_still_tracks_its_run() {
            let mock = MockBox::new();
            let device = Device::with_mock(mock.clone());
            let clip = device.clip();
            clip.bind_packet(&held().once_per_run(1)).unwrap();
            let narrower =
                ClipPacketTrigger::new(TrafficClass::HidIn, 2, Direction::IN, ClipAction::Stop)
                    .matching([0x07, 0x21], [0xFF, 0x21]);
            clip.bind_packet(&narrower).unwrap();
            let fired = |head: &[u8]| {
                mock.clip_packet(TrafficClass::HidIn, 2, Direction::IN, head)
                    .0
            };
            assert_eq!(fired(&[0x07, 0x21]), Some(ClipAction::Stop));
            assert_eq!(triggers(&device)[0].hits, 0);
            assert_eq!(fired(&[0x07, 0x20]), None, "it wins now, mid-run");
            assert_eq!(fired(&[0x07, 0x01]), None);
            assert_eq!(fired(&[0x07, 0x20]), Some(ClipAction::Start));
        }

        // An identical re-bind keeps the run and the count. An overwrite starts both again.
        #[test]
        fn a_rebind_keeps_the_run_and_an_overwrite_restarts_it() {
            let mock = MockBox::new();
            let device = Device::with_mock(mock.clone());
            let clip = device.clip();
            let run = held().once_per_run(1);
            let fired = || {
                mock.clip_packet(TrafficClass::HidIn, 2, Direction::IN, &[0x07, 0x21])
                    .0
            };
            clip.bind_packet(&run).unwrap();
            assert_eq!(fired(), Some(ClipAction::Start));
            clip.bind_packet(&run).unwrap();
            assert_eq!(triggers(&device)[0].hits, 1);
            assert_eq!(fired(), None);

            for overwrite in [
                ClipPacketTrigger {
                    action: ClipAction::Stop,
                    ..run.clone()
                },
                ClipPacketTrigger {
                    selector_len: 0,
                    ..run.clone()
                },
                held(),
            ] {
                clip.bind_packet(&overwrite).unwrap();
                let read = triggers(&device);
                assert_eq!(read.len(), 1);
                assert_eq!(read[0].trigger, overwrite);
                assert_eq!(read[0].hits, 0, "{overwrite:?}");
                assert_eq!(fired(), Some(overwrite.action));
            }
        }

        // consume takes every packet the trigger wins, whether or not the verb runs on it.
        #[test]
        fn a_consuming_trigger_takes_every_packet_it_wins() {
            let mock = MockBox::new().with_imperfect(true);
            let device = Device::with_mock(mock.clone());
            let clip = device.clip();
            let vendor = ClipPacketTrigger {
                class: TrafficClass::VendorInterrupt,
                id: 1,
                ..held()
            };
            clip.bind_packet(&vendor.clone().consume().once_per_run(1))
                .unwrap();
            clip.bind_packet(&held()).unwrap();
            let packet = |class, id, head: &[u8]| mock.clip_packet(class, id, Direction::IN, head);
            let vint = TrafficClass::VendorInterrupt;
            assert_eq!(
                packet(vint, 1, &[0x07, 0x21]),
                (Some(ClipAction::Start), true)
            );
            assert_eq!(packet(vint, 1, &[0x07, 0x21]), (None, true), "mid-run");
            assert_eq!(packet(vint, 1, &[0x07, 0x01]), (None, false));
            assert_eq!(
                packet(TrafficClass::HidIn, 2, &[0x07, 0x21]),
                (Some(ClipAction::Start), false)
            );
        }

        #[test]
        fn a_hit_count_saturates_at_its_field() {
            let scripted = ClipSettings {
                packet_triggers: vec![ClipPacketTriggerEntry {
                    trigger: held(),
                    hits: 0xFFFE,
                }],
                ..ClipSettings::default()
            };
            let mock = MockBox::new().with_clip_settings(scripted);
            let device = Device::with_mock(mock.clone());
            for want in [0xFFFF, 0xFFFF, 0xFFFF] {
                mock.clip_packet(TrafficClass::HidIn, 2, Direction::IN, &[0x07, 0x20]);
                assert_eq!(triggers(&device)[0].hits, want);
            }
        }

        // The clear-all sentinel drops both kinds, and the config goes with the box's soft state.
        #[test]
        fn clear_triggers_and_a_reset_drop_both_kinds() {
            let input = ClipTrigger::new(Button::SIDE1, Edge::Press, ClipAction::Start);
            let scripted = ClipSettings {
                retain: true,
                triggers: vec![input],
                ..ClipSettings::default()
            };
            let mock = MockBox::new().with_clip_settings(scripted.clone());
            let device = Device::with_mock(mock.clone());
            let clip = device.clip();
            clip.bind_packet(&held()).unwrap();
            let cfg = clip.query_config().unwrap();
            assert_eq!((cfg.triggers.len(), cfg.packet_triggers.len()), (1, 1));

            // A binding frame that is near the sentinel is a binding frame.
            for near in [
                [0xFF, 0xFF, 0xFF, 0, 0, 1],
                [0xFF, 0xFF, 0xFF, 1, 0, 0],
                [0xFF, 0x00, 0x00, 0, 0, 0],
                [0x00, 0xFF, 0xFF, 0, 0, 0],
            ] {
                device.link.send(FrameType::ClipTrigger, &near).unwrap();
            }
            let cfg = clip.query_config().unwrap();
            assert_eq!((cfg.triggers.len(), cfg.packet_triggers.len()), (1, 1));

            clip.clear_triggers().unwrap();
            let cfg = clip.query_config().unwrap();
            assert!(cfg.triggers.is_empty() && cfg.packet_triggers.is_empty());
            assert!(
                cfg.retain,
                "the sentinel clears the triggers and leaves the settings"
            );

            mock.set_clip_settings(scripted);
            clip.bind_packet(&held()).unwrap();
            device.reset().unwrap();
            assert_eq!(clip.query_config().unwrap(), ClipSettings::default());
        }
    }
}
