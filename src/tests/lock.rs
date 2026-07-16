//! LOCK command (§3.8): payload bytes, the target/direction wire, the list-format `RESP(LOCKS)` decode,
//! and the HEALTH `lock_on` bit (§4.2). Bytes are pinned to the firmware wire format in `ctrl_proto.h`.

use crate::protocol::command::lock_payload;
use crate::protocol::opcode::{
    LOCK_CLS_AXIS, LOCK_CLS_KEY, LOCK_CLS_MEDIA, LOCK_DIRBIT_NEG, LOCK_DIRBIT_POS, LOCK_ID_ALL,
};
use crate::protocol::{Resp, parse_resp};
use crate::types::{
    Axis, Button, Class, Health, Key, LockDirection, LockScope, LockTarget, Locks, Usage,
};

#[test]
fn lock_payload_bytes() {
    // [class][id u16 LE][direction][state]. Axis wheel / Negative / lock.
    assert_eq!(lock_payload(LOCK_CLS_AXIS, 2, 2, 1), [3, 2, 0, 2, 1]);
    // Media usage round-trips its 16 bits (VolumeUp 0x00E9).
    assert_eq!(
        lock_payload(LOCK_CLS_MEDIA, 0x00E9, 0, 1),
        [2, 0xE9, 0x00, 0, 1]
    );
    // Blanket all-keys: class KEY + id 0xFFFF.
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
    // A button/key/media all become a LockTarget::Usage sharing INJECT's (class, id).
    let bt: LockTarget = Button::Left.into();
    assert_eq!(bt, LockTarget::Usage(Usage::new(Class::Button, 0)));
    let at: LockTarget = Axis::Wheel.into();
    assert_eq!(at, LockTarget::Axis(Axis::Wheel));

    assert_eq!(
        (
            LockDirection::Both.as_u8(),
            LockDirection::Positive.as_u8(),
            LockDirection::Negative.as_u8()
        ),
        (0, 1, 2)
    );
    for d in [
        LockDirection::Both,
        LockDirection::Positive,
        LockDirection::Negative,
    ] {
        assert_eq!(LockDirection::from_u8(d.as_u8()), Some(d));
    }
    assert_eq!(LockDirection::from_u8(3), None);
}

#[test]
fn locks_list_decode() {
    // [6][n=2] then [AXIS(3), X(0), POS] and [KEY(1), 0x04, NEG].
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
    assert!(l.is_locked(Axis::X, LockDirection::Positive));
    assert!(!l.is_locked(Axis::X, LockDirection::Negative));
    assert!(l.is_locked(Key::A, LockDirection::Negative));
}

#[test]
fn locks_is_locked_both_needs_both_edges() {
    // AXIS X locked on both edges (dirbits POS|NEG).
    let both = Locks::from_payload(&[6, 1, 3, 0, 0, LOCK_DIRBIT_POS | LOCK_DIRBIT_NEG]).unwrap();
    assert!(both.is_locked(Axis::X, LockDirection::Both));
    let one = Locks::from_payload(&[6, 1, 3, 0, 0, LOCK_DIRBIT_POS]).unwrap();
    assert!(!one.is_locked(Axis::X, LockDirection::Both));
}

#[test]
fn decode_locks_through_parse_resp() {
    // Y- and Side2.release, as a two-entry list.
    let Some(Resp::Locks(l)) =
        parse_resp(&[6, 2, 3, 1, 0, LOCK_DIRBIT_NEG, 0, 4, 0, LOCK_DIRBIT_NEG])
    else {
        panic!("expected Locks");
    };
    assert!(l.is_locked(Axis::Y, LockDirection::Negative));
    assert!(l.is_locked(Button::Side2, LockDirection::Negative));
}

#[test]
fn locks_blanket_entry_decodes() {
    // Blanket all-keys: [6][n=1][KEY(1), 0xFFFF, POS].
    let l = Locks::from_payload(&[6, 1, 1, 0xFF, 0xFF, LOCK_DIRBIT_POS]).unwrap();
    let e = l.entries()[0];
    assert_eq!(e.scope, LockScope::Blanket(Class::Key));
    assert!(e.positive && !e.negative);
    // A covering blanket answers is_locked for any usage of that class, but only on its locked edge.
    assert!(l.is_locked(crate::Key::A, LockDirection::Positive));
    assert!(!l.is_locked(crate::Key::A, LockDirection::Negative));
    assert!(!l.is_locked(Button::Left, LockDirection::Positive)); // different class
}

#[test]
fn locks_unknown_entry_is_skipped() {
    // An unknown class byte (0x09) is a malformed wire; the entry is dropped, not kept as garbage.
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
    assert!(parse_resp(&[6]).is_none()); // needs at least the count byte
}

#[test]
fn health_lock_on_bit_roundtrips() {
    let h = Health::from_flags(0x20);
    assert!(h.lock_on);
    assert!(!h.link_up && !h.mouse_attached && !h.clone_configured && !h.injection_active);
    assert!(!h.rate_confident);
    assert_eq!(h.to_flags(), 0x20);
    // and it survives a full round-trip with the other bits set
    assert_eq!(Health::from_flags(0x3F).to_flags(), 0x3F);
}
