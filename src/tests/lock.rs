//! LOCK command (§3.8): payload bytes, target/direction wire, `RESP(LOCKS)` decode, and the HEALTH `lock_on` bit.

use crate::protocol::command::lock_payload;
use crate::protocol::opcode::{
    LOCK_CLS_AXIS, LOCK_CLS_KEY, LOCK_CLS_MEDIA, LOCK_DIRBIT_NEG, LOCK_DIRBIT_POS, LOCK_ID_ALL,
};
use crate::protocol::{Resp, parse_resp};
use crate::types::{
    Axis, Button, Class, Health, Key, Direction, LockScope, LockTarget, Locks, Usage,
};

#[test]
fn lock_payload_bytes() {
    assert_eq!(lock_payload(LOCK_CLS_AXIS, 2, 2, 1), [3, 2, 0, 2, 1]);
    assert_eq!(
        lock_payload(LOCK_CLS_MEDIA, 0x00E9, 0, 1),
        [2, 0xE9, 0x00, 0, 1]
    );
    assert_eq!(
        lock_payload(LOCK_CLS_KEY, LOCK_ID_ALL, 0, 1),
        [1, 0xFF, 0xFF, 0, 1]
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
            Direction::Negative.as_u8()
        ),
        (0, 1, 2)
    );
    for d in [
        Direction::Both,
        Direction::Positive,
        Direction::Negative,
    ] {
        assert_eq!(Direction::from_u8(d.as_u8()), Some(d));
    }
    assert_eq!(Direction::from_u8(3), None);
}

#[test]
fn locks_list_decode() {
    let l = Locks::from_payload(&[
        6,
        2,
        3,
        0,
        0,
        LOCK_DIRBIT_POS,
        1,
        0x04,
        0x00,
        LOCK_DIRBIT_NEG,
    ])
    .unwrap();
    assert_eq!(l.entries().len(), 2);
    assert!(l.is_locked(Axis::X, Direction::Positive));
    assert!(!l.is_locked(Axis::X, Direction::Negative));
    assert!(l.is_locked(Key::A, Direction::Negative));
}

#[test]
fn locks_is_locked_both_needs_both_edges() {
    let both = Locks::from_payload(&[6, 1, 3, 0, 0, LOCK_DIRBIT_POS | LOCK_DIRBIT_NEG]).unwrap();
    assert!(both.is_locked(Axis::X, Direction::Both));
    let one = Locks::from_payload(&[6, 1, 3, 0, 0, LOCK_DIRBIT_POS]).unwrap();
    assert!(!one.is_locked(Axis::X, Direction::Both));
}

#[test]
fn decode_locks_through_parse_resp() {
    let Some(Resp::Locks(l)) =
        parse_resp(&[6, 2, 3, 1, 0, LOCK_DIRBIT_NEG, 0, 4, 0, LOCK_DIRBIT_NEG])
    else {
        panic!("expected Locks");
    };
    assert!(l.is_locked(Axis::Y, Direction::Negative));
    assert!(l.is_locked(Button::Side2, Direction::Negative));
}

#[test]
fn locks_blanket_entry_decodes() {
    let l = Locks::from_payload(&[6, 1, 1, 0xFF, 0xFF, LOCK_DIRBIT_POS]).unwrap();
    let e = l.entries()[0];
    assert_eq!(e.scope, LockScope::Blanket(Class::Key));
    assert!(e.positive && !e.negative);
    assert!(l.is_locked(crate::Key::A, Direction::Positive));
    assert!(!l.is_locked(crate::Key::A, Direction::Negative));
    assert!(!l.is_locked(Button::Left, Direction::Positive));
}

#[test]
fn locks_unknown_entry_is_skipped() {
    let l = Locks::from_payload(&[
        6,
        2,
        0x09,
        0x00,
        0x00,
        LOCK_DIRBIT_POS,
        1,
        4,
        0,
        LOCK_DIRBIT_POS,
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
