//! CATCH command (§3.9): payload bytes, the mask/event/snapshot/state decode, the HEALTH catch_on bit, and EventStream lifecycle; bytes pinned to the firmware wire format in ctrl_proto.h.

#[cfg(feature = "mock")]
use crate::protocol::FrameType;
use crate::protocol::command::catch_payload;
use crate::protocol::opcode::{CATCH_BUTTONS, CATCH_KEYS, CATCH_MEDIA, CATCH_MOTION, CATCH_WHEEL};
use crate::protocol::{Resp, parse_resp};
use crate::types::{CatchMask, CatchState, Class, Health, MotionEvent, UsageSnapshot};

#[test]
fn catch_payload_bytes() {
    assert_eq!(catch_payload(CatchMask::all().bits()), [0x1F]);
    assert_eq!(catch_payload(0), [0x00]);
}

#[test]
fn catch_mask_class_bits_and_ops() {
    assert_eq!(CatchMask::MOTION.bits(), CATCH_MOTION);
    assert_eq!(CatchMask::WHEEL.bits(), CATCH_WHEEL);
    assert_eq!(CatchMask::BUTTONS.bits(), CATCH_BUTTONS);
    assert_eq!(CatchMask::KEYS.bits(), CATCH_KEYS);
    assert_eq!(CatchMask::MEDIA.bits(), CATCH_MEDIA);
    assert_eq!(CatchMask::all().bits(), 0x1F);
    assert!(CatchMask::empty().is_empty());

    let m = CatchMask::MOTION | CatchMask::BUTTONS;
    assert_eq!(m.bits(), 0x05);
    assert!(m.contains(CatchMask::MOTION));
    assert!(m.contains(CatchMask::BUTTONS));
    assert!(!m.contains(CatchMask::WHEEL));
    assert_eq!(
        CatchMask::MOTION
            | CatchMask::WHEEL
            | CatchMask::BUTTONS
            | CatchMask::KEYS
            | CatchMask::MEDIA,
        CatchMask::all()
    );

    assert_eq!(CatchMask::from_bits_truncate(0xFF), CatchMask::all());
    assert_eq!(CatchMask::from_bits_truncate(0xE0), CatchMask::empty());
}

#[test]
fn motion_event_decodes() {
    let r =
        MotionEvent::from_payload(&[0x04, 0x03, 0x02, 0x01, 0x2C, 0x01, 0xCE, 0xFF, 0xFF, 0xFF])
            .unwrap();
    assert_eq!((r.dx, r.dy, r.dz), (300, -50, -1));
    assert_eq!(r.ts_us, 0x0102_0304); // raw wire value; the reader widens it
    assert!(MotionEvent::from_payload(&[0; 9]).is_none()); // needs 10
}

#[test]
fn usage_snapshot_decodes() {
    let s = UsageSnapshot::from_payload(&[0, 0, 0, 0, 2, 0, 0, 0, 1, 0x04, 0x00]).unwrap();
    assert_eq!(s.usages.len(), 2);
    assert_eq!(s.class(), Some(Class::Button));
    assert!(s.is_held(crate::Button::Left));
    assert!(s.is_held(crate::Key::A));
    assert!(!s.is_held(crate::Button::Right));
}

#[test]
fn catch_state_decodes_mask_and_drops() {
    let c = CatchState::from_payload(&[7, 0x04, 0x04, 0x03, 0x02, 0x01]).unwrap();
    assert_eq!(c.mask, CatchMask::BUTTONS);
    assert_eq!(c.dropped, 0x01020304);
    assert!(CatchState::from_payload(&[7, 0, 0, 0, 0]).is_none()); // needs 6
}

#[test]
fn decode_catch_through_parse_resp() {
    let Some(Resp::Catch(c)) = parse_resp(&[7, 0x1F, 0, 0, 0, 0]) else {
        panic!("expected Catch");
    };
    assert_eq!(c.mask, CatchMask::all());
    assert_eq!(c.dropped, 0);
}

#[test]
fn health_catch_on_bit_roundtrips() {
    let h = Health::from_flags(0x40);
    assert!(h.catch_on);
    assert!(!h.lock_on && !h.link_up);
    assert_eq!(h.to_flags(), 0x40);
    assert_eq!(Health::from_flags(0x7F).to_flags(), 0x7F);
}

#[cfg(feature = "mock")]
#[test]
fn dropping_the_stream_unsubscribes() {
    use crate::{CatchMask, Device, MockBox};
    let mock = MockBox::new();
    let device = Device::with_mock(mock.clone());
    {
        let _stream = device.catch_events(CatchMask::all()).unwrap();
    } // stream dropped here -> CATCH(0)
    let catch_frames: Vec<_> = mock
        .recorded_frames()
        .into_iter()
        .filter(|f| f.ty == FrameType::Catch)
        .collect();
    assert_eq!(catch_frames.first().unwrap().payload, vec![0x1F]);
    assert_eq!(catch_frames.last().unwrap().payload, vec![0x00]);
}

#[cfg(feature = "mock")]
#[test]
fn pushed_events_arrive_on_the_stream() {
    use crate::{Button, CatchEvent, CatchMask, Device, MockBox, Usage};
    use std::time::Duration;
    let mock = MockBox::new();
    let device = Device::with_mock(mock.clone());
    let stream = device.catch_events(CatchMask::all()).unwrap();
    mock.push_motion(0, 1_000, 5, -7, 1);
    mock.push_usages(1, 2_000, &[Usage::from(Button::Side1)]);

    let CatchEvent::Motion(r) = stream.recv_timeout(Duration::from_secs(1)).expect("motion") else {
        panic!("expected a motion event");
    };
    assert_eq!((r.dx, r.dy, r.dz), (5, -7, 1));
    assert_eq!(r.ts_us, 1_000);

    let CatchEvent::Usages(u) = stream.recv_timeout(Duration::from_secs(1)).expect("usages") else {
        panic!("expected a usage event");
    };
    assert!(u.is_held(Button::Side1));
}

#[cfg(feature = "mock")]
#[test]
fn catch_buffer_drops_oldest_on_overflow() {
    use crate::{CatchEvent, CatchMask, Device, MockBox};
    use std::time::{Duration, Instant};
    let mock = MockBox::new();
    let device = Device::with_mock(mock.clone());
    let stream = device.catch_events(CatchMask::all()).unwrap();
    // Push well past the 256-deep buffer without draining; the oldest get evicted.
    const TOTAL: u16 = 300;
    const KEPT: u16 = 256; // CATCH_CAPACITY
    for i in 0..TOTAL {
        mock.push_motion((i & 0xff) as u8, i as u32, i as i16, 0, 0); // dx is a monotonic marker
    }
    let want_dropped = (TOTAL - KEPT) as u64;
    let deadline = Instant::now() + Duration::from_secs(2);
    while stream.dropped() < want_dropped && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        stream.dropped(),
        want_dropped,
        "exactly the overflow count was dropped"
    );
    let CatchEvent::Motion(first) = stream
        .recv_timeout(Duration::from_secs(1))
        .expect("survived")
    else {
        panic!("expected a motion event");
    };
    assert_eq!(
        first.dx,
        (TOTAL - KEPT) as i16,
        "the oldest events were dropped, the newest kept"
    );
}

#[cfg(feature = "mock")]
#[test]
fn timestamps_reach_the_consumer_as_the_wire_value() {
    use crate::{CatchEvent, CatchMask, Device, MockBox};
    use std::time::Duration;
    let mock = MockBox::new();
    let device = Device::with_mock(mock.clone());
    let stream = device.catch_events(CatchMask::all()).unwrap();

    // Handed over raw, like CatchState::dropped and the rolling SEQ: the wrap is the consumer's to
    // notice, so a stamp near u32::MAX followed by a small one must arrive untouched, not accumulated.
    let before = u32::MAX - 500;
    mock.push_motion(0, before, 1, 0, 0);
    mock.push_motion(1, 500, 2, 0, 0);

    let mut seen = Vec::new();
    for _ in 0..2 {
        let CatchEvent::Motion(m) = stream.recv_timeout(Duration::from_secs(1)).expect("motion")
        else {
            panic!("expected a motion event");
        };
        seen.push(m.ts_us);
    }
    assert_eq!(seen, vec![before, 500]);
}
