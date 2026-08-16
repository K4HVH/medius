//! `CATCH` (§3.9): the subscription address space, the three event frames, `RESP(CATCH)`, the HEALTH
//! bit and the EventStream lifecycle. Bytes are pinned to the firmware wire format in ctrl_proto.h.
use std::time::Duration;

use crate::protocol::command::catch_payload;
use crate::protocol::opcode::{CATCH_CLS_ANY, CATCH_ID_ANY, H_CATCH_ON};
use crate::protocol::response::{Resp, parse_resp};
use crate::types::{
    Axis, BusEvent, CatchClass, CatchFilter, CatchState, Class, ClockDomain, ControlStatus, Health,
    LockDirection, MotionEvent, TrafficEvent, UsageSnapshot,
};
use crate::{Button, Key, Usage};

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
fn snaplen_is_not_part_of_a_filter_s_identity() {
    // The box dedups its table on (class, id, direction). If two filters differing only in snaplen
    // counted as distinct here, two subscribers would each believe they had their own entry while
    // the second silently overwrote the first's capture length.
    use std::collections::BTreeSet;
    let a = CatchFilter::addr(CatchClass::VendorBulk, 0x83).snaplen(0);
    let b = CatchFilter::addr(CatchClass::VendorBulk, 0x83).snaplen(16);
    assert_eq!(a, b);
    let set: BTreeSet<CatchFilter> = [a, b].into_iter().collect();
    assert_eq!(set.len(), 1);
    // Direction still separates them, because the box treats it as part of the key.
    let c = CatchFilter::addr(CatchClass::VendorBulk, 0x83).direction(LockDirection::Negative);
    assert_ne!(a, c);
}

#[test]
fn filter_builders_produce_the_right_wire_pair() {
    assert_eq!(CatchFilter::all().wire(), (CATCH_CLS_ANY, CATCH_ID_ANY));
    assert_eq!(
        CatchFilter::class(CatchClass::Emit).wire(),
        (9, CATCH_ID_ANY)
    );
    assert_eq!(
        CatchFilter::addr(CatchClass::VendorBulk, 0x83).wire(),
        (7, 0x83)
    );
    let f = CatchFilter::addr(CatchClass::HidOut, 0x02)
        .direction(LockDirection::Negative)
        .snaplen(24);
    assert_eq!(f.direction, LockDirection::Negative);
    assert_eq!(f.snaplen, 24);
}

#[test]
fn motion_event_decodes_with_its_clock_domain() {
    // [ts u32][clk u8][dx][dy][dz] -- clk sits between ts and the axes, so every field after it
    // shifted by one when the domain byte was added.
    let p = [
        0x04, 0x03, 0x02, 0x01, 0, 0x2C, 0x01, 0xCE, 0xFF, 0xFF, 0xFF,
    ];
    let m = MotionEvent::from_payload(&p).unwrap();
    assert_eq!(m.ts_us, 0x0102_0304);
    assert_eq!(m.clock, ClockDomain::HostChip);
    assert_eq!((m.dx, m.dy, m.dz), (300, -50, -1));
    assert!(MotionEvent::from_payload(&p[..10]).is_none());
}

#[test]
fn usage_snapshot_decodes_with_its_clock_domain() {
    // [ts u32][clk=0][cls=0 Button][n=2] then two usages.
    let p = [0, 0, 0, 0, 0, 0, 2, 0, 0, 0, 1, 0x04, 0x00];
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
        let p = [7, 0, 0, 0, 0, byte, 0];
        let s = UsageSnapshot::from_payload(&p).unwrap();
        assert_eq!(s.class, want);
        assert!(s.usages.is_empty());
        assert_eq!(s.ts_us, 7);
    }
    assert!(UsageSnapshot::from_payload(&[7, 0, 0, 0, 0, 9, 0]).is_none()); // unknown class
    assert!(UsageSnapshot::from_payload(&[7, 0, 0, 0, 0]).is_none()); // no class byte at all
}

#[test]
fn a_cut_setup_packet_is_not_reported_as_a_data_stage() {
    // A control event with snaplen under 8 keeps only part of its setup packet. Falling through to
    // "the whole buffer is the data" handed a decoder a GET_DESCRIPTOR request labelled as the
    // descriptor it asked for -- bytes that are real, in a field that makes them a lie.
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
    assert_eq!(t.direction, LockDirection::Positive);
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
    assert_eq!(c.entries[0].filter.class, Some(CatchClass::Axis));
    assert_eq!(c.entries[0].filter.id, None);
    assert_eq!(c.entries[0].dropped, 7);
    assert_eq!(c.entries[1].filter.id, Some(0x83));
    assert_eq!(c.entries[1].filter.snaplen, 16);
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
    assert_eq!(c.entries[0].filter.class, Some(CatchClass::Axis));
    assert_eq!(c.entries[1].filter.class, Some(CatchClass::VendorBulk));
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
    // sentinel says nothing has been fitted -- which is the state a link too busy for clean exchanges
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
    use crate::{CatchEvent, Device, FrameType, MockBox};

    #[test]
    fn subscribing_sends_one_frame_per_entry_and_dropping_unsubscribes() {
        let mock = MockBox::new();
        let dev = Device::with_mock(mock.clone());
        {
            let _s = dev
                .catch_events([
                    CatchFilter::all().snaplen(16),
                    CatchFilter::addr(CatchClass::VendorBulk, 0x83),
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
    fn pushed_events_of_every_kind_arrive_on_the_stream() {
        let mock = MockBox::new();
        let dev = Device::with_mock(mock.clone());
        let s = dev.catch_events([CatchFilter::all()]).unwrap();

        mock.push_motion(0, 1_000, 5, -7, 1);
        mock.push_usages(1, 2_000, Class::Button, &[Usage::from(Button::Side1)]);
        mock.push_traffic(
            2,
            3_000,
            ClockDomain::DeviceChip,
            CatchClass::Emit,
            0,
            LockDirection::Positive,
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
    fn every_variant_reports_its_stamp_and_domain_uniformly() {
        let mock = MockBox::new();
        let dev = Device::with_mock(mock.clone());
        let s = dev.catch_events([CatchFilter::all()]).unwrap();
        mock.push_motion(0, 111, 1, 0, 0);
        mock.push_traffic(
            1,
            222,
            ClockDomain::DeviceChip,
            CatchClass::Bus,
            0xFFFF,
            LockDirection::Both,
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
    fn the_widest_snaplen_reaches_the_box() {
        // The box holds ONE entry per address, so two subscribers naming it with different capture
        // lengths have to be resolved rather than have one silently win. Taking either arbitrarily
        // meant a caller asking for whole packets started receiving cut ones the moment unrelated
        // code in the same process subscribed with a shorter snaplen. 0 = whole packet, so 0 wins.
        let snaplen_sent_to_box = |a: u8, b: u8| {
            let mock = MockBox::new();
            let dev = Device::with_mock(mock.clone());
            let _first = dev
                .catch_events([CatchFilter::addr(CatchClass::VendorBulk, 0x83).snaplen(a)])
                .unwrap();
            let _second = dev
                .catch_events([CatchFilter::addr(CatchClass::VendorBulk, 0x83).snaplen(b)])
                .unwrap();
            mock.recorded_frames()
                .iter()
                .filter(|f| f.ty == FrameType::Catch)
                .filter_map(|f| f.payload.get(5).copied())
                .next_back()
                .expect("a CATCH frame")
        };
        assert_eq!(snaplen_sent_to_box(16, 64), 64);
        assert_eq!(snaplen_sent_to_box(64, 16), 64);
        assert_eq!(
            snaplen_sent_to_box(16, 0),
            0,
            "whole packet beats a cut one"
        );
        assert_eq!(snaplen_sent_to_box(0, 16), 0, "and in either order");
    }

    #[test]
    fn an_exact_button_filter_receives_its_own_usage_event() {
        // The input frames carry content, not an address, so routing them means reading the address
        // out of the content. Sending the wire's wildcard id instead made every exact-id input
        // subscription match nothing at all -- and silently: the box accepted the entry, listed it in
        // RESP(CATCH), counted no drops, and the stream simply stayed empty.
        let mock = MockBox::new();
        let dev = Device::with_mock(mock.clone());
        let s = dev
            .catch_events([CatchFilter::addr(
                CatchClass::Button,
                Button::Left.as_id() as u16,
            )])
            .unwrap();
        mock.push_usages(0, 1_000, Class::Button, &[Usage::from(Button::Left)]);
        match s.recv().unwrap() {
            CatchEvent::Usages(u) => assert!(u.is_held(Button::Left)),
            other => panic!("expected the button snapshot, got {other:?}"),
        }
        // And a snapshot holding only a DIFFERENT button is not this subscriber's business.
        mock.push_usages(1, 2_000, Class::Button, &[Usage::from(Button::Side1)]);
        assert!(s.try_recv().is_none());
    }

    #[test]
    fn an_exact_axis_filter_receives_only_that_axis() {
        let mock = MockBox::new();
        let dev = Device::with_mock(mock.clone());
        let s = dev
            .catch_events([CatchFilter::addr(CatchClass::Axis, Axis::Wheel.as_u16())])
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
        // An axis has no press or release, so its direction is the sign of the delta -- the same
        // reading an axis LOCK uses. A subscriber asking for wheel-up must not be handed wheel-down.
        let mock = MockBox::new();
        let dev = Device::with_mock(mock.clone());
        let up = dev
            .catch_events([CatchFilter::addr(CatchClass::Axis, Axis::Wheel.as_u16())
                .direction(LockDirection::Positive)])
            .unwrap();
        mock.push_motion(0, 1_000, 0, 0, -1);
        assert!(up.try_recv().is_none());
        mock.push_motion(1, 2_000, 0, 0, 3);
        assert!(matches!(up.recv().unwrap(), CatchEvent::Motion(m) if m.dz == 3));
    }

    #[test]
    fn a_release_to_nothing_reaches_the_class_that_released() {
        // The empty snapshot is the release of the last held usage, and it lists nothing. It has to
        // reach the subscriber for its own class and nobody else's -- it used to go to everyone
        // subscribed to anything, so a vendor-bulk trace received keyboard events.
        let mock = MockBox::new();
        let dev = Device::with_mock(mock.clone());
        let keys = dev
            .catch_events([CatchFilter::class(CatchClass::Key)])
            .unwrap();
        let bulk = dev
            .catch_events([CatchFilter::addr(CatchClass::VendorBulk, 0x83)])
            .unwrap();
        mock.push_usages(0, 1_000, Class::Key, &[]);
        match keys.recv().unwrap() {
            CatchEvent::Usages(u) => assert!(u.usages.is_empty() && u.class == Class::Key),
            other => panic!("expected the empty key snapshot, got {other:?}"),
        }
        assert!(
            bulk.try_recv().is_none(),
            "an empty snapshot is not everyone's"
        );
    }

    #[test]
    fn each_subscriber_gets_only_what_it_asked_for() {
        // The box holds ONE table -- the union of every subscription -- so without per-subscriber
        // matching a caller's stream would change shape whenever unrelated code elsewhere in the
        // process subscribed to something else.
        let mock = MockBox::new();
        let dev = Device::with_mock(mock.clone());
        let bulk = dev
            .catch_events([CatchFilter::addr(CatchClass::VendorBulk, 0x83)])
            .unwrap();
        let bus = dev
            .catch_events([CatchFilter::class(CatchClass::Bus)])
            .unwrap();

        mock.push_traffic(
            0,
            1,
            ClockDomain::DeviceChip,
            CatchClass::VendorBulk,
            0x83,
            LockDirection::Positive,
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
            LockDirection::Both,
            0,
            2,
            &[0, 0],
        );

        match bulk.recv().unwrap() {
            CatchEvent::Traffic(t) => assert_eq!(t.class, CatchClass::VendorBulk),
            other => panic!("expected vendor bulk, got {other:?}"),
        }
        assert!(
            bulk.try_recv().is_none(),
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
        let s = dev
            .catch_events([CatchFilter::addr(CatchClass::VendorBulk, 0x83)])
            .unwrap();
        mock.push_traffic(
            0,
            1,
            ClockDomain::DeviceChip,
            CatchClass::VendorBulk,
            0x84,
            LockDirection::Positive,
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
            LockDirection::Positive,
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
        let s = dev.catch_events([CatchFilter::all()]).unwrap();
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
        let s = dev.catch_events([CatchFilter::all()]).unwrap();
        mock.push_motion(0, u32::MAX - 500, 1, 0, 0);
        mock.push_motion(1, 500, 1, 0, 0);
        let first = s.recv().unwrap().ts_us();
        let second = s.recv().unwrap().ts_us();
        assert_eq!(first, u32::MAX - 500);
        assert_eq!(second, 500); // a wrap is the consumer's to notice, not ours to smooth over
    }
}
