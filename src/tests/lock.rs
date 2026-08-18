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
