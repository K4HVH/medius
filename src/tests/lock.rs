//! LOCK command (§3.8): payload bytes, target/direction wire, scale weighing, `RESP(LOCKS)` decode, and the HEALTH `lock_on` bit.

use crate::protocol::command::lock_payload;
use crate::protocol::opcode::{
    LOCK_CLS_AXIS, LOCK_CLS_KEY, LOCK_CLS_MEDIA, LOCK_DIR_AGAINST, LOCK_DIR_BOTH, LOCK_DIR_NEG,
    LOCK_DIR_POS, LOCK_DIR_WITH, LOCK_ID_ALL, LOCK_SCALE_BLOCK, LOCK_SCALE_MAX, LOCK_SCALE_PASS,
};
use crate::protocol::{Resp, parse_resp};
use crate::types::{
    Axis, Button, Class, Direction, Health, Key, LockScope, LockTarget, Locks, Usage,
};

#[test]
fn lock_payload_bytes() {
    assert_eq!(
        lock_payload(LOCK_CLS_AXIS, 2, LOCK_DIR_NEG, LOCK_SCALE_BLOCK),
        [3, 2, 0, 2, 0]
    );
    assert_eq!(
        lock_payload(LOCK_CLS_MEDIA, 0x00E9, LOCK_DIR_BOTH, LOCK_SCALE_BLOCK),
        [2, 0xE9, 0x00, 0, 0]
    );
    assert_eq!(
        lock_payload(LOCK_CLS_KEY, LOCK_ID_ALL, LOCK_DIR_BOTH, LOCK_SCALE_BLOCK),
        [1, 0xFF, 0xFF, 0, 0]
    );
    // The scale rides the byte that used to be a state, so an unlock and a weighing both land here.
    assert_eq!(
        lock_payload(LOCK_CLS_AXIS, 0, LOCK_DIR_AGAINST, 40),
        [3, 0, 0, LOCK_DIR_AGAINST, 40]
    );
    assert_eq!(
        lock_payload(LOCK_CLS_AXIS, 0, LOCK_DIR_BOTH, LOCK_SCALE_PASS),
        [3, 0, 0, 0, 100]
    );
}

#[test]
fn lock_scale_constants() {
    assert_eq!(
        (LOCK_SCALE_BLOCK, LOCK_SCALE_PASS, LOCK_SCALE_MAX),
        (0, 100, 255)
    );
}

#[test]
fn lock_target_and_direction_wire() {
    assert_eq!(
        (Axis::X.as_u16(), Axis::Y.as_u16(), Axis::Wheel.as_u16()),
        (0, 1, 2)
    );
    let bt: LockTarget = Button::Left.into();
    assert_eq!(bt, LockTarget::Usage(Usage::new(Class::Button, 0)));
    let at: LockTarget = Axis::Wheel.into();
    assert_eq!(at, LockTarget::Axis(Axis::Wheel));

    assert_eq!(
        (
            Direction::Both.as_u8(),
            Direction::Positive.as_u8(),
            Direction::Negative.as_u8(),
            Direction::With.as_u8(),
            Direction::Against.as_u8(),
        ),
        (0, 1, 2, 3, 4)
    );
    for d in [
        Direction::Both,
        Direction::Positive,
        Direction::Negative,
        Direction::With,
        Direction::Against,
    ] {
        assert_eq!(Direction::from_u8(d.as_u8()), Some(d));
    }
    assert_eq!(Direction::from_u8(5), None);
}

#[test]
fn only_the_bearing_relative_directions_are_relative() {
    assert!(Direction::With.is_relative() && Direction::Against.is_relative());
    assert!(!Direction::Both.is_relative());
    assert!(!Direction::Positive.is_relative() && !Direction::Negative.is_relative());
}

#[test]
fn locks_list_decode() {
    let l = Locks::from_payload(&[
        6,
        2,
        3,
        0,
        0,
        LOCK_DIR_POS,
        LOCK_SCALE_BLOCK,
        1,
        0x04,
        0x00,
        LOCK_DIR_NEG,
        LOCK_SCALE_BLOCK,
    ])
    .unwrap();
    assert_eq!(l.entries().len(), 2);
    assert!(l.is_locked(Axis::X, Direction::Positive));
    assert!(!l.is_locked(Axis::X, Direction::Negative));
    assert!(l.is_locked(Key::A, Direction::Negative));
}

#[test]
fn locks_report_a_partial_scale_as_weighed_not_locked() {
    let l = Locks::from_payload(&[6, 1, 3, 0, 0, LOCK_DIR_AGAINST, 40]).unwrap();
    let e = l.entries()[0];
    assert_eq!(e.direction, Direction::Against);
    assert_eq!(e.scale, 40);
    assert!(!e.is_block());
    // Weighed is not locked: a caller checking is_locked must not read 40% as a block.
    assert!(!l.is_locked(Axis::X, Direction::Against));
    assert_eq!(l.scale_of(Axis::X, Direction::Against), 40);
    // and a direction nothing covers is passing untouched
    assert_eq!(l.scale_of(Axis::X, Direction::With), LOCK_SCALE_PASS);
    assert_eq!(l.scale_of(Axis::Y, Direction::Against), LOCK_SCALE_PASS);
}

#[test]
fn locks_scale_of_takes_the_lowest_of_overlapping_entries() {
    let l = Locks::from_payload(&[6, 2, 3, 0, 0, LOCK_DIR_BOTH, 60, 3, 0, 0, LOCK_DIR_NEG, 25])
        .unwrap();
    assert_eq!(l.scale_of(Axis::X, Direction::Negative), 25);
    assert_eq!(l.scale_of(Axis::X, Direction::Positive), 60);
}

#[test]
fn locks_is_locked_both_needs_both_signs() {
    let both = Locks::from_payload(&[
        6,
        2,
        3,
        0,
        0,
        LOCK_DIR_POS,
        LOCK_SCALE_BLOCK,
        3,
        0,
        0,
        LOCK_DIR_NEG,
        LOCK_SCALE_BLOCK,
    ])
    .unwrap();
    assert!(both.is_locked(Axis::X, Direction::Both));
    let one = Locks::from_payload(&[6, 1, 3, 0, 0, LOCK_DIR_POS, LOCK_SCALE_BLOCK]).unwrap();
    assert!(!one.is_locked(Axis::X, Direction::Both));
    // A relative block leaves both fixed signs passing, so Both must not read it as a lock.
    let rel = Locks::from_payload(&[6, 1, 3, 0, 0, LOCK_DIR_AGAINST, LOCK_SCALE_BLOCK]).unwrap();
    assert!(!rel.is_locked(Axis::X, Direction::Both));
    assert!(rel.is_locked(Axis::X, Direction::Against));
}

#[test]
fn decode_locks_through_parse_resp() {
    let Some(Resp::Locks(l)) = parse_resp(&[
        6,
        2,
        3,
        1,
        0,
        LOCK_DIR_NEG,
        LOCK_SCALE_BLOCK,
        0,
        4,
        0,
        LOCK_DIR_NEG,
        LOCK_SCALE_BLOCK,
    ]) else {
        panic!("expected Locks");
    };
    assert!(l.is_locked(Axis::Y, Direction::Negative));
    assert!(l.is_locked(Button::Side2, Direction::Negative));
}

#[test]
fn locks_blanket_entry_decodes() {
    let l = Locks::from_payload(&[6, 1, 1, 0xFF, 0xFF, LOCK_DIR_POS, LOCK_SCALE_BLOCK]).unwrap();
    let e = l.entries()[0];
    assert_eq!(e.scope, LockScope::Blanket(Class::Key));
    assert_eq!(e.direction, Direction::Positive);
    assert!(e.is_block());
    assert!(l.is_locked(crate::Key::A, Direction::Positive));
    assert!(!l.is_locked(crate::Key::A, Direction::Negative));
    assert!(!l.is_locked(Button::Left, Direction::Positive));
}

#[test]
fn locks_unknown_entry_is_skipped() {
    // An unknown class, then an unknown direction byte: both skip, neither derails the entries after.
    let l = Locks::from_payload(&[
        6,
        3,
        0x09,
        0x00,
        0x00,
        LOCK_DIR_POS,
        LOCK_SCALE_BLOCK,
        3,
        0x00,
        0x00,
        0x7F,
        LOCK_SCALE_BLOCK,
        1,
        4,
        0,
        LOCK_DIR_POS,
        LOCK_SCALE_BLOCK,
    ])
    .unwrap();
    assert_eq!(l.entries().len(), 1);
    assert_eq!(
        l.entries()[0].scope,
        LockScope::Target(crate::Key::new(4).into())
    );
}

#[test]
fn locks_truncated_payload_is_none() {
    assert!(parse_resp(&[6]).is_none());
    // A five-byte entry cut short must not decode as a shorter one.
    assert!(Locks::from_payload(&[6, 1, 3, 0, 0, LOCK_DIR_POS]).is_none());
}

#[test]
fn locks_wide_entry_decodes_a_gain() {
    let l = Locks::from_payload(&[6, 1, 3, 0, 0, LOCK_DIR_WITH, LOCK_SCALE_MAX]).unwrap();
    assert_eq!(l.scale_of(Axis::X, Direction::With), 255);
    assert!(!l.is_locked(Axis::X, Direction::With));
}

#[test]
fn health_lock_on_bit_roundtrips() {
    let h = Health::from_flags(0x20);
    assert!(h.lock_on);
    assert!(!h.link_up && !h.mouse_attached && !h.clone_configured && !h.injection_active);
    assert!(!h.rate_confident);
    assert_eq!(h.to_flags(), 0x20);
    assert_eq!(Health::from_flags(0x3F).to_flags(), 0x3F);
}

#[cfg(feature = "mock")]
#[test]
fn a_relative_direction_needs_a_bearing_and_only_an_axis_has_one() {
    use crate::error::Error;
    use crate::types::{Blanket, MediaKey};
    let dev = crate::Device::with_mock(crate::MockBox::new());
    for d in [Direction::With, Direction::Against] {
        for r in [
            dev.lock(Button::Left, d),
            dev.scale(Key::A, d, 40),
            dev.lock(MediaKey::MUTE, d),
            dev.lock_all(Blanket::Buttons, d),
            dev.lock_all(Blanket::Keys, d),
            dev.lock_all(Blanket::Media, d),
        ] {
            assert!(matches!(r, Err(Error::RelativeDirection { .. })), "{r:?}");
        }
        assert!(dev.scale(Axis::X, d, 130).is_ok());
        assert!(dev.scale_all(Blanket::Aim, d, 40).is_ok());
        assert!(dev.scale_all(Blanket::Wheel, d, 40).is_ok());
    }
}

#[cfg(feature = "mock")]
#[test]
fn nothing_refused_reaches_the_wire() {
    use crate::protocol::FrameType;
    use crate::types::Blanket;
    let mock = crate::MockBox::new();
    let dev = crate::Device::with_mock(mock.clone());
    let _ = dev.lock(Button::Left, Direction::With);
    let _ = dev.lock_all(Blanket::Keys, Direction::Against);
    assert_eq!(
        mock.recorded_frames()
            .iter()
            .filter(|f| f.ty == FrameType::Lock)
            .count(),
        0
    );
}

#[cfg(feature = "mock")]
#[test]
fn a_media_edge_is_sent_as_the_both_the_box_reports() {
    use crate::protocol::FrameType;
    use crate::types::MediaKey;
    let mock = crate::MockBox::new();
    let dev = crate::Device::with_mock(mock.clone());
    dev.lock(MediaKey::MUTE, Direction::PRESS).unwrap();
    let sent: Vec<Vec<u8>> = mock
        .recorded_frames()
        .into_iter()
        .filter(|f| f.ty == FrameType::Lock)
        .map(|f| f.payload)
        .collect();
    assert_eq!(
        sent,
        vec![vec![
            LOCK_CLS_MEDIA,
            0xE2,
            0x00,
            LOCK_DIR_BOTH,
            LOCK_SCALE_BLOCK
        ]]
    );
}

#[cfg(feature = "mock")]
#[test]
fn the_mock_answers_locks_from_the_table_the_frames_build() {
    let dev = crate::Device::with_mock(crate::MockBox::new());
    dev.scale(Axis::X, Direction::Both, 50).unwrap();
    let l = dev.query_locks().unwrap();
    // Both is the fixed pair only: two entries, and the relative pair still passes.
    assert_eq!(l.entries().len(), 2);
    assert_eq!(l.scale_of(Axis::X, Direction::Positive), 50);
    assert_eq!(l.scale_of(Axis::X, Direction::Negative), 50);
    assert_eq!(l.scale_of(Axis::X, Direction::With), LOCK_SCALE_PASS);
    assert_eq!(l.scale_of(Axis::X, Direction::Against), LOCK_SCALE_PASS);
    dev.unlock(Axis::X, Direction::Both).unwrap();
    assert_eq!(dev.query_locks().unwrap().entries().len(), 0);
}

#[cfg(feature = "mock")]
#[test]
fn vector_mode_reports_the_scale_the_box_applies_to_the_aim() {
    use crate::types::BearingMode;
    use std::time::Duration;
    let dev = crate::Device::with_mock(crate::MockBox::new());
    dev.scale(Axis::X, Direction::With, 130).unwrap();
    dev.scale(Axis::Y, Direction::With, 60).unwrap();
    dev.set_bearing(Some(Duration::from_millis(20)), BearingMode::PerAxis)
        .unwrap();
    let l = dev.query_locks().unwrap();
    assert_eq!(l.scale_of(Axis::X, Direction::With), 130);
    assert_eq!(l.scale_of(Axis::Y, Direction::With), 60);
    // In vector mode one relative scale governs the whole aim, the lower of the two, so the readback
    // names 60 on both axes rather than each axis's stored byte.
    dev.set_bearing(Some(Duration::from_millis(20)), BearingMode::Vector)
        .unwrap();
    let l = dev.query_locks().unwrap();
    assert_eq!(l.scale_of(Axis::X, Direction::With), 60);
    assert_eq!(l.scale_of(Axis::Y, Direction::With), 60);
    // The absolute pair is stored and reported per axis in either mode. Which of its two slots a
    // delta lands in is a renderer question the box answers on the emitted value, not here.
    dev.scale(Axis::X, Direction::Positive, 25).unwrap();
    assert_eq!(
        dev.query_locks()
            .unwrap()
            .scale_of(Axis::X, Direction::Positive),
        25
    );
}

#[cfg(feature = "mock")]
#[test]
fn a_key_blanket_reports_one_entry_per_blocked_edge() {
    use crate::types::Blanket;
    let dev = crate::Device::with_mock(crate::MockBox::new());
    dev.lock_all(Blanket::Keys, Direction::Positive).unwrap();
    let l = dev.query_locks().unwrap();
    assert_eq!(
        l.entries(),
        [crate::types::LockEntry {
            scope: LockScope::Blanket(Class::Key),
            direction: Direction::Positive,
            scale: LOCK_SCALE_BLOCK,
        }]
    );
    assert!(l.is_locked(Key::A, Direction::Positive));
    assert!(!l.is_locked(Key::A, Direction::Negative));
    // Both edges are two entries, never a single Both the box is not holding.
    dev.lock_all(Blanket::Keys, Direction::Negative).unwrap();
    let l = dev.query_locks().unwrap();
    assert_eq!(
        l.entries().iter().map(|e| e.direction).collect::<Vec<_>>(),
        vec![Direction::Positive, Direction::Negative]
    );
    dev.unlock_all(Blanket::Keys, Direction::Positive).unwrap();
    assert_eq!(
        dev.query_locks()
            .unwrap()
            .entries()
            .iter()
            .map(|e| e.direction)
            .collect::<Vec<_>>(),
        vec![Direction::Negative]
    );
}

#[cfg(feature = "mock")]
#[test]
fn a_media_lock_reports_as_both_whatever_edge_was_asked_for() {
    use crate::types::MediaKey;
    let dev = crate::Device::with_mock(crate::MockBox::new());
    dev.lock(MediaKey::MUTE, Direction::RELEASE).unwrap();
    let l = dev.query_locks().unwrap();
    assert_eq!(l.entries().len(), 1);
    assert_eq!(l.entries()[0].direction, Direction::Both);
    assert!(l.is_locked(MediaKey::MUTE, Direction::Both));
}

#[cfg(feature = "mock")]
#[test]
fn a_scale_at_or_above_a_pass_unlocks_a_one_bit_class() {
    let dev = crate::Device::with_mock(crate::MockBox::new());
    dev.scale(Button::Left, Direction::Positive, 50).unwrap();
    let l = dev.query_locks().unwrap();
    // Under a full pass a button truncates to a block, so that is what reads back.
    assert_eq!(
        l.scale_of(Button::Left, Direction::Positive),
        LOCK_SCALE_BLOCK
    );
    assert!(l.is_locked(Button::Left, Direction::Positive));
    // 150% is an amplification a one-bit field cannot carry, so the box truncates it to a pass.
    dev.scale(Button::Left, Direction::Positive, 150).unwrap();
    assert_eq!(dev.query_locks().unwrap().entries().len(), 0);
}

#[cfg(feature = "mock")]
#[test]
fn a_relative_direction_on_a_one_bit_class_writes_nothing_box_side() {
    // The crate refuses to send one, so drive the modelled table with the frames a 3.1.x host would.
    use crate::mock::LockTable;
    use crate::protocol::opcode::{LOCK_CLS_BTN, LOCK_ID_ALL};
    use crate::types::BearingMode;
    let mut t = LockTable::default();
    t.apply(LOCK_CLS_BTN, 0, LOCK_DIR_AGAINST, LOCK_SCALE_BLOCK);
    t.apply(LOCK_CLS_KEY, 0x04, LOCK_DIR_WITH, LOCK_SCALE_BLOCK);
    t.apply(LOCK_CLS_KEY, LOCK_ID_ALL, LOCK_DIR_WITH, LOCK_SCALE_BLOCK);
    assert_eq!(t.pack(BearingMode::PerAxis).entries().len(), 0);
    // A media usage reads no direction at all, so the same frame blocks it whole.
    t.apply(LOCK_CLS_MEDIA, 0xE2, LOCK_DIR_WITH, LOCK_SCALE_BLOCK);
    assert_eq!(t.pack(BearingMode::PerAxis).entries().len(), 1);
}

#[cfg(feature = "mock")]
#[test]
fn a_reset_clears_the_lock_table() {
    let dev = crate::Device::with_mock(crate::MockBox::new());
    dev.lock(Axis::X, Direction::Both).unwrap();
    dev.lock_all(crate::types::Blanket::Keys, Direction::Both)
        .unwrap();
    assert!(!dev.query_locks().unwrap().entries().is_empty());
    dev.reset().unwrap();
    assert_eq!(dev.query_locks().unwrap().entries().len(), 0);
}

#[cfg(feature = "mock")]
#[test]
fn the_reply_truncates_granular_keys_and_never_the_bounded_classes() {
    use crate::types::MediaKey;
    let dev = crate::Device::with_mock(crate::MockBox::new());
    dev.lock(MediaKey::MUTE, Direction::Both).unwrap();
    for u in 0x04..=0x3Fu8 {
        dev.lock(crate::Key::new(u), Direction::Both).unwrap();
    }
    let l = dev.query_locks().unwrap();
    // The reply holds 96 entries. 60 keys on both edges offer 120, so what comes back is the one
    // media entry plus the 95 key edges that fit: media first, because granular keys are enumerated
    // last precisely so the unbounded class cannot starve the bounded one off the frame.
    assert_eq!(l.entries().len(), 96);
    assert_eq!(
        l.entries()[0],
        crate::types::LockEntry {
            scope: LockScope::Target(MediaKey::MUTE.into()),
            direction: Direction::Both,
            scale: LOCK_SCALE_BLOCK,
        }
    );
    assert!(l.is_locked(MediaKey::MUTE, Direction::Both));
    assert!(l.entries()[1..].iter().all(|e| matches!(
        e.scope,
        LockScope::Target(LockTarget::Usage(u)) if u.class == Class::Key
    )));
    // 95 key edges is usages 0x04..=0x32 on both edges (94) then 0x33's press edge alone, so the cut
    // lands mid-usage and everything past it is gone.
    assert_eq!(
        *l.entries().last().unwrap(),
        crate::types::LockEntry {
            scope: LockScope::Target(crate::Key::new(0x33).into()),
            direction: Direction::Positive,
            scale: LOCK_SCALE_BLOCK,
        }
    );
    assert!(l.is_locked(crate::Key::new(0x32), Direction::Negative));
    assert!(!l.is_locked(crate::Key::new(0x33), Direction::Negative));
    assert!(!l.is_locked(crate::Key::new(0x34), Direction::Positive));
}
