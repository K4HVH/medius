//! `CATCH` (§3.9): the subscription address space, the three event frames, `RESP(CATCH)`, the HEALTH
//! bit and the EventStream lifecycle. Bytes are pinned to the firmware wire format in ctrl_proto.h.
use std::time::Duration;

#[cfg(feature = "mock")]
use crate::Usage;
use crate::protocol::command::catch_payload;
use crate::protocol::opcode::{CATCH_CLS_ANY, CATCH_ID_ANY, H_CATCH_ON};
use crate::protocol::response::{Resp, parse_resp};
use crate::types::{
    Axis, BusEvent, Capture, CatchClass, CatchFilter, CatchState, Class, ClockDomain,
    ControlStatus, Direction, Health, MotionEvent, TrafficClass, TrafficEvent, UsageSnapshot,
};
use crate::{Button, Key};

#[test]
fn catch_payload_bytes() {
    // Subscribe to everything: class and id both wildcards, state 1.
    assert_eq!(
        catch_payload(CATCH_CLS_ANY, CATCH_ID_ANY, 0, 1, 0),
        [0xFF, 0xFF, 0xFF, 0, 1, 0]
    );
    // One vendor bulk endpoint, IN only, cut to 16 bytes.
    assert_eq!(catch_payload(7, 0x83, 1, 1, 16), [7, 0x83, 0x00, 1, 1, 16]);
    // The blanket clear a host sends to tear the whole table down.
    assert_eq!(
        catch_payload(CATCH_CLS_ANY, CATCH_ID_ANY, 0, 0, 0),
        [0xFF, 0xFF, 0xFF, 0, 0, 0]
    );
}

#[test]
fn catch_classes_match_the_wire() {
    // Classes 0..3 must equal LOCK's and INJECT's, or one vocabulary silently becomes two.
    assert_eq!(CatchClass::Button.as_u8(), 0);
    assert_eq!(CatchClass::Key.as_u8(), 1);
    assert_eq!(CatchClass::Media.as_u8(), 2);
    assert_eq!(CatchClass::Axis.as_u8(), 3);
    assert_eq!(CatchClass::HidIn.as_u8(), 4);
    assert_eq!(CatchClass::HidOut.as_u8(), 5);
    assert_eq!(CatchClass::VendorInterrupt.as_u8(), 6);
    assert_eq!(CatchClass::VendorBulk.as_u8(), 7);
    assert_eq!(CatchClass::Control.as_u8(), 8);
    assert_eq!(CatchClass::Emit.as_u8(), 9);
    assert_eq!(CatchClass::Bus.as_u8(), 10);
    assert_eq!(CatchClass::from_u8(10), Some(CatchClass::Bus));
    assert_eq!(CatchClass::from_u8(11), None);
    assert_eq!(CatchClass::from_u8(0xFF), None); // the wildcard is not a class
}

#[test]
fn the_two_class_vocabularies_agree() {
    // Class (INJECT/LOCK) and CatchClass are one vocabulary at one set of byte values. A caller
    // holding a UsageSnapshot's class has to be able to compare it to the filter that asked for it.
    for (c, want) in [
        (Class::Button, CatchClass::Button),
        (Class::Key, CatchClass::Key),
        (Class::Media, CatchClass::Media),
    ] {
        assert_eq!(CatchClass::from(c), want);
        assert_eq!(c.as_u8(), want.as_u8());
    }
    for t in TrafficClass::ALL {
        let c = CatchClass::from(t);
        assert_eq!(t.as_u8(), c.as_u8());
        assert!(c.is_traffic() && !c.is_input());
        assert_eq!(TrafficClass::try_from(c), Ok(t));
    }
    // The input half refuses to become a traffic class, and hands itself back to say which.
    assert_eq!(
        TrafficClass::try_from(CatchClass::Key),
        Err(CatchClass::Key)
    );
    assert!(CatchClass::Axis.is_input());
}

#[test]
fn direction_reads_three_ways_over_two_values() {
    assert_eq!(Direction::PRESS, Direction::Positive);
    assert_eq!(Direction::IN, Direction::Positive);
    assert_eq!(Direction::RELEASE, Direction::Negative);
    assert_eq!(Direction::OUT, Direction::Negative);
    assert!(Direction::Both.admits(Direction::Positive));
    assert!(Direction::Positive.admits(Direction::Both));
    assert!(!Direction::Positive.admits(Direction::Negative));
    assert_eq!(Direction::of_delta(3), Direction::Positive);
    assert_eq!(Direction::of_delta(-3), Direction::Negative);
    assert_eq!(Direction::of_delta(0), Direction::Both);
}

#[test]
fn capture_normalises_and_widens() {
    assert_eq!(Capture::Whole.bytes(), None);
    assert_eq!(
        Capture::First(0).bytes(),
        None,
        "First(0) is the whole packet"
    );
    assert_eq!(Capture::First(16).bytes(), Some(16));
    // 0 on the wire means whole, so whole beats every finite length in both orders.
    assert_eq!(
        Capture::First(16).widest(Capture::First(64)),
        Capture::First(64)
    );
    assert_eq!(
        Capture::First(64).widest(Capture::First(16)),
        Capture::First(64)
    );
    assert_eq!(Capture::First(16).widest(Capture::Whole), Capture::Whole);
    assert_eq!(Capture::Whole.widest(Capture::First(16)), Capture::Whole);
    assert_eq!(Capture::First(0).widest(Capture::First(16)), Capture::Whole);
}

#[test]
fn equality_covers_the_capture_and_addressing_does_not() {
    // The box dedups its table on (class, id, direction), so two filters differing only in capture
    // are ONE box entry, but a PartialEq that said they were equal meant assert_eq! passed
    // on two filters that behave differently. Addressing is same_address(); equality is equality.
    let a = CatchFilter::traffic(TrafficClass::VendorBulk, 0x83);
    let b = a.with_capture(Capture::First(16));
    assert_ne!(a, b);
    assert!(a.same_address(b));
    let c = a.with_direction(Direction::OUT);
    assert!(!a.same_address(c), "direction is part of the box's key");
}

#[test]
fn watching_an_input_is_written_like_locking_it() {
    // The whole point of the input constructors: `lock(Key::A, ..)` and `watch(Key::A)` name the same
    // thing the same way, with no id arithmetic at the call site.
    assert_eq!(
        CatchFilter::watch(Key::new(0x04)).wire(),
        (CatchClass::Key.as_u8(), 0x04)
    );
    assert_eq!(
        CatchFilter::watch(Button::Left).wire(),
        (CatchClass::Button.as_u8(), Button::Left.as_id() as u16)
    );
    assert_eq!(
        CatchFilter::watch_axis(Axis::Wheel).wire(),
        (CatchClass::Axis.as_u8(), Axis::Wheel.as_u16())
    );
    assert_eq!(
        CatchFilter::watch_class(Class::Media).wire(),
        (CatchClass::Media.as_u8(), CATCH_ID_ANY)
    );
    let all = CatchFilter::all_input();
    assert_eq!(all.len(), 4);
    assert!(all.iter().all(|f| f.class().unwrap().is_input()));
    assert!(all.iter().all(|f| f.id().is_none()));
}

#[test]
fn filter_builders_produce_the_right_wire_pair() {
    assert_eq!(
        CatchFilter::everything().wire(),
        (CATCH_CLS_ANY, CATCH_ID_ANY)
    );
    assert_eq!(
        CatchFilter::traffic_class(TrafficClass::Emit).wire(),
        (9, CATCH_ID_ANY)
    );
    assert_eq!(
        CatchFilter::traffic(TrafficClass::VendorBulk, 0x83).wire(),
        (7, 0x83)
    );
    let f = CatchFilter::traffic(TrafficClass::HidOut, 0x02)
        .outbound()
        .with_capture(Capture::First(24));
    assert_eq!(f.direction(), Direction::Negative);
    assert_eq!(f.capture(), Capture::First(24));
    assert_eq!(
        CatchFilter::watch(Key::new(0x04)).on_press().direction(),
        Direction::Positive
    );
    assert_eq!(
        CatchFilter::traffic_class(TrafficClass::HidIn)
            .inbound()
            .direction(),
        Direction::Positive
    );
}

#[test]
fn a_wildcard_class_carrying_a_real_id_addresses_nothing() {
    // `id` means something different in every class, so the box refuses this outright and the host
    // has to as well: reading it as a wildcard instead would subscribe to everything, with no error.
    assert!(CatchFilter::from_wire(CATCH_CLS_ANY, 5, 0, 0).is_none());
    assert!(CatchFilter::from_wire(CATCH_CLS_ANY, CATCH_ID_ANY, 0, 0).is_some());
    assert!(CatchFilter::from_wire(200, 5, 0, 0).is_none()); // unknown class
    assert!(CatchFilter::from_wire(7, 5, 9, 0).is_none()); // unknown direction
    let f = CatchFilter::from_wire(7, 0x83, 2, 16).unwrap();
    assert_eq!(f.class(), Some(CatchClass::VendorBulk));
    assert_eq!(f.id(), Some(0x83));
    assert_eq!(f.direction(), Direction::OUT);
    assert_eq!(f.capture(), Capture::First(16));
    assert_eq!(f.wire(), (7, 0x83));
}

#[test]
fn motion_event_decodes_with_its_clock_domain() {
    // [ts u32][clk u8][dx][dy][dz]: clk sits between ts and the axes, so every field after it
    // shifted by one when the domain byte was added.
    let p = [
        0x04, 0x03, 0x02, 0x01, 0, 0x2C, 0x01, 0xCE, 0xFF, 0xFF, 0xFF,
    ];
    let m = MotionEvent::from_payload(&p).unwrap();
    assert_eq!(m.ts_us, 0x0102_0304);
    assert_eq!(m.clock, ClockDomain::HostChip);
    assert_eq!((m.dx, m.dy, m.dz), (300, -50, -1));
    assert!(MotionEvent::from_payload(&p[..10]).is_none());
    // axes() names only what moved; all_axes() names all three whatever they are.
    let moved: Vec<_> = m.axes().collect();
    assert_eq!(moved, [(Axis::X, 300), (Axis::Y, -50), (Axis::Wheel, -1)]);
    let still = MotionEvent {
        dx: 0,
        dy: 0,
        dz: 4,
        ..m
    };
    assert_eq!(still.axes().collect::<Vec<_>>(), [(Axis::Wheel, 4)]);
    assert_eq!(still.all_axes().len(), 3);
}

#[test]
fn usage_snapshot_decodes_with_its_clock_domain() {
    // [ts u32][clk=0][cls=0 Button][dir=1 press][n=2] then two usages.
    let p = [0, 0, 0, 0, 0, 0, 1, 2, 0, 0, 0, 1, 0x04, 0x00];
    let s = UsageSnapshot::from_payload(&p).unwrap();
    assert_eq!(s.clock, ClockDomain::HostChip);
    assert_eq!(s.usages.len(), 2);
    assert_eq!(s.class, Class::Button);
    assert!(s.is_held(Button::Left));
    assert!(s.is_held(Key::new(0x04)));
}

#[test]
fn an_empty_snapshot_still_names_the_class_that_went_quiet() {
    // Releasing the last held usage is the edge a caller is waiting for, and it lists nothing. With
    // the class read off the first usage there was none to read, so "all buttons released" and "all
    // media released" were the same twelve bytes and neither could be routed.
    for (byte, want) in [(0u8, Class::Button), (1, Class::Key), (2, Class::Media)] {
        let p = [7, 0, 0, 0, 0, byte, 2, 0];
        let s = UsageSnapshot::from_payload(&p).unwrap();
        assert_eq!(s.class, want);
        assert_eq!(s.direction, Direction::Negative);
        assert!(s.usages.is_empty());
        assert_eq!(s.ts_us, 7);
    }
    assert!(UsageSnapshot::from_payload(&[7, 0, 0, 0, 0, 9, 2, 0]).is_none()); // unknown class
    assert!(UsageSnapshot::from_payload(&[7, 0, 0, 0, 0, 0, 9, 0]).is_none()); // unknown direction
    assert!(UsageSnapshot::from_payload(&[7, 0, 0, 0, 0, 0]).is_none()); // no direction byte at all
}

#[test]
fn a_cut_setup_packet_is_not_reported_as_a_data_stage() {
    // A control event captured under 8 bytes keeps only part of its setup packet. Falling through to
    // "the whole buffer is the data" handed a decoder a GET_DESCRIPTOR request labelled as the
    // descriptor it asked for: bytes that are real, in a field that makes them a lie.
    let p = [0, 0, 0, 0, 1, 8, 0, 0, 0, 0, 8, 0, 0x80, 0x06, 0x00, 0x01];
    let t = TrafficEvent::from_payload(&p).unwrap();
    assert_eq!(t.class, CatchClass::Control);
    assert_eq!(t.bytes.len(), 4);
    assert!(t.truncated());
    assert_eq!(t.setup(), None);
    assert!(t.data().is_empty(), "got {:?}", t.data());

    // The whole packet present: setup and data split where they should.
    let full = [
        0, 0, 0, 0, 1, 8, 0, 0, 0, 0, 10, 0, 0x80, 0x06, 0x00, 0x01, 0, 0, 0x12, 0, 0x12, 0x01,
    ];
    let t = TrafficEvent::from_payload(&full).unwrap();
    assert_eq!(t.setup().unwrap(), &[0x80, 0x06, 0x00, 0x01, 0, 0, 0x12, 0]);
    assert_eq!(t.data(), &[0x12, 0x01]);
}

#[test]
fn control_status_covers_every_answer_the_device_can_give() {
    let with_flags = |flags: u8| {
        let p = [0, 0, 0, 0, 1, 8, 0, 0, 0, flags, 0, 0];
        TrafficEvent::from_payload(&p).unwrap().control_status()
    };
    assert_eq!(with_flags(0x00), Some(ControlStatus::Ok));
    assert_eq!(with_flags(0xFD), Some(ControlStatus::Stalled));
    assert_eq!(with_flags(0xFE), Some(ControlStatus::Naked));
    // An unknown status stays unknown. A catch-all arm reported it as a timeout, so a future
    // firmware's new status would read as a device fault that never happened.
    assert_eq!(with_flags(0x42), Some(ControlStatus::Other(0x42)));
    // A class that is not Control has no control status at all, whatever its flags say.
    let p = [0, 0, 0, 0, 1, 7, 0x83, 0x00, 1, 0x01, 0, 0];
    assert_eq!(
        TrafficEvent::from_payload(&p).unwrap().control_status(),
        None
    );
}

#[test]
fn a_bulk_zero_length_packet_is_flagged_both_ways() {
    let zlp = |flags: u8| {
        let p = [0, 0, 0, 0, 1, 7, 0x83, 0x00, 1, flags, 0, 0];
        TrafficEvent::from_payload(&p).unwrap().bulk_zlp()
    };
    assert!(!zlp(0x00));
    assert!(!zlp(0x01)); // END alone is not a ZLP
    assert!(zlp(0x02));
    assert!(zlp(0x03)); // END and ZLP together
}

#[test]
fn traffic_event_decodes() {
    // [ts][clk=1][class=7 VendorBulk][id=0x0083][dir=1 IN][flags=1 END][true_len=4][4 bytes]
    let p = [
        0x04, 0x03, 0x02, 0x01, 1, 7, 0x83, 0x00, 1, 0x01, 0x04, 0x00, 0xDE, 0xAD, 0xBE, 0xEF,
    ];
    let t = TrafficEvent::from_payload(&p).unwrap();
    assert_eq!(t.ts_us, 0x0102_0304);
    assert_eq!(t.clock, ClockDomain::DeviceChip);
    assert_eq!(t.class, CatchClass::VendorBulk);
    assert_eq!(t.id, 0x83);
    assert_eq!(t.direction, Direction::IN);
    assert_eq!(t.true_len, 4);
    assert_eq!(t.bytes, [0xDE, 0xAD, 0xBE, 0xEF]);
    assert!(!t.truncated());
    assert!(t.bulk_end_of_transfer());
    assert!(!t.bulk_zlp());
    assert!(TrafficEvent::from_payload(&p[..11]).is_none());
    // An unknown class must not decode into a plausible one.
    let mut bad = p;
    bad[5] = 200;
    assert!(TrafficEvent::from_payload(&bad).is_none());
}

#[test]
fn truncation_is_visible() {
    // true_len 64 with 8 bytes delivered: without the flag a cut capture and a genuinely short
    // packet are indistinguishable, which is the whole reason true_len is on the wire.
    let mut p = vec![0, 0, 0, 0, 1, 6, 0x83, 0x00, 1, 0, 64, 0];
    p.extend_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
    let t = TrafficEvent::from_payload(&p).unwrap();
    assert_eq!(t.true_len, 64);
    assert_eq!(t.bytes.len(), 8);
    assert!(t.truncated());
}

#[test]
fn control_event_splits_setup_from_data() {
    let mut p = vec![0, 0, 0, 0, 1, 8, 0x00, 0x00, 1, 0x00, 10, 0];
    p.extend_from_slice(&[0xA1, 0x01, 0x00, 0x02, 0x00, 0x00, 0x02, 0x00]); // setup
    p.extend_from_slice(&[0xAA, 0xBB]); // data stage
    let t = TrafficEvent::from_payload(&p).unwrap();
    assert_eq!(
        t.setup().unwrap(),
        &[0xA1, 0x01, 0x00, 0x02, 0x00, 0x00, 0x02, 0x00]
    );
    assert_eq!(t.data(), &[0xAA, 0xBB]);
    assert_eq!(t.control_status(), Some(ControlStatus::Ok));
    // A STALL is what the real device answered, not the box's own refusal.
    let mut stalled = p.clone();
    stalled[9] = 0xFD;
    assert_eq!(
        TrafficEvent::from_payload(&stalled)
            .unwrap()
            .control_status(),
        Some(ControlStatus::Stalled)
    );
    // Other classes have no setup packet, and their whole payload is data.
    let other = [0u8, 0, 0, 0, 1, 9, 0, 0, 1, 0, 2, 0, 0x11, 0x22];
    let o = TrafficEvent::from_payload(&other).unwrap();
    assert!(o.setup().is_none());
    assert_eq!(o.data(), &[0x11, 0x22]);
    assert!(o.control_status().is_none());
}

#[test]
fn bus_event_decodes_its_kind() {
    let mk = |flags: u8, a: u8, b: u8| {
        TrafficEvent::from_payload(&[0, 0, 0, 0, 1, 10, 0xFF, 0xFF, 0, flags, 2, 0, a, b]).unwrap()
    };
    assert_eq!(mk(0, 0, 0).bus_event(), Some(BusEvent::Reset));
    assert_eq!(mk(3, 2, 0).bus_event(), Some(BusEvent::Configured(2)));
    assert_eq!(
        mk(5, 1, 3).bus_event(),
        Some(BusEvent::SetInterface {
            interface: 1,
            alt: 3
        })
    );
    assert_eq!(mk(9, 0, 0).bus_event(), Some(BusEvent::CloneDown));
    assert_eq!(mk(200, 0, 0).bus_event(), None);
}

#[test]
fn a_catch_event_answers_class_id_and_direction_uniformly() {
    use crate::types::CatchEvent;
    let motion = CatchEvent::Motion(
        MotionEvent::from_payload(&[0, 0, 0, 0, 0, 1, 0, 0xFF, 0xFF, 0, 0]).unwrap(),
    );
    assert_eq!(motion.class(), CatchClass::Axis);
    assert_eq!(motion.id(), None);
    // Both, not a guess: this report moved X positive and Y negative at once.
    assert_eq!(motion.direction(), Direction::Both);
    assert!(motion.bytes().is_empty());

    let usages =
        CatchEvent::Usages(UsageSnapshot::from_payload(&[0, 0, 0, 0, 0, 1, 1, 0]).unwrap());
    assert_eq!(usages.class(), CatchClass::Key);
    assert_eq!(usages.id(), None);
    assert_eq!(usages.direction(), Direction::PRESS);
    assert!(usages.bytes().is_empty());

    let traffic = CatchEvent::Traffic(
        TrafficEvent::from_payload(&[0, 0, 0, 0, 1, 7, 0x83, 0, 2, 0, 1, 0, 9]).unwrap(),
    );
    assert_eq!(traffic.class(), CatchClass::VendorBulk);
    assert_eq!(traffic.id(), Some(0x83));
    assert_eq!(traffic.direction(), Direction::OUT);
    assert_eq!(traffic.bytes(), &[9]);
}

#[test]
fn catch_state_decodes_header_entries_and_clock() {
    let mut p = vec![7u8, 0x01]; // what, flags: table full
    p.extend_from_slice(&0x0102_0304u32.to_le_bytes()); // dropped
    p.extend_from_slice(&(-250i32).to_le_bytes()); // clk_offset_us
    p.extend_from_slice(&(-1500i32).to_le_bytes()); // clk_rate_ppb
    p.extend_from_slice(&90u16.to_le_bytes()); // clk_delay_us
    p.extend_from_slice(&12u16.to_le_bytes()); // clk_age_ms
    p.push(2); // n
    p.extend_from_slice(&[3, 0xFF, 0xFF, 0, 0, 7, 0]); // axis blanket, 7 drops
    p.extend_from_slice(&[7, 0x83, 0x00, 1, 16, 0xA0, 0x0F]); // vend bulk ep, 4000 drops

    let c = CatchState::from_payload(&p).unwrap();
    assert!(c.table_full);
    assert_eq!(c.dropped, 0x0102_0304);
    assert_eq!(c.clock.offset_us, -250);
    assert_eq!(c.clock.rate_ppb, Some(-1500));
    assert_eq!(c.clock.delay_us, 90);
    assert_eq!(c.clock.error_bound_us(), 45);
    assert_eq!(c.clock.age.unwrap().as_millis(), 12);
    assert_eq!(c.entries.len(), 2);
    assert_eq!(c.entries[0].filter.class(), Some(CatchClass::Axis));
    assert_eq!(c.entries[0].filter.id(), None);
    assert_eq!(c.entries[0].dropped, 7);
    assert_eq!(c.entries[1].filter.id(), Some(0x83));
    assert_eq!(c.entries[1].filter.capture(), Capture::First(16));
    assert_eq!(c.entries[1].dropped, 4000);
    assert!(CatchState::from_payload(&p[..18]).is_none());
}

#[test]
fn one_unrecognised_entry_does_not_discard_the_whole_reply() {
    // A class this build does not know must cost that entry and nothing else. Failing the parse threw
    // away the other entries, every drop count and the clock estimate, and the caller saw it as a
    // missing reply rather than a partially-understood one.
    let mut p = vec![7u8, 0];
    p.extend_from_slice(&99u32.to_le_bytes());
    p.extend_from_slice(&0i32.to_le_bytes());
    p.extend_from_slice(&0i32.to_le_bytes());
    p.extend_from_slice(&90u16.to_le_bytes());
    p.extend_from_slice(&12u16.to_le_bytes());
    p.push(3);
    p.extend_from_slice(&[3, 0xFF, 0xFF, 0, 0, 7, 0]); // axis blanket
    p.extend_from_slice(&[77, 0x01, 0x00, 0, 0, 1, 0]); // a class from some later firmware
    p.extend_from_slice(&[7, 0x83, 0x00, 1, 16, 5, 0]); // vendor bulk

    let c = CatchState::from_payload(&p).unwrap();
    assert_eq!(c.dropped, 99);
    assert_eq!(c.clock.delay_us, 90);
    assert_eq!(c.entries.len(), 2);
    assert_eq!(c.entries[0].filter.class(), Some(CatchClass::Axis));
    assert_eq!(c.entries[1].filter.class(), Some(CatchClass::VendorBulk));
    assert_eq!(c.entries[1].dropped, 5);
}

#[test]
fn a_device_stamp_translates_into_the_host_domain() {
    // Only the None path was covered, so a sign inversion here would have shipped green. The box
    // reports host-minus-device, so a device stamp moves FORWARD by a positive offset.
    let build = |offset: i32, age: u16| {
        let mut p = vec![7u8, 0];
        p.extend_from_slice(&0u32.to_le_bytes());
        p.extend_from_slice(&offset.to_le_bytes());
        p.extend_from_slice(&0i32.to_le_bytes());
        p.extend_from_slice(&90u16.to_le_bytes());
        p.extend_from_slice(&age.to_le_bytes());
        p.push(0);
        CatchState::from_payload(&p).unwrap().clock
    };
    assert_eq!(build(250, 12).to_host_domain(1_000), Some(1_250));
    assert_eq!(build(-250, 12).to_host_domain(1_000), Some(750));
    // Past the u32 wrap it stays arithmetic rather than saturating: the caller gets an i64.
    assert_eq!(
        build(1_000, 12).to_host_domain(u32::MAX),
        Some(4_294_968_295)
    );
    assert_eq!(build(250, u16::MAX).to_host_domain(1_000), None);
}

#[test]
fn an_unfitted_rate_is_distinct_from_a_measured_zero() {
    // Two different answers over the same wire field. A fitted 0 says the crystals are matched; the
    // sentinel says nothing has been fitted, which is the state a link too busy for clean exchanges
    // stays in, exactly when assuming no drift costs the most.
    let build = |rate: i32| {
        let mut p = vec![7u8, 0];
        p.extend_from_slice(&0u32.to_le_bytes());
        p.extend_from_slice(&0i32.to_le_bytes());
        p.extend_from_slice(&rate.to_le_bytes());
        p.extend_from_slice(&90u16.to_le_bytes());
        p.extend_from_slice(&12u16.to_le_bytes());
        p.push(0);
        CatchState::from_payload(&p).unwrap()
    };
    let none = build(crate::protocol::opcode::CLK_RATE_NONE);
    assert_eq!(none.clock.rate_ppb, None);
    assert_eq!(none.clock.drift_us_over(Duration::from_secs(10)), 0);

    let flat = build(0);
    assert_eq!(flat.clock.rate_ppb, Some(0));
    assert_eq!(flat.clock.drift_us_over(Duration::from_secs(10)), 0);
    assert_ne!(none.clock.rate_ppb, flat.clock.rate_ppb);

    // And a real rate really extrapolates: 20 ppm over 10 s is 200 us, which is far past the 45 us
    // error bound the same reply advertises.
    let drifting = build(20_000);
    assert_eq!(drifting.clock.drift_us_over(Duration::from_secs(10)), 200);
    assert_eq!(drifting.clock.drift_us_over(Duration::from_secs(0)), 0);
}

#[test]
fn no_clock_estimate_is_distinct_from_a_zero_offset() {
    // 0xFFFF age is the box saying it has no estimate. Reporting it as "zero milliseconds old" would
    // make a caller trust an offset that was never measured.
    let mut p = vec![7u8, 0];
    p.extend_from_slice(&0u32.to_le_bytes());
    p.extend_from_slice(&0i32.to_le_bytes());
    p.extend_from_slice(&0i32.to_le_bytes());
    p.extend_from_slice(&0u16.to_le_bytes());
    p.extend_from_slice(&u16::MAX.to_le_bytes());
    p.push(0);
    let c = CatchState::from_payload(&p).unwrap();
    assert_eq!(c.clock.age, None);
    assert_eq!(c.clock.offset_us, 0);
    assert_eq!(c.clock.to_host_domain(1000), None);
    assert!(c.is_empty());
}

#[test]
fn decode_catch_through_parse_resp() {
    let mut p = vec![7u8, 0];
    p.extend_from_slice(&[0; 16]);
    p.push(0);
    assert!(matches!(parse_resp(&p), Some(Resp::Catch(_))));
}

#[test]
fn health_catch_on_bit_roundtrips() {
    let h = Health::from_flags(H_CATCH_ON);
    assert!(h.catch_on);
    assert_eq!(h.to_flags(), H_CATCH_ON);
}

#[cfg(feature = "mock")]
mod with_mock {
    use super::*;
    use crate::types::Input;
    use crate::{CatchEvent, Device, Error, FrameType, MockBox};

    fn bulk(id: u16) -> CatchFilter {
        CatchFilter::traffic(TrafficClass::VendorBulk, id)
    }

    #[test]
    fn subscribing_sends_one_frame_per_entry_and_dropping_unsubscribes() {
        let mock = MockBox::new();
        let dev = Device::with_mock(mock.clone());
        {
            let _s = dev
                .catch_events([
                    CatchFilter::everything().with_capture(Capture::First(16)),
                    bulk(0x83),
                ])
                .unwrap();
            let sent: Vec<_> = mock
                .recorded_frames()
                .into_iter()
                .filter(|f| f.ty == FrameType::Catch)
                .collect();
            assert_eq!(sent.len(), 2);
            assert!(sent.iter().all(|f| f.payload[4] == 1)); // both are subscribes
        }
        // The guard fires on drop; the box must be told to stop.
        let last = mock
            .recorded_frames()
            .into_iter()
            .rfind(|f| f.ty == FrameType::Catch)
            .unwrap();
        assert_eq!(last.payload[4], 0);
    }

    #[test]
    fn an_empty_subscription_is_refused() {
        // Otherwise the caller holds a stream that never yields, which reads as a dead box.
        let dev = Device::with_mock(MockBox::new());
        assert!(matches!(
            dev.catch_events([]).unwrap_err(),
            Error::EmptySubscription
        ));
        assert!(matches!(
            dev.input_events([]).unwrap_err(),
            Error::EmptySubscription
        ));
    }

    #[test]
    fn a_capture_on_an_input_class_is_refused_not_ignored() {
        // The firmware's input taps never read the capture: they pass NULL where the traffic tap
        // passes &snaplen. Accepting one is therefore a public knob wired to nothing.
        let dev = Device::with_mock(MockBox::new());
        let err = dev
            .catch_events([CatchFilter::watch_class(Class::Key).with_capture(Capture::First(8))])
            .unwrap_err();
        assert!(
            matches!(err, Error::CaptureNotApplicable { class } if class == CatchClass::Key),
            "got {err:?}"
        );
        // The wildcard covers traffic, so capping it is exactly right and must still be allowed.
        let _ok = dev
            .catch_events([CatchFilter::everything().with_capture(Capture::First(16))])
            .expect("everything() may be capped");
    }

    #[test]
    fn an_exact_id_of_the_blanket_sentinel_is_refused() {
        // 0xFFFF is the every-id sentinel on the wire. An exact subscription to it becomes the whole
        // class the moment it is sent: a much wider stream than asked for, with no error to say so.
        // Only a media usage is 16 bits wide enough to express it.
        let dev = Device::with_mock(MockBox::new());
        let err = dev
            .catch_events([CatchFilter::watch(Usage::new(Class::Media, 0xFFFF))])
            .unwrap_err();
        assert!(
            matches!(err, Error::ReservedId { class, id } if class == CatchClass::Media && id == 0xFFFF),
            "got {err:?}"
        );
        // The blanket itself is still fine: it means the whole class on purpose.
        let _ok = dev
            .catch_events([CatchFilter::watch_class(Class::Media)])
            .expect("a blanket is not an exact id");
        // And one below the sentinel is a perfectly good usage.
        let _fine = dev
            .catch_events([CatchFilter::watch(Usage::new(Class::Media, 0xFFFE))])
            .expect("0xFFFE addresses one usage");
    }

    #[test]
    fn input_events_refuses_what_it_cannot_decode() {
        let dev = Device::with_mock(MockBox::new());
        let err = dev
            .input_events([CatchFilter::traffic_class(TrafficClass::VendorBulk)])
            .unwrap_err();
        assert!(
            matches!(err, Error::NotAnInputFilter { class } if class == CatchClass::VendorBulk),
            "got {err:?}"
        );
        assert!(matches!(
            dev.input_events([CatchFilter::everything()]).unwrap_err(),
            Error::WildcardNotInput
        ));
        // A press-only subscription never sees the release, so the NEXT press cannot be told from a
        // chord: the edge decoder would stop reporting a key after its first press.
        assert!(matches!(
            dev.input_events([CatchFilter::watch(Key::new(0x04)).on_press()])
                .unwrap_err(),
            Error::HalfEdgeInputFilter
        ));
        let _ok = dev
            .input_events(CatchFilter::all_input())
            .expect("all_input is what input_events is for");
    }

    #[test]
    fn pushed_events_of_every_kind_arrive_on_the_stream() {
        let mock = MockBox::new();
        let dev = Device::with_mock(mock.clone());
        let s = dev.catch_events([CatchFilter::everything()]).unwrap();

        mock.push_motion(0, 1_000, 5, -7, 1);
        mock.push_usages(
            1,
            2_000,
            Class::Button,
            Direction::PRESS,
            &[Usage::from(Button::Side1)],
        );
        mock.push_traffic(
            2,
            3_000,
            ClockDomain::DeviceChip,
            CatchClass::Emit,
            0,
            Direction::IN,
            0,
            3,
            &[1, 2, 3],
        );

        match s.recv().unwrap() {
            CatchEvent::Motion(m) => assert_eq!((m.dx, m.dy, m.dz), (5, -7, 1)),
            other => panic!("expected motion, got {other:?}"),
        }
        match s.recv().unwrap() {
            CatchEvent::Usages(u) => assert!(u.is_held(Button::Side1)),
            other => panic!("expected usages, got {other:?}"),
        }
        match s.recv().unwrap() {
            CatchEvent::Traffic(t) => {
                assert_eq!(t.class, CatchClass::Emit);
                assert_eq!(t.clock, ClockDomain::DeviceChip);
                assert_eq!(t.bytes, [1, 2, 3]);
            }
            other => panic!("expected traffic, got {other:?}"),
        }
    }

    #[test]
    fn the_stream_is_an_iterator() {
        let mock = MockBox::new();
        let dev = Device::with_mock(mock.clone());
        let s = dev.catch_events([CatchFilter::everything()]).unwrap();
        for i in 0..3u8 {
            mock.push_motion(i, 100 + i as u32, i as i16 + 1, 0, 0);
        }
        let dx: Vec<i16> = (&s)
            .into_iter()
            .take(3)
            .map(|e| match e {
                CatchEvent::Motion(m) => m.dx,
                other => panic!("expected motion, got {other:?}"),
            })
            .collect();
        assert_eq!(dx, [1, 2, 3]);
    }

    #[test]
    fn every_variant_reports_its_stamp_and_domain_uniformly() {
        let mock = MockBox::new();
        let dev = Device::with_mock(mock.clone());
        let s = dev.catch_events([CatchFilter::everything()]).unwrap();
        mock.push_motion(0, 111, 1, 0, 0);
        mock.push_traffic(
            1,
            222,
            ClockDomain::DeviceChip,
            CatchClass::Bus,
            0xFFFF,
            Direction::Both,
            0,
            2,
            &[0, 0],
        );
        let a = s.recv().unwrap();
        let b = s.recv().unwrap();
        assert_eq!((a.ts_us(), a.clock()), (111, ClockDomain::HostChip));
        assert_eq!((b.ts_us(), b.clock()), (222, ClockDomain::DeviceChip));
    }

    #[test]
    fn the_widest_capture_reaches_the_box() {
        // The box holds ONE entry per address, so two subscribers naming it with different captures
        // have to be resolved rather than have one win. Taking either arbitrarily meant a caller
        // asking for whole packets started receiving cut ones the moment unrelated code in the same
        // process subscribed with a shorter one. Whole beats every finite length.
        let capture_sent_to_box = |a: Capture, b: Capture| {
            let mock = MockBox::new();
            let dev = Device::with_mock(mock.clone());
            let _first = dev.catch_events([bulk(0x83).with_capture(a)]).unwrap();
            let _second = dev.catch_events([bulk(0x83).with_capture(b)]).unwrap();
            mock.recorded_frames()
                .iter()
                .filter(|f| f.ty == FrameType::Catch)
                .filter_map(|f| f.payload.get(5).copied())
                .next_back()
                .expect("a CATCH frame")
        };
        let f = Capture::First;
        assert_eq!(capture_sent_to_box(f(16), f(64)), 64);
        assert_eq!(capture_sent_to_box(f(64), f(16)), 64);
        assert_eq!(capture_sent_to_box(f(16), Capture::Whole), 0);
        assert_eq!(capture_sent_to_box(Capture::Whole, f(16)), 0);
    }

    #[test]
    fn a_narrow_entry_does_not_cut_a_broad_subscribers_packets() {
        // The box resolves an event to its most SPECIFIC matching entry and captures at THAT entry's
        // length. Resolving only between identical addresses is therefore not enough: a blanket
        // subscriber asking for whole packets had them cut to 8 the moment another caller subscribed
        // to one endpoint with a shorter capture (on that endpoint only), and nothing marked the cut.
        let mock = MockBox::new();
        let dev = Device::with_mock(mock.clone());
        let _broad = dev.catch_events([CatchFilter::everything()]).unwrap();
        let _narrow = dev
            .catch_events([bulk(0x83).with_capture(Capture::First(8))])
            .unwrap();
        let captures: Vec<u8> = mock
            .recorded_frames()
            .iter()
            .filter(|f| f.ty == FrameType::Catch)
            .filter_map(|f| f.payload.get(5).copied())
            .collect();
        assert!(!captures.is_empty());
        assert!(
            captures.iter().all(|s| *s == 0),
            "every entry must still capture whole packets, got {captures:?}"
        );
    }

    #[test]
    fn a_more_specific_entry_does_not_widen_a_broader_one() {
        // The flip side of the fold, and the direction that must NOT happen: an exact endpoint asking
        // for whole packets does not force the blanket entry, which serves every OTHER endpoint,
        // to stop capping. Folding by "overlaps at all" instead of "is no more specific" put the
        // blanket at whole packets and handed the link every byte of every bulk pipe.
        let mock = MockBox::new();
        let dev = Device::with_mock(mock.clone());
        let _capped = dev
            .catch_events([CatchFilter::traffic_class(TrafficClass::VendorBulk)
                .with_capture(Capture::First(16))])
            .unwrap();
        let _whole = dev.catch_events([bulk(0x83)]).unwrap();
        let sent: Vec<(u16, u8)> = mock
            .recorded_frames()
            .iter()
            .filter(|f| f.ty == FrameType::Catch && f.payload[4] == 1)
            .map(|f| {
                (
                    u16::from_le_bytes([f.payload[1], f.payload[2]]),
                    f.payload[5],
                )
            })
            .collect();
        assert!(
            sent.contains(&(CATCH_ID_ANY, 16)),
            "the blanket stays capped, got {sent:?}"
        );
        assert!(
            sent.contains(&(0x83, 0)),
            "the exact endpoint gets whole packets, got {sent:?}"
        );
    }

    #[test]
    fn direction_ranks_in_the_fold_the_same_way_it_ranks_in_the_box() {
        // Direction is part of specificity, not merely a filter, so it has to rank in the capture
        // fold too. A BOTH entry asking for whole packets covers the OUT events that resolve to a
        // narrower OUT entry, so that entry has to be widened. Otherwise the broad subscriber's OUT
        // packets come back cut, on that endpoint only.
        let sent = |broad: Capture, narrow: Capture| {
            let mock = MockBox::new();
            let dev = Device::with_mock(mock.clone());
            let _b = dev.catch_events([bulk(0x83).with_capture(broad)]).unwrap();
            let _n = dev
                .catch_events([bulk(0x83).outbound().with_capture(narrow)])
                .unwrap();
            let mut out: Vec<(u8, u8)> = mock
                .recorded_frames()
                .iter()
                .filter(|f| f.ty == FrameType::Catch && f.payload[4] == 1)
                .map(|f| (f.payload[3], f.payload[5]))
                .collect();
            out.dedup();
            out
        };
        // Broad wants whole packets: the narrow OUT entry is widened to match.
        let widened = sent(Capture::Whole, Capture::First(8));
        assert!(
            widened.contains(&(Direction::Both.as_u8(), 0))
                && widened.contains(&(Direction::OUT.as_u8(), 0)),
            "got {widened:?}"
        );
        // The reverse must NOT happen: a narrower OUT entry asking for whole packets does not stop
        // the BOTH entry, which also serves every IN event, from capping.
        let kept = sent(Capture::First(8), Capture::Whole);
        assert!(
            kept.contains(&(Direction::Both.as_u8(), 8))
                && kept.contains(&(Direction::OUT.as_u8(), 0)),
            "got {kept:?}"
        );
    }

    #[test]
    fn a_subscription_past_the_box_s_table_is_refused_not_truncated() {
        // The box holds 32 entries and drops the rest, reporting it only in a flag nothing was
        // obliged to read, so the caller got a stream missing whatever did not fit.
        use crate::protocol::opcode::CATCH_MAX_ENTRIES;
        let mock = MockBox::new();
        let dev = Device::with_mock(mock.clone());
        let fits: Vec<CatchFilter> = (0..CATCH_MAX_ENTRIES as u16)
            .map(|i| bulk(0x80 + i))
            .collect();
        let _ok = dev
            .catch_events(fits.clone())
            .expect("exactly the table size fits");

        let one_more = CatchFilter::traffic(TrafficClass::VendorInterrupt, 0x99);
        let err = dev.catch_events([one_more]).unwrap_err();
        assert!(
            matches!(
                err,
                Error::CatchTableFull { needed, limit }
                    if needed == CATCH_MAX_ENTRIES + 1 && limit == CATCH_MAX_ENTRIES
            ),
            "got {err:?}"
        );
        // And the refusal must not have left the registry holding it: the union is unchanged, so a
        // later subscribe of something that DOES fit still works.
        drop(_ok);
        let _after = dev
            .catch_events([CatchFilter::traffic_class(TrafficClass::Bus)])
            .expect("the refused subscription left nothing behind");
    }

    #[test]
    fn an_unchanged_entry_is_not_re_sent() {
        // catch_sync runs holding catch_lock, and every frame is a blocking serial write. Re-sending
        // the whole table on any change made dropping one stream cost a write per entry, ahead of
        // every other subscribe and the keepalive.
        let mock = MockBox::new();
        let dev = Device::with_mock(mock.clone());
        let a = dev
            .catch_events([
                bulk(0x83),
                bulk(0x84),
                CatchFilter::traffic_class(TrafficClass::Bus),
            ])
            .unwrap();
        let b = dev
            .catch_events([CatchFilter::traffic(TrafficClass::VendorInterrupt, 0x85)])
            .unwrap();
        let before = mock
            .recorded_frames()
            .iter()
            .filter(|f| f.ty == FrameType::Catch)
            .count();
        drop(b);
        let sent: Vec<Vec<u8>> = mock
            .recorded_frames()
            .iter()
            .filter(|f| f.ty == FrameType::Catch)
            .skip(before)
            .map(|f| f.payload.clone())
            .collect();
        // Exactly one frame: the unsubscribe for what B alone wanted. A's three entries are already
        // in the box at the right capture and must not be rewritten.
        assert_eq!(sent.len(), 1, "sent {sent:?}");
        assert_eq!(sent[0][4], 0, "the one frame is an unsubscribe");
        assert_eq!(sent[0][0], CatchClass::VendorInterrupt.as_u8());
        drop(a);
    }

    #[test]
    fn a_changed_capture_still_reaches_the_box() {
        // The flip side: capture is deliberately not part of a filter's ADDRESS, so the key-set
        // difference cannot see it move and it has to be compared explicitly.
        let mock = MockBox::new();
        let dev = Device::with_mock(mock.clone());
        let a = dev
            .catch_events([bulk(0x83).with_capture(Capture::First(8))])
            .unwrap();
        let before = mock
            .recorded_frames()
            .iter()
            .filter(|f| f.ty == FrameType::Catch)
            .count();
        // A second subscriber wants whole packets on the same address: the entry must be rewritten.
        let _b = dev.catch_events([bulk(0x83)]).unwrap();
        let sent: Vec<Vec<u8>> = mock
            .recorded_frames()
            .iter()
            .filter(|f| f.ty == FrameType::Catch)
            .skip(before)
            .map(|f| f.payload.clone())
            .collect();
        assert_eq!(sent.len(), 1, "sent {sent:?}");
        assert_eq!(sent[0][5], 0, "rewritten at the widest capture");
        drop(a);
    }

    #[test]
    fn duplicate_addresses_in_one_call_take_the_widest() {
        // The same rule inside a single subscribe. Collecting straight into a set kept whichever
        // duplicate happened to be listed last, so the two orderings of one pair disagreed.
        let sent = |filters: [CatchFilter; 2]| {
            let mock = MockBox::new();
            let dev = Device::with_mock(mock.clone());
            let _s = dev.catch_events(filters).unwrap();
            mock.recorded_frames()
                .iter()
                .filter(|f| f.ty == FrameType::Catch)
                .filter_map(|f| f.payload.get(5).copied())
                .next_back()
                .expect("a CATCH frame")
        };
        let a = bulk(0x83);
        let f = Capture::First;
        assert_eq!(sent([a.with_capture(f(16)), a.with_capture(f(64))]), 64);
        assert_eq!(sent([a.with_capture(f(64)), a.with_capture(f(16))]), 64);
        assert_eq!(
            sent([a.with_capture(f(16)), a.with_capture(Capture::Whole)]),
            0
        );
        assert_eq!(
            sent([a.with_capture(Capture::Whole), a.with_capture(f(16))]),
            0
        );
    }

    #[test]
    fn an_exact_input_filter_receives_its_class_and_diffs_it() {
        // The input frames carry content, not an address, so routing them means reading the address
        // out of the content. Sending the wire's wildcard id instead made every exact-id input
        // subscription match nothing at all: the box accepted the entry, listed it in RESP(CATCH),
        // counted no drops, and the stream stayed empty.
        //
        // A snapshot lists what is HELD, so the RELEASE of Left is the snapshot that no longer
        // mentions Left. Delivering only snapshots that CONTAIN the subscriber's usage therefore
        // discards precisely the edge it was waiting for, and only when some other subscriber's
        // usage is still held, so a single-subscriber test cannot see it.
        let mock = MockBox::new();
        let dev = Device::with_mock(mock.clone());
        let s = dev
            .catch_events([CatchFilter::watch(Button::Left)])
            .unwrap();
        let next = |what: &str| match s.recv_timeout(Duration::from_secs(1)) {
            Some(CatchEvent::Usages(u)) => u,
            other => panic!("expected {what}, got {other:?}"),
        };
        mock.push_usages(
            0,
            1_000,
            Class::Button,
            Direction::PRESS,
            &[Usage::from(Button::Left)],
        );
        assert!(next("the press").is_held(Button::Left));

        // Left released while another subscriber's Side1 is still held. The box lists only Side1 --
        // and that snapshot IS how this subscriber learns Left came up.
        mock.push_usages(
            1,
            2_000,
            Class::Button,
            Direction::RELEASE,
            &[Usage::from(Button::Side1)],
        );
        assert!(!next("the release").is_held(Button::Left));

        // A different class still is not its business. Asserted after an event that MUST arrive, so
        // this cannot pass merely by outrunning the reader thread.
        mock.push_usages(
            2,
            3_000,
            Class::Key,
            Direction::PRESS,
            &[Usage::from(Key::new(0x04))],
        );
        mock.push_usages(3, 4_000, Class::Button, Direction::RELEASE, &[]);
        assert!(next("the empty button snapshot").usages.is_empty());
    }

    #[test]
    fn an_input_filter_direction_selects_the_edge() {
        // The box emits on both edges as soon as any other subscriber holds a wider entry, so without
        // the edge on the wire a press-only subscription received releases too.
        let mock = MockBox::new();
        let dev = Device::with_mock(mock.clone());
        let press = dev
            .catch_events([CatchFilter::watch_class(Class::Key).on_press()])
            .unwrap();
        mock.push_usages(0, 1_000, Class::Key, Direction::RELEASE, &[]);
        mock.push_usages(
            1,
            2_000,
            Class::Key,
            Direction::PRESS,
            &[Usage::from(Key::new(0x04))],
        );
        match press.recv_timeout(Duration::from_secs(1)) {
            Some(CatchEvent::Usages(u)) => {
                assert_eq!(u.direction, Direction::PRESS);
                assert!(
                    u.is_held(Key::new(0x04)),
                    "the release must not arrive first"
                );
            }
            other => panic!("expected the press, got {other:?}"),
        }
    }

    #[test]
    fn an_exact_axis_filter_receives_only_that_axis() {
        let mock = MockBox::new();
        let dev = Device::with_mock(mock.clone());
        let s = dev
            .catch_events([CatchFilter::watch_axis(Axis::Wheel)])
            .unwrap();
        mock.push_motion(0, 1_000, 40, -9, 0); // X and Y only
        assert!(s.try_recv().is_none());
        mock.push_motion(1, 2_000, 0, 0, 1);
        match s.recv().unwrap() {
            CatchEvent::Motion(m) => assert_eq!(m.dz, 1),
            other => panic!("expected motion, got {other:?}"),
        }
    }

    #[test]
    fn an_axis_direction_selects_the_sign_of_the_movement() {
        // An axis has no press or release, so its direction is the sign of the delta, the same
        // reading an axis LOCK uses. A subscriber asking for wheel-up must not be handed wheel-down.
        let mock = MockBox::new();
        let dev = Device::with_mock(mock.clone());
        let up = dev
            .catch_events(
                [CatchFilter::watch_axis(Axis::Wheel).with_direction(Direction::Positive)],
            )
            .unwrap();
        mock.push_motion(0, 1_000, 0, 0, -1);
        assert!(up.try_recv().is_none());
        mock.push_motion(1, 2_000, 0, 0, 3);
        assert!(matches!(up.recv().unwrap(), CatchEvent::Motion(m) if m.dz == 3));
    }

    #[test]
    fn a_release_to_nothing_reaches_the_class_that_released() {
        // The empty snapshot is the release of the last held usage, and it lists nothing. It has to
        // reach the subscriber for its own class and nobody else's: it used to go to everyone
        // subscribed to anything, so a vendor-bulk trace received keyboard events.
        let mock = MockBox::new();
        let dev = Device::with_mock(mock.clone());
        let keys = dev
            .catch_events([CatchFilter::watch_class(Class::Key)])
            .unwrap();
        let traffic = dev.catch_events([bulk(0x83)]).unwrap();
        mock.push_usages(0, 1_000, Class::Key, Direction::RELEASE, &[]);
        match keys.recv_timeout(Duration::from_secs(1)).expect("release") {
            CatchEvent::Usages(u) => assert!(u.usages.is_empty() && u.class == Class::Key),
            other => panic!("expected the empty key snapshot, got {other:?}"),
        }
        assert!(
            traffic.try_recv().is_none(),
            "an empty snapshot is not everyone's"
        );
    }

    #[test]
    fn each_subscriber_gets_only_what_it_asked_for() {
        // The box holds ONE table (the union of every subscription), so without per-subscriber
        // matching a caller's stream would change shape whenever unrelated code elsewhere in the
        // process subscribed to something else.
        let mock = MockBox::new();
        let dev = Device::with_mock(mock.clone());
        let b = dev.catch_events([bulk(0x83)]).unwrap();
        let bus = dev
            .catch_events([CatchFilter::traffic_class(TrafficClass::Bus)])
            .unwrap();

        mock.push_traffic(
            0,
            1,
            ClockDomain::DeviceChip,
            CatchClass::VendorBulk,
            0x83,
            Direction::IN,
            0,
            2,
            &[1, 2],
        );
        mock.push_traffic(
            1,
            2,
            ClockDomain::DeviceChip,
            CatchClass::Bus,
            0xFFFF,
            Direction::Both,
            0,
            2,
            &[0, 0],
        );

        match b.recv().unwrap() {
            CatchEvent::Traffic(t) => assert_eq!(t.class, CatchClass::VendorBulk),
            other => panic!("expected vendor bulk, got {other:?}"),
        }
        assert!(
            b.try_recv().is_none(),
            "the bus event must not reach the bulk subscriber"
        );

        match bus.recv().unwrap() {
            CatchEvent::Traffic(t) => assert_eq!(t.class, CatchClass::Bus),
            other => panic!("expected bus, got {other:?}"),
        }
        assert!(
            bus.try_recv().is_none(),
            "the bulk event must not reach the bus subscriber"
        );
    }

    #[test]
    fn an_endpoint_filter_excludes_its_neighbours() {
        let mock = MockBox::new();
        let dev = Device::with_mock(mock.clone());
        let s = dev.catch_events([bulk(0x83)]).unwrap();
        mock.push_traffic(
            0,
            1,
            ClockDomain::DeviceChip,
            CatchClass::VendorBulk,
            0x84,
            Direction::IN,
            0,
            1,
            &[9],
        );
        mock.push_traffic(
            1,
            2,
            ClockDomain::DeviceChip,
            CatchClass::VendorBulk,
            0x83,
            Direction::IN,
            0,
            1,
            &[7],
        );
        match s.recv().unwrap() {
            CatchEvent::Traffic(t) => assert_eq!((t.id, t.bytes[0]), (0x83, 7)),
            other => panic!("expected 0x83, got {other:?}"),
        }
    }

    #[test]
    fn catch_buffer_drops_oldest_on_overflow() {
        const TOTAL: u16 = 300;
        const KEPT: usize = 256; // CATCH_CAPACITY
        let mock = MockBox::new();
        let dev = Device::with_mock(mock.clone());
        let s = dev.catch_events([CatchFilter::everything()]).unwrap();
        for i in 0..TOTAL {
            mock.push_motion(i as u8, 0, i as i16, 0, 0);
        }
        // The reader delivers on its own thread, so wait for the count rather than assuming it has
        // caught up: asserting immediately makes this pass or fail on scheduling.
        let want = (TOTAL as usize - KEPT) as u64;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while s.dropped() < want && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(s.dropped(), want);
        // The survivor is the oldest of what remains, so the newest are the ones kept.
        match s.recv().unwrap() {
            CatchEvent::Motion(m) => assert_eq!(m.dx, (TOTAL as usize - KEPT) as i16),
            other => panic!("expected motion, got {other:?}"),
        }
    }

    #[test]
    fn timestamps_reach_the_consumer_as_the_wire_value() {
        let mock = MockBox::new();
        let dev = Device::with_mock(mock.clone());
        let s = dev.catch_events([CatchFilter::everything()]).unwrap();
        mock.push_motion(0, u32::MAX - 500, 1, 0, 0);
        mock.push_motion(1, 500, 1, 0, 0);
        let first = s.recv().unwrap().ts_us();
        let second = s.recv().unwrap().ts_us();
        assert_eq!(first, u32::MAX - 500);
        assert_eq!(second, 500); // a wrap is the consumer's to notice, not ours to smooth over
    }

    #[test]
    fn input_events_turn_snapshots_into_edges() {
        let mock = MockBox::new();
        let dev = Device::with_mock(mock.clone());
        let mut s = dev.input_events(CatchFilter::all_input()).unwrap();
        let a = Usage::from(Key::new(0x04));
        let b = Usage::from(Key::new(0x05));

        mock.push_usages(0, 1_000, Class::Key, Direction::PRESS, &[a]);
        mock.push_usages(1, 2_000, Class::Key, Direction::PRESS, &[a, b]);
        mock.push_usages(2, 3_000, Class::Key, Direction::RELEASE, &[b]);
        mock.push_usages(3, 4_000, Class::Key, Direction::RELEASE, &[]);
        mock.push_motion(4, 5_000, 3, -4, 0);

        let next = |s: &mut crate::InputStream| s.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(next(&mut s).input, Input::Press(a));
        assert_eq!(next(&mut s).input, Input::Press(b));
        let e = next(&mut s);
        assert_eq!(e.input, Input::Release(a));
        assert_eq!(e.ts_us, 3_000, "the edge carries its report's stamp");
        assert_eq!(next(&mut s).input, Input::Release(b));
        assert_eq!(
            next(&mut s).input,
            Input::Motion {
                dx: 3,
                dy: -4,
                dz: 0
            }
        );
    }

    #[test]
    fn one_report_can_carry_several_edges() {
        // A swap in a single report: A comes up and B goes down at once. Both edges have to surface,
        // and the release first, or a consumer counting held usages goes negative.
        let mock = MockBox::new();
        let dev = Device::with_mock(mock.clone());
        let mut s = dev.input_events(CatchFilter::all_input()).unwrap();
        let a = Usage::from(Button::Left);
        let b = Usage::from(Button::Side1);
        mock.push_usages(0, 1_000, Class::Button, Direction::PRESS, &[a]);
        mock.push_usages(1, 2_000, Class::Button, Direction::PRESS, &[b]);
        let next = |s: &mut crate::InputStream| s.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(next(&mut s).input, Input::Press(a));
        assert_eq!(next(&mut s).input, Input::Release(a));
        assert_eq!(next(&mut s).input, Input::Press(b));
        assert_eq!(s.held(Class::Button), &[b]);
        assert!(s.held(Class::Key).is_empty());
    }

    #[test]
    fn an_input_stream_gets_only_the_usages_it_asked_for() {
        // A snapshot is the CLASS's state, and the box sends every held usage of that class as soon
        // as ANY subscriber in the process widens the table. Routing must stay class-only or the
        // release edge is lost, so the DECODER has to filter, or a one-key stream reports edges for
        // every key someone else subscribed to.
        let mock = MockBox::new();
        let dev = Device::with_mock(mock.clone());
        let f = Usage::from(Key::new(0x09));
        let g = Usage::from(Key::new(0x0A));
        let mut narrow = dev
            .input_events([CatchFilter::watch(Key::new(0x09))])
            .unwrap();
        let _wide = dev
            .catch_events([CatchFilter::watch_class(Class::Key)])
            .unwrap();

        mock.push_usages(0, 1_000, Class::Key, Direction::PRESS, &[g]);
        mock.push_usages(1, 2_000, Class::Key, Direction::PRESS, &[g, f]);
        mock.push_usages(2, 3_000, Class::Key, Direction::RELEASE, &[g]);
        assert_eq!(
            narrow.recv_timeout(Duration::from_secs(1)).unwrap().input,
            Input::Press(f),
            "the other subscriber's key must not surface here"
        );
        assert_eq!(
            narrow.recv_timeout(Duration::from_secs(1)).unwrap().input,
            Input::Release(f)
        );
        assert_eq!(narrow.held(Class::Key), &[]);
        assert!(narrow.recv_timeout(Duration::from_millis(100)).is_none());
    }

    #[test]
    fn a_malformed_snapshot_cannot_wedge_the_held_set() {
        // Two defences the wire is not obliged to respect. A usage listed twice fired two presses
        // with no release between them and left `held` a multiset. A usage whose own class byte
        // disagrees with the frame's was filed under the frame's class, where no snapshot of its real
        // class could ever release it.
        let mock = MockBox::new();
        let dev = Device::with_mock(mock.clone());
        let mut s = dev.input_events(CatchFilter::all_input()).unwrap();
        let a = Usage::from(Key::new(0x04));
        let button = Usage::from(Button::Left);

        mock.push_usages(0, 1_000, Class::Key, Direction::PRESS, &[a, a]);
        assert_eq!(
            s.recv_timeout(Duration::from_secs(1)).unwrap().input,
            Input::Press(a)
        );
        assert!(
            s.recv_timeout(Duration::from_millis(100)).is_none(),
            "once, not twice"
        );
        assert_eq!(s.held(Class::Key), &[a]);

        // A button riding inside a KEY snapshot is dropped, not filed under Key.
        mock.push_usages(1, 2_000, Class::Key, Direction::PRESS, &[a, button]);
        assert!(s.recv_timeout(Duration::from_millis(100)).is_none());
        assert_eq!(s.held(Class::Key), &[a]);
        assert!(s.held(Class::Button).is_empty());
    }

    #[test]
    fn a_report_that_moved_nothing_is_not_an_input_event() {
        // The routing fallback delivers an unaddressable motion report to every axis subscriber. Left
        // undecoded that becomes a phantom zero-delta event, the exact shape the emission
        // suppression work was about.
        let mock = MockBox::new();
        let dev = Device::with_mock(mock.clone());
        let mut s = dev.input_events(CatchFilter::all_input()).unwrap();
        mock.push_motion(0, 1_000, 0, 0, 0);
        assert!(s.recv_timeout(Duration::from_millis(150)).is_none());
        mock.push_motion(1, 2_000, 0, 0, -2);
        assert_eq!(
            s.recv_timeout(Duration::from_secs(1)).unwrap().input,
            Input::Motion {
                dx: 0,
                dy: 0,
                dz: -2
            }
        );
    }

    #[test]
    fn a_snapshot_that_changes_nothing_yields_nothing() {
        // Two identical snapshots are one state, not two events. The deadline in recv_timeout must
        // not restart on a report that decoded to no edge at all.
        let mock = MockBox::new();
        let dev = Device::with_mock(mock.clone());
        let mut s = dev.input_events(CatchFilter::all_input()).unwrap();
        mock.push_usages(0, 1_000, Class::Key, Direction::RELEASE, &[]);
        mock.push_usages(1, 2_000, Class::Media, Direction::RELEASE, &[]);
        // And the deadline has to SURVIVE those reports rather than restart on each one: recomputing
        // the full timeout per loop turned a 200 ms budget into 700 ms under a stream of empties.
        let began = std::time::Instant::now();
        assert!(s.recv_timeout(Duration::from_millis(150)).is_none());
        let waited = began.elapsed();
        assert!(
            waited >= Duration::from_millis(140) && waited < Duration::from_millis(400),
            "waited {waited:?} for a 150 ms budget"
        );
        assert!(s.try_recv().is_none());
    }

    #[test]
    fn the_input_stream_tracks_each_class_separately() {
        // One held-set per class. Sharing one made a key press look like a button release.
        let mock = MockBox::new();
        let dev = Device::with_mock(mock.clone());
        let mut s = dev.input_events(CatchFilter::all_input()).unwrap();
        let btn = Usage::from(Button::Left);
        let key = Usage::from(Key::new(0x04));
        mock.push_usages(0, 1_000, Class::Button, Direction::PRESS, &[btn]);
        mock.push_usages(1, 2_000, Class::Key, Direction::PRESS, &[key]);
        let next = |s: &mut crate::InputStream| s.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(next(&mut s).input, Input::Press(btn));
        assert_eq!(next(&mut s).input, Input::Press(key));
        assert!(s.recv_timeout(Duration::from_millis(150)).is_none());
        assert_eq!(s.held(Class::Button), &[btn]);
        assert_eq!(s.held(Class::Key), &[key]);
    }

    #[test]
    fn watching_one_key_needs_no_diffing_at_all() {
        // The shape the docs lead with: one exact usage, and is_held answers the question.
        let mock = MockBox::new();
        let dev = Device::with_mock(mock.clone());
        let f = Usage::from(Key::new(0x09));
        let mut s = dev
            .input_events([CatchFilter::watch(Key::new(0x09))])
            .unwrap();
        mock.push_usages(0, 1_000, Class::Key, Direction::PRESS, &[f]);
        mock.push_usages(1, 2_000, Class::Key, Direction::RELEASE, &[]);
        let next = |s: &mut crate::InputStream| s.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(next(&mut s).input, Input::Press(f));
        assert_eq!(next(&mut s).input, Input::Release(f));
    }
}

#[cfg(feature = "mock")]
mod timeline {
    use super::*;
    use crate::types::{CatchEvent, Timeline};
    use std::time::Instant;

    fn motion_at(ts_us: u32) -> CatchEvent {
        let mut p = vec![0u8; 11];
        p[..4].copy_from_slice(&ts_us.to_le_bytes());
        p[5] = 1; // dx = 1, so the report moved something
        CatchEvent::Motion(MotionEvent::from_payload(&p).unwrap())
    }

    fn traffic_at(ts_us: u32) -> CatchEvent {
        let mut p = vec![0u8; 12];
        p[..4].copy_from_slice(&ts_us.to_le_bytes());
        p[4] = 1; // device chip
        p[5] = 7; // vendor bulk
        CatchEvent::Traffic(TrafficEvent::from_payload(&p).unwrap())
    }

    #[test]
    fn box_microseconds_unwrap_past_the_rollover() {
        // 32 bits of microseconds is 71.6 minutes. A consumer subtracting two raw stamps across the
        // wrap gets a number about 4295 seconds wrong, in the negative direction.
        let mut t = Timeline::new();
        assert_eq!(
            t.box_us(&motion_at(u32::MAX - 500)),
            (u32::MAX - 500) as u64
        );
        assert_eq!(t.box_us(&motion_at(500)), (1u64 << 32) + 500);
        assert_eq!(t.box_us(&motion_at(1_500)), (1u64 << 32) + 1_500);
        // Re-reading the same event must not advance the epoch again.
        assert_eq!(t.box_us(&motion_at(1_500)), (1u64 << 32) + 1_500);
    }

    #[test]
    fn the_two_domains_unwrap_independently() {
        // Interleaving a device-chip stamp below a host-chip one is a domain change, not a wrap.
        let mut t = Timeline::new();
        assert_eq!(t.box_us(&motion_at(10_000)), 10_000);
        assert_eq!(t.box_us(&traffic_at(5)), 5);
        assert_eq!(t.box_us(&motion_at(10_001)), 10_001);
        assert_eq!(t.box_us(&traffic_at(6)), 6);
    }

    #[test]
    fn the_timeline_takes_a_decoded_input_event_too() {
        // The two features have to compose: a caller on the input_events path must be able to put an
        // edge on this machine's clock without dropping back to the raw stream. Taking only a
        // CatchEvent forced exactly that, and cost the caller the edge decoding to buy a timestamp.
        use crate::types::{Input, InputEvent, Usage};
        let mut t = Timeline::new();
        let origin = Instant::now();
        let edge = InputEvent {
            ts_us: 5_000,
            clock: ClockDomain::HostChip,
            input: Input::Press(Usage::from(Key::new(0x04))),
        };
        let a = t.observe_at(&edge, origin + Duration::from_millis(10));
        assert_eq!(a.box_us, 5_000);
        // And it shares one domain state with the raw events, so a mixed reader stays consistent.
        let raw = t.observe_at(&motion_at(6_000), origin + Duration::from_millis(11));
        assert_eq!(raw.host.duration_since(a.host), Duration::from_millis(1));
        assert_eq!(t.samples(ClockDomain::HostChip), 2);
        // The underlying frame types work directly as well, on the same unwrapped domain.
        let mut p = vec![0u8; 11];
        p[..4].copy_from_slice(&7_000u32.to_le_bytes());
        let m = MotionEvent::from_payload(&p).unwrap();
        assert_eq!(t.box_us(&m), 7_000);
    }

    #[test]
    fn a_reset_forgets_the_rollover_for_a_rebooted_chip() {
        let mut t = Timeline::new();
        assert_eq!(t.box_us(&motion_at(u32::MAX - 5)), (u32::MAX - 5) as u64);
        t.reset(ClockDomain::HostChip);
        assert_eq!(t.box_us(&motion_at(7)), 7, "a reboot is not a wrap");
        assert_eq!(t.samples(ClockDomain::HostChip), 0);
    }

    #[test]
    fn the_host_mapping_follows_the_fastest_sample() {
        // The error is one-sided (an event can arrive late but never early), so the mapping tracks
        // the MINIMUM lag. An average would be dragged by every slow delivery and never recover.
        let origin = Instant::now();
        let mut t = Timeline::new();
        // 50 ms of lag on the first sample; nothing better has been seen, so that is the floor and
        // the event reads as having no excess.
        let a = t.observe_at(&motion_at(1_000), origin + Duration::from_millis(50));
        assert_eq!(a.box_us, 1_000);
        assert_eq!(a.excess, Duration::ZERO);
        // 39 ms later on the wall for 1 ms of box time: 38 ms of excess against the same floor.
        let b = t.observe_at(&motion_at(2_000), origin + Duration::from_millis(89));
        assert!(b.host > a.host, "the later event maps later");
        assert!(b.excess > Duration::from_millis(37), "got {:?}", b.excess);
        // A relatively faster delivery lowers the floor, and the next event is measured against it.
        let _fast = t.observe_at(&motion_at(3_000), origin + Duration::from_millis(51));
        let d = t.observe_at(&motion_at(4_000), origin + Duration::from_millis(52));
        assert_eq!(d.excess, Duration::ZERO, "the new floor is the reference");
        assert_eq!(t.samples(ClockDomain::HostChip), 4);
        assert_eq!(t.samples(ClockDomain::DeviceChip), 0);
    }

    #[test]
    fn a_small_correction_is_absorbed_and_a_large_one_re_anchors() {
        // The floor only improves, and an improvement shifts later events earlier. A SMALL shift is
        // smoothed so the timeline does not visibly run backwards. A LARGE one is the estimate having
        // been wrong, and holding the wrong answer to stay monotonic wedged the stream for as long as
        // the error lasted: a 5 s first sample pinned 5000 events to one instant.
        let origin = Instant::now();
        let mut t = Timeline::new();
        // Sub-millisecond correction: absorbed, no regression.
        let a = t.observe_at(&motion_at(1_000), origin + Duration::from_millis(10));
        let b = t.observe_at(&motion_at(1_100), origin + Duration::from_micros(10_050));
        assert!(b.host >= a.host, "{:?} < {:?}", b.host, a.host);

        // A 40 ms correction re-anchors instead of wedging, and the very next event is already on the
        // improved mapping rather than clamped to the stale one.
        let mut t = Timeline::new();
        let slow = t.observe_at(&motion_at(1_000), origin + Duration::from_millis(50));
        let fast = t.observe_at(&motion_at(1_100), origin + Duration::from_millis(10));
        assert!(fast.host < slow.host, "a large correction re-anchors");
        let next = t.observe_at(&motion_at(2_100), origin + Duration::from_millis(11));
        assert_eq!(
            next.host.duration_since(fast.host),
            Duration::from_millis(1)
        );
        assert_eq!(next.excess, Duration::ZERO);
    }

    #[test]
    fn the_host_mapping_is_anchored_to_the_arrival_not_to_the_box_stamp() {
        // Deleting the floor entirely (host = box_us alone) left every other timeline test green.
        // The mapping has to put an event NEAR the moment it arrived, not near the box's own epoch,
        // or it is not a host clock at all.
        let origin = Instant::now();
        let mut t = Timeline::new();
        // A box that booted an hour before this process: stamps are huge, arrivals are not.
        let far = 3_600_000_000u32;
        let s = t.observe_at(&motion_at(far), origin + Duration::from_millis(20));
        assert_eq!(s.box_us, far as u64);
        assert_eq!(
            s.host.duration_since(origin),
            Duration::from_millis(20),
            "the first sample maps to its own arrival"
        );
    }

    #[test]
    fn excess_is_measured_in_the_unit_it_says() {
        // Both the crate and the FFI built `excess` from the wrong Duration constructor at one point;
        // asserting only ZERO and a one-sided bound let a 1000x unit error through.
        let origin = Instant::now();
        let mut t = Timeline::new();
        t.observe_at(&motion_at(1_000), origin + Duration::from_millis(10));
        let late = t.observe_at(&motion_at(2_000), origin + Duration::from_millis(37));
        // 1000 us of box time against 27 ms of wall time: 26 ms late, exactly.
        assert_eq!(late.excess, Duration::from_millis(26));
    }

    #[test]
    fn an_out_of_order_event_is_not_a_rollover() {
        // The box drains its taps through strict-priority queues, and BOTH clock domains span several
        // of them, so a later-tapped event can arrive first. Treating every backward step as a wrap
        // turned a 1 us inversion into a permanent 71.6-minute jump that also destroyed the floor.
        let mut t = Timeline::new();
        assert_eq!(t.box_us(&motion_at(10_000)), 10_000);
        assert_eq!(
            t.box_us(&motion_at(9_999)),
            9_999,
            "1 us inversion, not a wrap"
        );
        assert_eq!(t.box_us(&motion_at(11_000)), 11_000);
        // A whole second of reordering is still reordering.
        assert_eq!(t.box_us(&motion_at(10_000_000)), 10_000_000);
        assert_eq!(t.box_us(&motion_at(9_000_000)), 9_000_000);
        // And the real wrap still works. Walk up to it the way a running box does: a single jump
        // of more than half the range is genuinely ambiguous and is read as reordering.
        let mut t = Timeline::new();
        for step in [
            1_000u32,
            1_000_000_000,
            2_000_000_000,
            3_000_000_000,
            u32::MAX - 5,
        ] {
            assert_eq!(t.box_us(&motion_at(step)), step as u64);
        }
        assert_eq!(t.box_us(&motion_at(20)), (1u64 << 32) + 20);
        assert_eq!(
            t.box_us(&motion_at(u32::MAX - 2)),
            (u32::MAX - 2) as u64,
            "a straggler from the previous epoch stays in it"
        );
    }

    #[test]
    fn each_domain_keeps_its_own_floor() {
        // Sharing one floor across the two chips made a device-chip stamp land wherever the host
        // chip's offset happened to put it. The two clocks are unrelated; only per-domain works.
        let origin = Instant::now();
        let mut t = Timeline::new();
        // Host chip: stamps near zero, arriving at 10 ms.
        let h = t.observe_at(&motion_at(1_000), origin + Duration::from_millis(10));
        // Device chip: stamps an hour ahead, arriving at 11 ms. With a shared floor this lands an
        // hour away.
        let d = t.observe_at(
            &traffic_at(3_600_001_000),
            origin + Duration::from_millis(11),
        );
        assert_eq!(h.host.duration_since(origin), Duration::from_millis(10));
        assert_eq!(d.host.duration_since(origin), Duration::from_millis(11));
        assert_eq!(t.samples(ClockDomain::HostChip), 1);
        assert_eq!(t.samples(ClockDomain::DeviceChip), 1);
    }

    #[test]
    fn a_reset_clears_one_domains_floor_and_leaves_the_other_alone() {
        // Keeping the old boot's floor put every post-reboot event an epoch away; clearing BOTH
        // domains threw away a good estimate for a chip that never restarted.
        let origin = Instant::now();
        let mut t = Timeline::new();
        t.observe_at(&motion_at(1_000), origin + Duration::from_millis(500));
        t.observe_at(&traffic_at(1_000), origin + Duration::from_millis(20));
        t.reset(ClockDomain::HostChip);
        assert_eq!(t.samples(ClockDomain::HostChip), 0);
        assert_eq!(
            t.samples(ClockDomain::DeviceChip),
            1,
            "the other chip is untouched"
        );
        // The rebooted chip's first event maps to its own arrival, not to the stale 500 ms floor.
        let after = t.observe_at(&motion_at(7), origin + Duration::from_millis(600));
        assert_eq!(
            after.host.duration_since(origin),
            Duration::from_millis(600)
        );
        assert_eq!(after.box_us, 7);
        // And the surviving domain still has its floor: 1000 us on, 1 ms later.
        let other = t.observe_at(&traffic_at(2_000), origin + Duration::from_millis(30));
        assert_eq!(other.host.duration_since(origin), Duration::from_millis(21));
    }

    #[test]
    fn the_floor_window_lets_a_stale_estimate_recover() {
        // An all-time minimum cannot be right: the two crystals drift at up to 20 ppm, so a floor
        // taken an hour ago is 72 ms wrong and only ever gets worse. The window bounds how old the
        // floor can be, which means it has to be able to RISE.
        let origin = Instant::now();
        let mut t = Timeline::new();
        // One unusually fast delivery, then a long run of steady ones 5 ms slower.
        t.observe_at(&motion_at(0), origin);
        let mut last = Duration::ZERO;
        for i in 1..=9_000u32 {
            let s = t.observe_at(
                &motion_at(i * 1_000),
                origin + Duration::from_micros(i as u64 * 1_000 + 5_000),
            );
            last = s.excess;
        }
        assert_eq!(
            last,
            Duration::ZERO,
            "the floor rose to the steady delivery"
        );
    }

    #[test]
    fn a_steady_stream_maps_onto_the_boxs_own_spacing() {
        // Once the floor is found, host instants are spaced exactly as the box stamped them, whatever
        // the delivery jitter was. The jitter here never beats the first sample, so the floor holds.
        let origin = Instant::now();
        let mut t = Timeline::new();
        let mut out = Vec::new();
        for (i, jitter) in [0u64, 7, 2, 31, 1].iter().enumerate() {
            let ts = 1_000 + i as u32 * 1_000;
            out.push(
                t.observe_at(
                    &motion_at(ts),
                    origin + Duration::from_millis(10 + i as u64 + jitter),
                )
                .host,
            );
        }
        for w in out.windows(2) {
            assert_eq!(
                w[1].duration_since(w[0]),
                Duration::from_millis(1),
                "1000 us apart on the box is 1 ms apart here"
            );
        }
    }
}
