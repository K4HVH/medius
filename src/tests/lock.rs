//! LOCK command (§3.8): payload bytes, enum wire values, `RESP(LOCKS)` decoding, and the HEALTH
//! `lock_on` bit (§4.2). Bytes are pinned to the firmware wire format in `ctrl_proto.h`.

#[cfg(feature = "mock")]
use crate::protocol::FrameType;
use crate::protocol::command::lock_payload;
use crate::protocol::{Resp, parse_resp};
use crate::types::{Button, Health, LockDirection, LockTarget, Locks};

#[test]
fn lock_payload_bytes() {
    // Wheel / Negative / lock.
    assert_eq!(lock_payload(2, 2, 1), [2, 2, 1]);
}

#[test]
fn lock_enums_pin_wire_values_and_roundtrip() {
    // Targets: X=0, Y=1, Wheel=2, buttons = 3 + button id (Left=0..Side2=4).
    assert_eq!(LockTarget::X.as_u8(), 0);
    assert_eq!(LockTarget::Y.as_u8(), 1);
    assert_eq!(LockTarget::Wheel.as_u8(), 2);
    assert_eq!(LockTarget::Button(Button::Left).as_u8(), 3);
    assert_eq!(LockTarget::Button(Button::Side2).as_u8(), 7);
    for t in [
        LockTarget::X,
        LockTarget::Y,
        LockTarget::Wheel,
        LockTarget::Button(Button::Left),
        LockTarget::Button(Button::Right),
        LockTarget::Button(Button::Middle),
        LockTarget::Button(Button::Side1),
        LockTarget::Button(Button::Side2),
    ] {
        assert_eq!(LockTarget::from_u8(t.as_u8()), Some(t));
    }
    assert_eq!(LockTarget::from_u8(8), None);

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
fn locks_mask_bit_layout() {
    // X+ = bit0, Side2.release = bit15 (target*2 + edge).
    let xpos = Locks::from_payload(&[6, 0x01, 0x00]).unwrap();
    assert!(xpos.is_locked(LockTarget::X, LockDirection::Positive));
    assert!(!xpos.is_locked(LockTarget::X, LockDirection::Negative));

    let side2_rel = Locks::from_payload(&[6, 0x00, 0x80]).unwrap();
    assert_eq!(side2_rel.mask(), 0x8000);
    assert!(side2_rel.is_locked(LockTarget::Button(Button::Side2), LockDirection::Negative));
    assert!(!side2_rel.is_locked(LockTarget::Button(Button::Side2), LockDirection::Positive));
}

#[test]
fn locks_is_locked_both_needs_both_edges() {
    // X+ and X- both set (bits 0 and 1).
    let both = Locks::from_payload(&[6, 0x03, 0x00]).unwrap();
    assert!(both.is_locked(LockTarget::X, LockDirection::Both));
    // Only X+ set.
    let one = Locks::from_payload(&[6, 0x01, 0x00]).unwrap();
    assert!(!one.is_locked(LockTarget::X, LockDirection::Both));
}

#[test]
fn decode_locks_through_parse_resp() {
    // Y- (bit3) + Side2.release (bit15) = mask 0x8008.
    let Some(Resp::Locks(l)) = parse_resp(&[6, 0x08, 0x80]) else {
        panic!("expected Locks");
    };
    assert_eq!(l.mask(), 0x8008);
    assert!(l.is_locked(LockTarget::Y, LockDirection::Negative));
    assert!(l.is_locked(LockTarget::Button(Button::Side2), LockDirection::Negative));
}

#[test]
fn locks_truncated_payload_is_none() {
    assert!(parse_resp(&[6, 0x00]).is_none()); // LOCKS needs 3
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

#[cfg(feature = "mock")]
#[test]
fn lock_sends_a_lock_frame() {
    use crate::{Device, MockBox};
    let mock = MockBox::new();
    let device = Device::with_mock(mock.clone());
    device
        .lock(LockTarget::Wheel, LockDirection::Negative)
        .unwrap();
    let frames = mock.recorded_frames();
    let lock = frames
        .iter()
        .find(|f| f.ty == FrameType::Lock)
        .expect("a LOCK frame was recorded");
    assert_eq!(lock.payload, vec![2, 2, 1]);
}

#[cfg(feature = "mock")]
#[test]
fn unlock_sends_state_zero() {
    use crate::{Device, MockBox};
    let mock = MockBox::new();
    let device = Device::with_mock(mock.clone());
    device.unlock(LockTarget::X, LockDirection::Both).unwrap();
    let frames = mock.recorded_frames();
    let lock = frames
        .iter()
        .find(|f| f.ty == FrameType::Lock)
        .expect("a LOCK frame was recorded");
    assert_eq!(lock.payload, vec![0, 0, 0]);
}

#[cfg(feature = "mock")]
#[test]
fn query_locks_roundtrips_a_mask() {
    use crate::{Device, MockBox};
    // X+ (bit0) + Side2.release (bit15).
    let locks = Locks::from_payload(&[6, 0x01, 0x80]).unwrap();
    let mock = MockBox::new().with_locks(locks);
    let device = Device::with_mock(mock);
    let got = device.query_locks().unwrap();
    assert_eq!(got.mask(), 0x8001);
    assert!(got.is_locked(LockTarget::X, LockDirection::Positive));
    assert!(got.is_locked(LockTarget::Button(Button::Side2), LockDirection::Negative));
}
