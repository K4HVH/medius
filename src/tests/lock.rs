//! LOCK command (§3.8): payload bytes, target/direction wire, signed scale weighing, `RESP(LOCKS)`
//! decode, and the HEALTH `lock_on` bit.

use crate::protocol::command::lock_payload;
use crate::protocol::opcode::{
    LOCK_CLS_AXIS, LOCK_CLS_KEY, LOCK_CLS_MEDIA, LOCK_DIR_AGAINST, LOCK_DIR_BOTH, LOCK_DIR_NEG,
    LOCK_DIR_POS, LOCK_DIR_WITH, LOCK_ID_ALL, LOCK_SCALE_BLOCK, LOCK_SCALE_MAX, LOCK_SCALE_MIN,
    LOCK_SCALE_PASS,
};
use crate::protocol::{Resp, parse_resp};
use crate::types::{
    Axis, Button, Class, Direction, Health, Key, LockScope, LockTarget, Locks, Usage,
};

#[test]
fn lock_payload_bytes() {
    assert_eq!(
        lock_payload(LOCK_CLS_AXIS, 2, LOCK_DIR_NEG, LOCK_SCALE_BLOCK),
        [3, 2, 0, 2, 0, 0]
    );
    assert_eq!(
        lock_payload(LOCK_CLS_MEDIA, 0x00E9, LOCK_DIR_BOTH, LOCK_SCALE_BLOCK),
        [2, 0xE9, 0x00, 0, 0, 0]
    );
    assert_eq!(
        lock_payload(LOCK_CLS_KEY, LOCK_ID_ALL, LOCK_DIR_BOTH, LOCK_SCALE_BLOCK),
        [1, 0xFF, 0xFF, 0, 0, 0]
    );
    // The scale is the two bytes that used to be a state, so an unlock and a weighing both land here.
    assert_eq!(
        lock_payload(LOCK_CLS_AXIS, 0, LOCK_DIR_AGAINST, 40),
        [3, 0, 0, LOCK_DIR_AGAINST, 40, 0]
    );
    assert_eq!(
        lock_payload(LOCK_CLS_AXIS, 0, LOCK_DIR_BOTH, LOCK_SCALE_PASS),
        [3, 0, 0, 0, 100, 0]
    );
}

#[test]
fn a_reversing_scale_is_two_signed_bytes_on_the_wire() {
    // The sign is the whole point of the field being an i16: a u8 write, or a decode that forgets the
    // high byte, turns -100 into 156 and the axis amplifies instead of reversing.
    assert_eq!(
        lock_payload(LOCK_CLS_AXIS, 0, LOCK_DIR_BOTH, -100),
        [3, 0, 0, 0, 0x9C, 0xFF]
    );
    assert_eq!(
        lock_payload(LOCK_CLS_AXIS, 1, LOCK_DIR_POS, LOCK_SCALE_MIN),
        [3, 1, 0, LOCK_DIR_POS, 0x01, 0xFF]
    );
    let l = Locks::from_payload(&[6, 1, 3, 0, 0, LOCK_DIR_BOTH, 0x9C, 0xFF]).unwrap();
    assert_eq!(l.scale_of(Axis::X, Direction::Both), -100);
    assert!(
        !l.is_locked(Axis::X, Direction::Both),
        "a reversal is not a block"
    );
}

#[test]
fn lock_scale_constants() {
    assert_eq!(
        (
            LOCK_SCALE_BLOCK,
            LOCK_SCALE_PASS,
            LOCK_SCALE_MAX,
            LOCK_SCALE_MIN
        ),
        (0, 100, 255, -255)
    );
}

#[test]
fn axis_wire_ids_and_total_from_u16() {
    assert_eq!(
        (
            Axis::X.as_u16(),
            Axis::Y.as_u16(),
            Axis::Wheel.as_u16(),
            Axis::Pan.as_u16(),
        ),
        (0, 1, 2, 3)
    );
    // Every named axis round-trips, and from_u16 is total: an id no axis names is None, never a panic.
    for a in Axis::ALL {
        assert_eq!(Axis::from_u16(a.as_u16()), Some(a));
    }
    assert_eq!(Axis::from_u16(4), None);
    assert_eq!(Axis::from_u16(0xFFFF), None);
}

#[test]
fn button_is_an_open_id() {
    // The five names are constants over the wire ids; any id past them is representable.
    assert_eq!(
        (
            Button::LEFT.as_id(),
            Button::RIGHT.as_id(),
            Button::MIDDLE.as_id(),
            Button::SIDE1.as_id(),
            Button::SIDE2.as_id(),
        ),
        (0, 1, 2, 3, 4)
    );
    assert_eq!(Button::new(0), Button::LEFT);
    assert_eq!(Button::new(8).as_id(), 8);
    // from_id is total: every byte is a button, including one past what any device wires.
    assert_eq!(Button::from_id(200).as_id(), 200);
    // A button past five carries into the momentary-usage space unchanged.
    assert_eq!(Usage::from(Button::new(8)), Usage::new(Class::Button, 8));
}

#[test]
fn lock_target_and_direction_wire() {
    assert_eq!(
        (Axis::X.as_u16(), Axis::Y.as_u16(), Axis::Wheel.as_u16()),
        (0, 1, 2)
    );
    let bt: LockTarget = Button::LEFT.into();
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
        0,
        0,
        1,
        0x04,
        0x00,
        LOCK_DIR_NEG,
        0,
        0,
    ])
    .unwrap();
    assert_eq!(l.entries().len(), 2);
    assert!(l.is_locked(Axis::X, Direction::Positive));
    assert!(!l.is_locked(Axis::X, Direction::Negative));
    assert!(l.is_locked(Key::A, Direction::Negative));
}

#[test]
fn locks_report_a_partial_scale_as_weighed_not_locked() {
    let l = Locks::from_payload(&[6, 1, 3, 0, 0, LOCK_DIR_AGAINST, 40, 0]).unwrap();
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
    let l = Locks::from_payload(&[
        6,
        2,
        3,
        0,
        0,
        LOCK_DIR_BOTH,
        60,
        0,
        3,
        0,
        0,
        LOCK_DIR_NEG,
        25,
        0,
    ])
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
        0,
        0,
        3,
        0,
        0,
        LOCK_DIR_NEG,
        0,
        0,
    ])
    .unwrap();
    assert!(both.is_locked(Axis::X, Direction::Both));
    let one = Locks::from_payload(&[6, 1, 3, 0, 0, LOCK_DIR_POS, 0, 0]).unwrap();
    assert!(!one.is_locked(Axis::X, Direction::Both));
    // A relative block leaves both fixed signs passing, so Both must not read it as a lock.
    let rel = Locks::from_payload(&[6, 1, 3, 0, 0, LOCK_DIR_AGAINST, 0, 0]).unwrap();
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
        0,
        0,
        0,
        4,
        0,
        LOCK_DIR_NEG,
        0,
        0,
    ]) else {
        panic!("expected Locks");
    };
    assert!(l.is_locked(Axis::Y, Direction::Negative));
    assert!(l.is_locked(Button::SIDE2, Direction::Negative));
}

#[test]
fn locks_blanket_entry_decodes() {
    let l = Locks::from_payload(&[6, 1, 1, 0xFF, 0xFF, LOCK_DIR_POS, 0, 0]).unwrap();
    let e = l.entries()[0];
    assert_eq!(e.scope, LockScope::Blanket(Class::Key));
    assert_eq!(e.direction, Direction::Positive);
    assert!(e.is_block());
    assert!(l.is_locked(crate::Key::A, Direction::Positive));
    assert!(!l.is_locked(crate::Key::A, Direction::Negative));
    assert!(!l.is_locked(Button::LEFT, Direction::Positive));
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
        0,
        0,
        3,
        0x00,
        0x00,
        0x7F,
        0,
        0,
        1,
        4,
        0,
        LOCK_DIR_POS,
        0,
        0,
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
    // A six-byte entry cut short must not decode as a shorter one, and the cut that matters is the
    // scale's high byte: dropping it is what a five-byte reader would do.
    assert!(Locks::from_payload(&[6, 1, 3, 0, 0, LOCK_DIR_POS]).is_none());
    assert!(Locks::from_payload(&[6, 1, 3, 0, 0, LOCK_DIR_POS, 0]).is_none());
}

#[test]
fn locks_wide_entry_decodes_a_gain() {
    let l = Locks::from_payload(&[6, 1, 3, 0, 0, LOCK_DIR_WITH, 0xFF, 0x00]).unwrap();
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
            dev.lock(Button::LEFT, d),
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
    let _ = dev.lock(Button::LEFT, Direction::With);
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
        vec![vec![LOCK_CLS_MEDIA, 0xE2, 0x00, LOCK_DIR_BOTH, 0, 0]]
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
fn ac_pan_is_a_lockable_axis_peer_of_the_wheel() {
    let dev = crate::Device::with_mock(crate::MockBox::new());
    dev.lock(Axis::Pan, Direction::Positive).unwrap();
    dev.scale(Axis::Pan, Direction::Against, 40).unwrap();
    let l = dev.query_locks().unwrap();
    assert!(l.is_locked(Axis::Pan, Direction::Positive));
    assert_eq!(l.scale_of(Axis::Pan, Direction::Against), 40);
    // Pan is its own axis row: the wheel it neighbours is untouched.
    assert_eq!(
        l.scale_of(Axis::Wheel, Direction::Positive),
        LOCK_SCALE_PASS
    );
    // Both clears the relative pair too, so the Against weighing does not keep applying unseen.
    dev.unlock(Axis::Pan, Direction::Both).unwrap();
    let l = dev.query_locks().unwrap();
    assert!(!l.is_locked(Axis::Pan, Direction::Positive));
    assert_eq!(l.scale_of(Axis::Pan, Direction::Against), LOCK_SCALE_PASS);
}

#[cfg(feature = "mock")]
#[test]
fn a_button_past_the_five_named_locks_only_when_the_clone_declares_it() {
    use crate::types::MouseCaps;
    let wide = crate::MockBox::new().with_mouse_caps(MouseCaps {
        n_buttons: 16,
        has_x: true,
        has_y: true,
        has_wheel: true,
        pan: false,
        has_report_id: false,
        n_hid: 1,
    });
    let dev = crate::Device::with_mock(wide);
    dev.lock(Button::new(8), Direction::Positive).unwrap();
    let l = dev.query_locks().unwrap();
    assert!(l.is_locked(Button::new(8), Direction::Positive));
    assert_eq!(
        l.scale_of(Button::new(8), Direction::Positive),
        LOCK_SCALE_BLOCK
    );

    // A narrow clone (the default five-button mock) has no row for button 8, so the box drops it.
    let narrow = crate::Device::with_mock(crate::MockBox::new());
    narrow.lock(Button::new(8), Direction::Positive).unwrap();
    assert_eq!(narrow.query_locks().unwrap().entries().len(), 0);
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
    // In vector mode one relative scale governs both axes, the lower of the two, so the readback
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
    dev.scale(Button::LEFT, Direction::Positive, 50).unwrap();
    let l = dev.query_locks().unwrap();
    // Under a full pass a button truncates to a block, so that is what reads back.
    assert_eq!(
        l.scale_of(Button::LEFT, Direction::Positive),
        LOCK_SCALE_BLOCK
    );
    assert!(l.is_locked(Button::LEFT, Direction::Positive));
    // 150% is an amplification a one-bit field cannot carry, so the box truncates it to a pass.
    dev.scale(Button::LEFT, Direction::Positive, 150).unwrap();
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
    t.apply(LOCK_CLS_BTN, 0, LOCK_DIR_AGAINST, LOCK_SCALE_BLOCK, 5);
    t.apply(LOCK_CLS_KEY, 0x04, LOCK_DIR_WITH, LOCK_SCALE_BLOCK, 5);
    t.apply(
        LOCK_CLS_KEY,
        LOCK_ID_ALL,
        LOCK_DIR_WITH,
        LOCK_SCALE_BLOCK,
        5,
    );
    assert_eq!(t.pack(BearingMode::PerAxis).entries().len(), 0);
    // A media usage reads no direction at all, so the same frame blocks it whole.
    t.apply(LOCK_CLS_MEDIA, 0xE2, LOCK_DIR_WITH, LOCK_SCALE_BLOCK, 5);
    assert_eq!(t.pack(BearingMode::PerAxis).entries().len(), 1);
}

#[cfg(feature = "mock")]
#[test]
fn the_signed_scale_is_bounded_and_axis_only() {
    // Both refusals are the crate's, before the wire.
    use crate::protocol::FrameType;
    let mock = crate::MockBox::new();
    let dev = crate::Device::with_mock(mock.clone());
    for bad in [LOCK_SCALE_MAX + 1, LOCK_SCALE_MIN - 1, i16::MAX, i16::MIN] {
        assert!(
            matches!(
                dev.scale(Axis::X, Direction::Both, bad),
                Err(crate::Error::LockScaleRange { .. })
            ),
            "scale {bad} should be refused"
        );
    }
    for t in [
        crate::types::LockTarget::from(Button::LEFT),
        crate::types::LockTarget::from(Key::A),
        crate::types::LockTarget::from(crate::types::MediaKey::MUTE),
    ] {
        assert!(
            matches!(
                dev.scale(t, Direction::Both, -100),
                Err(crate::Error::LockScaleUsage { .. })
            ),
            "a reversal on {t:?} should be refused"
        );
    }
    assert!(!mock.saw(FrameType::Lock), "none of them reached the wire");
    // The bounds themselves are accepted, and only an axis takes the negative one.
    dev.scale(Axis::X, Direction::Both, LOCK_SCALE_MIN).unwrap();
    dev.scale(Axis::Y, Direction::Both, LOCK_SCALE_MAX).unwrap();
    assert_eq!(
        dev.query_locks()
            .unwrap()
            .scale_of(Axis::X, Direction::Positive),
        LOCK_SCALE_MIN
    );
}

#[cfg(feature = "mock")]
#[test]
fn the_box_writes_nothing_at_all_for_a_reversal_on_a_one_bit_class() {
    // The crate refuses one before the wire, so drive the modelled table directly: if the box merely
    // truncated a negative to a block, a host that lost its guard would see a lock the box refused.
    use crate::mock::LockTable;
    use crate::protocol::opcode::{LOCK_CLS_BTN, LOCK_CLS_KEY, LOCK_CLS_MEDIA};
    use crate::types::BearingMode;
    let mut t = LockTable::default();
    t.apply(LOCK_CLS_BTN, 0, LOCK_DIR_BOTH, -100, 5);
    t.apply(LOCK_CLS_KEY, 0x04, LOCK_DIR_BOTH, -100, 5);
    t.apply(LOCK_CLS_KEY, LOCK_ID_ALL, LOCK_DIR_POS, -100, 5);
    t.apply(LOCK_CLS_MEDIA, 0xE9, LOCK_DIR_BOTH, -100, 5);
    t.apply(LOCK_CLS_MEDIA, LOCK_ID_ALL, LOCK_DIR_BOTH, -100, 5);
    assert_eq!(t.pack(BearingMode::PerAxis).entries().len(), 0);
    // and the axis, which does take one, still lands
    t.apply(LOCK_CLS_AXIS, 0, LOCK_DIR_BOTH, -100, 5);
    assert_eq!(t.pack(BearingMode::PerAxis).entries().len(), 2);
}

#[test]
fn scale_of_both_ranks_a_block_above_a_reversal() {
    // A signed minimum would call -50 the lowest and report a reversal over a block, when the block is
    // what a delta actually meets. Both is the least that SURVIVES, which is a magnitude.
    let l = Locks::from_payload(&[
        6,
        2,
        3,
        0,
        0,
        LOCK_DIR_POS,
        0,
        0,
        3,
        0,
        0,
        LOCK_DIR_AGAINST,
        0xCE,
        0xFF,
    ])
    .unwrap();
    assert_eq!(l.scale_of(Axis::X, Direction::Both), LOCK_SCALE_BLOCK);
    assert_eq!(l.scale_of(Axis::X, Direction::Against), -50);
    assert!(l.is_locked(Axis::X, Direction::Positive));
    // and with nothing blocked, the reversal is still the least that survives against a wider pass
    let l = Locks::from_payload(&[
        6,
        2,
        3,
        0,
        0,
        LOCK_DIR_POS,
        130,
        0,
        3,
        0,
        0,
        LOCK_DIR_AGAINST,
        0x9C,
        0xFF,
    ])
    .unwrap();
    assert_eq!(l.scale_of(Axis::X, Direction::Both), -100);
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

// The flag byte itself, against the firmware's CTRL_RST_F_NVS. A wrong value is worse than a no-op:
// the box refuses a RESET carrying an undefined bit whole, so the release would be lost too.
#[cfg(feature = "mock")]
#[test]
fn a_factory_reset_carries_the_nvs_bit_and_a_plain_reset_carries_nothing() {
    use crate::protocol::FrameType;
    for (factory, want) in [(false, vec![0x00u8]), (true, vec![0x01u8])] {
        let mock = crate::MockBox::new();
        let dev = crate::Device::with_mock(mock.clone());
        if factory {
            dev.factory_reset().unwrap();
        } else {
            dev.reset().unwrap();
        }
        let sent: Vec<Vec<u8>> = mock
            .recorded_frames()
            .into_iter()
            .filter(|f| f.ty == FrameType::Reset)
            .map(|f| f.payload)
            .collect();
        assert_eq!(sent, vec![want], "factory={factory}");
    }
}

// Both resets release the session; only the flagged one takes the stored half with it.
#[cfg(feature = "mock")]
#[test]
fn only_a_factory_reset_clears_what_the_box_keeps_in_nvs() {
    for factory in [false, true] {
        let dev = crate::Device::with_mock(crate::MockBox::new());
        dev.set_name("Named").unwrap();
        dev.set_spread(50).unwrap();
        assert_eq!(dev.query_version().unwrap().name, "Named");
        assert_eq!(dev.query_spread().unwrap().percent, 50);
        if factory {
            dev.factory_reset().unwrap();
        } else {
            dev.reset().unwrap();
        }
        let name = dev.query_version().unwrap().name;
        let spread = dev.query_spread().unwrap().percent;
        if factory {
            assert_ne!(name, "Named", "the name survived a factory reset");
            assert_eq!(spread, 100, "spread did not return to its default");
        } else {
            assert_eq!(name, "Named", "a plain RESET took the name");
            assert_eq!(spread, 50, "a plain RESET took an option");
        }
    }
}

#[cfg(feature = "mock")]
#[test]
fn the_reply_truncates_granular_keys_and_never_the_bounded_classes() {
    use crate::types::MediaKey;
    let dev = crate::Device::with_mock(crate::MockBox::new());
    dev.lock(MediaKey::MUTE, Direction::Both).unwrap();
    dev.lock(MediaKey::new(0xE9), Direction::Both).unwrap();
    for u in 0x04..=0x3Fu8 {
        dev.lock(crate::Key::new(u), Direction::Both).unwrap();
    }
    let l = dev.query_locks().unwrap();
    // The reply holds 85 entries.
    assert_eq!(l.entries().len(), 85);
    assert_eq!(
        l.entries()[0],
        crate::types::LockEntry {
            scope: LockScope::Target(MediaKey::MUTE.into()),
            direction: Direction::Both,
            scale: LOCK_SCALE_BLOCK,
        }
    );
    assert!(l.is_locked(MediaKey::MUTE, Direction::Both));
    assert!(l.entries()[2..].iter().all(|e| matches!(
        e.scope,
        LockScope::Target(LockTarget::Usage(u)) if u.class == Class::Key
    )));
    // 83 key edges is usages 0x04..=0x2C on both edges (82) then 0x2D's press edge alone, so the cut
    // lands mid-usage and everything past it is gone.
    assert_eq!(
        *l.entries().last().unwrap(),
        crate::types::LockEntry {
            scope: LockScope::Target(crate::Key::new(0x2D).into()),
            direction: Direction::Positive,
            scale: LOCK_SCALE_BLOCK,
        }
    );
    assert!(l.is_locked(crate::Key::new(0x2C), Direction::Negative));
    assert!(!l.is_locked(crate::Key::new(0x2D), Direction::Negative));
    assert!(!l.is_locked(crate::Key::new(0x2E), Direction::Positive));
}

#[test]
fn a_lock_that_never_reached_the_wire_is_not_held() {
    // A send failure means the box was never told, so the desired state must not claim the lock:
    // one held here keeps `is_idle` false and the keepalive open for a lock nothing is applying.
    use crate::transport::Disconnected;
    let dev = crate::Device::from_transport(std::sync::Arc::new(Disconnected));
    assert!(dev.lock(Axis::X, Direction::Both).is_err());
    assert!(dev.link.desired().lock().is_idle());
    assert!(dev.link.desired().lock().held_locks().is_empty());
}

// The box's granular media locks live in a fixed slot array, not a set: INPUT_MEDIA_MAX
// (= CTRL_CONS_EVENT_MAX = 8) slots, and `media_set` takes the first free one.
#[cfg(feature = "mock")]
fn media_ids(l: &Locks) -> Vec<u16> {
    l.entries()
        .iter()
        .filter_map(|e| match e.scope {
            LockScope::Target(LockTarget::Usage(u)) if u.class == Class::Media => Some(u.id),
            _ => None,
        })
        .collect()
}

#[cfg(feature = "mock")]
#[test]
fn the_ninth_granular_media_lock_is_dropped() {
    // media_set walks the eight slots for a free one and returns without taking anything when every
    // slot is full, so the ninth distinct usage never reaches the table and never reads back.
    use crate::types::MediaKey;
    let dev = crate::Device::with_mock(crate::MockBox::new());
    let taken = [0xEAu16, 0xE9, 0x30, 0xB5, 0xB6, 0xCD, 0xE2, 0xB7];
    for id in taken {
        dev.lock(MediaKey::new(id), Direction::Both).unwrap();
    }
    dev.lock(MediaKey::new(0x223), Direction::Both).unwrap();
    let l = dev.query_locks().unwrap();
    assert_eq!(media_ids(&l), taken);
    assert!(l.is_locked(MediaKey::new(0xB7), Direction::Both));
    assert!(!l.is_locked(MediaKey::new(0x223), Direction::Both));
}

#[cfg(feature = "mock")]
#[test]
fn a_released_media_slot_is_refilled_before_the_end() {
    // Unlocking clears the slot the usage sat in and the next lock takes the first free one, so a
    // replacement lands where the released usage was, ahead of the ones that outlived it.
    use crate::types::MediaKey;
    let dev = crate::Device::with_mock(crate::MockBox::new());
    for id in [0xEAu16, 0xE9, 0x30] {
        dev.lock(MediaKey::new(id), Direction::Both).unwrap();
    }
    dev.unlock(MediaKey::new(0xE9), Direction::Both).unwrap();
    dev.lock(MediaKey::new(0xB5), Direction::Both).unwrap();
    assert_eq!(
        media_ids(&dev.query_locks().unwrap()),
        vec![0xEA, 0xB5, 0x30]
    );
}

#[cfg(feature = "mock")]
#[test]
fn a_locks_reply_past_the_entry_cap_still_answers() {
    // Locks::from_entries and MockBox::set_locks are both public and unbounded, but the box appends
    // at most CTRL_RESP_LOCKS_MAXN entries and always replies (ctrl_locks_append).
    use crate::types::{LockEntry, MediaKey};
    let mock = crate::MockBox::new();
    let entries: Vec<LockEntry> = (0..256u16)
        .map(|i| LockEntry {
            scope: LockScope::Target(MediaKey::new(0x100 + i).into()),
            direction: Direction::Both,
            scale: LOCK_SCALE_BLOCK,
        })
        .collect();
    mock.set_locks(Locks::from_entries(entries));
    let dev = crate::Device::with_mock(mock);
    let l = dev.query_locks().unwrap();
    // 85 entries is ids 0x100..=0x154; everything from 0x155 up fell off the reply.
    assert_eq!(l.entries().len(), 85);
    assert!(l.is_locked(MediaKey::new(0x100), Direction::Both));
    assert!(l.is_locked(MediaKey::new(0x154), Direction::Both));
    assert!(!l.is_locked(MediaKey::new(0x155), Direction::Both));
}

#[cfg(feature = "mock")]
#[test]
fn a_reapply_rebuilds_the_media_slots_in_the_order_the_box_had_them() {
    // The reply enumerates media in slot order, so a replay that refills the slots in a different
    // order reports the same locks as a different `Locks`.
    use crate::protocol::FrameType;
    use crate::types::MediaKey;
    let dev = crate::Device::with_mock(crate::MockBox::new());
    for id in [0xEAu16, 0xE9, 0x30, 0xB5] {
        dev.lock(MediaKey::new(id), Direction::Both).unwrap();
    }
    let before = dev.query_locks().unwrap();
    assert_eq!(media_ids(&before), vec![0xEA, 0xE9, 0x30, 0xB5]);

    // RESET clears the box's table the way the firmware's silence window does, without touching what
    // the host holds; `reapply` is then what a reconnect runs.
    dev.link.send(FrameType::Reset, &[0]).unwrap();
    assert!(dev.query_locks().unwrap().entries().is_empty());
    dev.reapply().unwrap();
    assert_eq!(dev.query_locks().unwrap(), before);
}
