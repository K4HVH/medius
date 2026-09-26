//! `REWRITE` (§3.14): payload bytes, action/class vocabulary, `RESP(REWRITE)` / `RESP(REWRITE_ENTRY)`
//! decode, and the MockBox round-trip through the whole table lifecycle.

use crate::protocol::command::rewrite_payload;
use crate::protocol::{Resp, parse_resp};
use crate::types::rewrite::rewrite_entry_from_payload;
use crate::types::{Direction, RewriteAction, RewriteClass, RewriteRule};

#[test]
fn rewrite_payload_bytes() {
    // Emit(9), endpoint 1 IN, state add, Replace, off 0, match [01,00] / mask [FF,00], payload [AA,BB].
    let p = rewrite_payload(
        9,
        0x0001,
        1,
        1,
        3,
        0,
        &[0x01, 0x00],
        &[0xFF, 0x00],
        &[0xAA, 0xBB],
    );
    assert_eq!(
        p,
        vec![
            0x09, 0x01, 0x00, 0x01, 0x01, 0x03, 0x00, 0x00, 0x02, 0x01, 0x00, 0xFF, 0x00, 0xAA,
            0xBB
        ]
    );
}

#[test]
fn rewrite_payload_no_match_no_payload() {
    // A bare DROP on HidIn(4) interface 0, Both: mlen 0, no match/mask/payload.
    let p = rewrite_payload(4, 0, 0, 1, 1, 0, &[], &[], &[]);
    assert_eq!(
        p,
        vec![0x04, 0x00, 0x00, 0x00, 0x01, 0x01, 0x00, 0x00, 0x00]
    );
}

#[test]
fn rewrite_class_and_action_wire() {
    assert_eq!(RewriteClass::HidIn.as_u8(), 4);
    assert_eq!(RewriteClass::Emit.as_u8(), 9);
    assert_eq!(RewriteClass::Any.as_u8(), 0xFF);
    assert_eq!(RewriteClass::from_u8(8), Some(RewriteClass::Control));
    assert_eq!(RewriteClass::from_u8(0), None); // a parsed-input class is not rewritable
    assert_eq!(RewriteClass::from_u8(10), None); // the bus class is not rewritable
    assert_eq!(RewriteClass::from_u8(11), None); // nor are a clip's transfers

    assert_eq!(RewriteAction::ReplyReplace.as_u8(), 8);
    assert_eq!(RewriteAction::from_u8(2), Some(RewriteAction::Patch));
    assert_eq!(RewriteAction::from_u8(8), Some(RewriteAction::ReplyReplace));
    assert_eq!(RewriteAction::from_u8(9), None); // nine actions, 0 to 8
}

#[test]
fn rewrite_action_admissibility_matches_firmware() {
    // DROP is a report surface only; ANSWER/STALL/NAK/REPLY_* are control-only.
    assert!(RewriteAction::Drop.is_valid_for(RewriteClass::Emit));
    assert!(!RewriteAction::Drop.is_valid_for(RewriteClass::Control));
    assert!(!RewriteAction::Drop.is_valid_for(RewriteClass::Any));
    assert!(RewriteAction::Answer.is_valid_for(RewriteClass::Control));
    assert!(!RewriteAction::Answer.is_valid_for(RewriteClass::Emit));
    assert!(RewriteAction::Patch.is_valid_for(RewriteClass::HidIn));
    assert!(RewriteAction::ReplyPatch.is_valid_for(RewriteClass::Control));
    assert!(!RewriteAction::ReplyPatch.is_valid_for(RewriteClass::HidIn));
}

#[test]
fn resp_rewrite_decode() {
    // [12][flags 0][gen 5][n 1] then one entry: Emit ep 1 IN Replace mlen 2 off 0 plen 2 hits 7.
    let p = [
        12, 0, 5, 1, 0x09, 0x01, 0x00, 0x01, 0x03, 0x02, 0x00, 0x00, 0x02, 0x00, 0x07, 0x00,
    ];
    let Some(Resp::Rewrite(t)) = parse_resp(&p) else {
        panic!("not a RESP(REWRITE)");
    };
    assert!(!t.table_full);
    assert_eq!(t.generation, 5);
    assert_eq!(t.entries.len(), 1);
    let e = t.entries[0];
    assert_eq!(e.class, RewriteClass::Emit);
    assert_eq!(e.id, 1);
    assert_eq!(e.direction, Direction::IN);
    assert_eq!(e.action, RewriteAction::Replace);
    assert_eq!((e.match_len, e.offset, e.payload_len, e.hits), (2, 0, 2, 7));
}

#[test]
fn resp_rewrite_table_full_flag() {
    let p = [12, 0x01, 3, 0];
    let Some(Resp::Rewrite(t)) = parse_resp(&p) else {
        panic!("not a RESP(REWRITE)");
    };
    assert!(t.table_full);
    assert_eq!(t.generation, 3);
    assert!(t.entries.is_empty());
}

#[test]
fn resp_rewrite_high_bytes_decode() {
    // Hand-computed [12][flags 0][gen 0][n 1] then one entry with nonzero high bytes in offset
    // (0x0102 = 258) and hits (0xFFFF = 65535), payload_len 3.
    let p = [
        12, 0x00, 0x00, 1, // what, flags, gen, n
        0x09, 0x01, 0x00, 0x01, 0x03, 0x04, 0x02, 0x01, 0x03, 0x00, 0xFF, 0xFF,
    ];
    let Some(Resp::Rewrite(t)) = parse_resp(&p) else {
        panic!("not a RESP(REWRITE)");
    };
    assert_eq!(t.entries.len(), 1);
    let e = t.entries[0];
    assert_eq!(e.class, RewriteClass::Emit);
    assert_eq!(e.id, 1);
    assert_eq!(e.direction, Direction::IN);
    assert_eq!(e.action, RewriteAction::Replace);
    assert_eq!(e.match_len, 4);
    assert_eq!(e.offset, 258);
    assert_eq!(e.payload_len, 3);
    assert_eq!(e.hits, 65535);
}

#[test]
fn resp_rewrite_entry_replays_as_a_set() {
    // [13][index 0] then the rule in the REWRITE command shape (state hardcoded 1).
    let p = [
        13, 0, 0x09, 0x01, 0x00, 0x01, 0x01, 0x03, 0x00, 0x00, 0x02, 0x01, 0x00, 0xFF, 0x00, 0xAA,
        0xBB,
    ];
    let rule = rewrite_entry_from_payload(&p).expect("decodes");
    assert_eq!(rule.class, RewriteClass::Emit);
    assert_eq!(rule.id, 1);
    assert_eq!(rule.direction, Direction::IN);
    assert_eq!(rule.action, RewriteAction::Replace);
    assert_eq!(rule.offset, 0);
    assert_eq!(rule.match_bytes, vec![0x01, 0x00]);
    assert_eq!(rule.mask, vec![0xFF, 0x00]);
    assert_eq!(rule.payload, vec![0xAA, 0xBB]);

    // The decoded rule re-encodes to the same body the command carries (minus the two-byte prefix).
    let body = rewrite_payload(
        rule.class.as_u8(),
        rule.id,
        rule.direction.as_u8(),
        1,
        rule.action.as_u8(),
        rule.offset,
        &rule.match_bytes,
        &rule.mask,
        &rule.payload,
    );
    assert_eq!(body, &p[2..]);
}

#[test]
fn resp_rewrite_entry_high_offset_and_long_payload() {
    // A control ReplyPatch at offset 0x0102 (258) with a 300-byte payload: the offset survives as
    // a u16 (a u8 read gives 2) and the payload decodes past 256 bytes.
    let mut p = vec![
        13, 0, // what, index
        0x08, 0x00, 0x00, 0x00, 0x01, 0x07, 0x02, 0x01,
        0x02, // Control, id 0, Both, state 1, ReplyPatch, off 258, mlen 2
        0x80, 0x06, // match
        0xFF, 0xFF, // mask
    ];
    p.resize(p.len() + 300, 0xAB); // 300-byte payload
    let rule = rewrite_entry_from_payload(&p).expect("decodes");
    assert_eq!(rule.class, RewriteClass::Control);
    assert_eq!(rule.action, RewriteAction::ReplyPatch);
    assert_eq!(rule.offset, 258);
    assert_eq!(rule.match_bytes, vec![0x80, 0x06]);
    assert_eq!(rule.mask, vec![0xFF, 0xFF]);
    assert_eq!(rule.payload.len(), 300);
    assert!(rule.payload.iter().all(|&b| b == 0xAB));

    // Re-encodes to the same body the command carries (minus the two-byte prefix), like its sibling.
    let body = rewrite_payload(
        rule.class.as_u8(),
        rule.id,
        rule.direction.as_u8(),
        1,
        rule.action.as_u8(),
        rule.offset,
        &rule.match_bytes,
        &rule.mask,
        &rule.payload,
    );
    assert_eq!(body, &p[2..]);
}

#[test]
fn rewrite_rule_builders() {
    let r = RewriteRule::new(
        RewriteClass::HidIn,
        0,
        Direction::Both,
        RewriteAction::Patch,
    )
    .at_offset(2)
    .matching(vec![0x00], vec![0xFF])
    .with_payload(vec![0x42]);
    assert_eq!(r.offset, 2);
    assert_eq!(r.match_bytes, vec![0x00]);
    assert_eq!(r.mask, vec![0xFF]);
    assert_eq!(r.payload, vec![0x42]);
    assert_eq!(r.key().class, RewriteClass::HidIn);
}

#[cfg(feature = "mock")]
mod mock_roundtrip {
    use crate::error::Error;
    use crate::types::{Direction, ImperfectStatus, RewriteAction, RewriteClass, RewriteRule};
    use crate::{Device, MockBox};

    fn allowed_mock() -> MockBox {
        MockBox::new().with_imperfect(true)
    }

    #[test]
    fn set_rewrite_requires_the_opt_in() {
        let device = Device::with_mock(MockBox::new()); // opt-in off by default
        let rule = RewriteRule::new(RewriteClass::Emit, 1, Direction::IN, RewriteAction::Drop);
        assert!(matches!(
            device.set_rewrite(&rule),
            Err(Error::ImperfectRequired)
        ));
    }

    #[test]
    fn set_query_and_clear_roundtrip() {
        let mock = allowed_mock();
        let device = Device::with_mock(mock.clone());
        let rule = RewriteRule::new(RewriteClass::Emit, 1, Direction::IN, RewriteAction::Drop);
        device.set_rewrite(&rule).unwrap();

        let table = device.query_rewrite().unwrap();
        assert_eq!(table.entries.len(), 1);
        assert_eq!(table.entries[0].class, RewriteClass::Emit);
        assert_eq!(table.entries[0].action, RewriteAction::Drop);
        assert_eq!(table.generation, 1);
        assert!(device.query_health().unwrap().rewrite_on);

        device.clear_rewrite().unwrap();
        let table = device.query_rewrite().unwrap();
        assert!(table.entries.is_empty());
        // gen stays monotonic across the clear (1 set → 2 clear), never back to a value already seen.
        assert_eq!(table.generation, 2);
        assert!(!device.query_health().unwrap().rewrite_on);
    }

    #[test]
    fn idempotent_reset_does_not_bump_gen() {
        let device = Device::with_mock(allowed_mock());
        let rule = RewriteRule::new(RewriteClass::Emit, 1, Direction::IN, RewriteAction::Replace)
            .with_payload(vec![0xAA, 0xBB]);
        device.set_rewrite(&rule).unwrap();
        device.set_rewrite(&rule).unwrap();
        assert_eq!(device.query_rewrite().unwrap().generation, 1);
    }

    #[test]
    fn remove_rewrite_drops_one_rule() {
        let device = Device::with_mock(allowed_mock());
        let a = RewriteRule::new(RewriteClass::Emit, 1, Direction::IN, RewriteAction::Drop);
        let b = RewriteRule::new(RewriteClass::HidIn, 0, Direction::Both, RewriteAction::Drop);
        device.set_rewrite(&a).unwrap();
        device.set_rewrite(&b).unwrap();
        assert_eq!(device.query_rewrite().unwrap().entries.len(), 2);
        device.remove_rewrite(&a).unwrap();
        let table = device.query_rewrite().unwrap();
        assert_eq!(table.entries.len(), 1);
        assert_eq!(table.entries[0].class, RewriteClass::HidIn);
    }

    #[test]
    fn query_entry_replays_a_stored_rule() {
        let device = Device::with_mock(allowed_mock());
        let rule = RewriteRule::new(
            RewriteClass::Control,
            0,
            Direction::Both,
            RewriteAction::Answer,
        )
        .matching(vec![0x80, 0x06], vec![0xFF, 0xFF])
        .with_payload(vec![0x12, 0x01]);
        device.set_rewrite(&rule).unwrap();
        let read = device.query_rewrite_entry(0).unwrap();
        assert_eq!(read.class, RewriteClass::Control);
        assert_eq!(read.action, RewriteAction::Answer);
        assert_eq!(read.match_bytes, vec![0x80, 0x06]);
        assert_eq!(read.mask, vec![0xFF, 0xFF]);
        assert_eq!(read.payload, vec![0x12, 0x01]);
    }

    #[test]
    fn bad_mask_length_is_rejected_before_the_wire() {
        let mock = allowed_mock();
        let device = Device::with_mock(mock.clone());
        let mut rule = RewriteRule::new(RewriteClass::Emit, 1, Direction::IN, RewriteAction::Patch);
        rule.match_bytes = vec![0x00, 0x00];
        rule.mask = vec![0xFF]; // shorter than match
        assert!(matches!(
            device.set_rewrite(&rule),
            Err(Error::RewriteMaskLength { .. })
        ));
        assert!(!mock.saw(crate::FrameType::Rewrite));
    }

    #[test]
    fn control_only_action_on_report_class_is_rejected() {
        let device = Device::with_mock(allowed_mock());
        let rule = RewriteRule::new(RewriteClass::Emit, 1, Direction::IN, RewriteAction::Stall);
        assert!(matches!(
            device.set_rewrite(&rule),
            Err(Error::RewriteActionClass { .. })
        ));
    }

    #[test]
    fn relative_direction_is_rejected() {
        let device = Device::with_mock(allowed_mock());
        let rule = RewriteRule::new(RewriteClass::Emit, 1, Direction::With, RewriteAction::Drop);
        assert!(matches!(
            device.set_rewrite(&rule),
            Err(Error::RelativeDirection { .. })
        ));
    }

    fn answer(id: u16, len: usize) -> RewriteRule {
        RewriteRule::new(
            RewriteClass::Control,
            id,
            Direction::Both,
            RewriteAction::Answer,
        )
        .with_payload(vec![0x5A; len])
    }

    // The box counts payload bytes only, and an overwrite frees its old payload before the new one is
    // costed. A rule past what is left never reaches the wire.
    #[test]
    fn the_payload_pool_is_checked_before_the_wire() {
        use crate::REWRITE_PAYLOAD_POOL;
        let mock = allowed_mock();
        let device = Device::with_mock(mock.clone());
        for id in 0..4 {
            device.set_rewrite(&answer(id, 500)).unwrap();
        }
        device.set_rewrite(&answer(4, 48)).unwrap();
        mock.clear_recorded();
        assert!(matches!(
            device.set_rewrite(&answer(5, 1)),
            Err(Error::RewritePoolFull { len: 1, free: 0, limit }) if limit == REWRITE_PAYLOAD_POOL
        ));
        assert!(!mock.saw(crate::FrameType::Rewrite));
        // A rule with no payload takes nothing from the pool.
        let stall = RewriteRule::new(
            RewriteClass::Control,
            9,
            Direction::Both,
            RewriteAction::Stall,
        );
        device.set_rewrite(&stall).unwrap();
        // An overwrite of the 48-byte rule has 48 bytes to spend, and one byte more is refused.
        device.set_rewrite(&answer(4, 48)).unwrap();
        assert!(matches!(
            device.set_rewrite(&answer(4, 49)),
            Err(Error::RewritePoolFull {
                len: 49,
                free: 48,
                ..
            })
        ));
        device.remove_rewrite(&answer(0, 0)).unwrap();
        device.set_rewrite(&answer(5, 500)).unwrap();
        let table = device.query_rewrite().unwrap();
        assert_eq!(table.entries.len(), 6);
        assert!(!table.table_full);
    }

    // full means the last rule was refused for room (count or pool); any accepted change resets it,
    // and the keepalive's identical re-sends leave it standing.
    #[test]
    fn table_full_is_the_last_rule_refused_for_room() {
        use crate::protocol::FrameType;
        use crate::protocol::command::rewrite_payload;
        let device = Device::with_mock(allowed_mock());
        let full = || device.query_rewrite().unwrap().table_full;
        let raw = |r: &RewriteRule| {
            rewrite_payload(
                r.class.as_u8(),
                r.id,
                r.direction.as_u8(),
                1,
                r.action.as_u8(),
                r.offset,
                &r.match_bytes,
                &r.mask,
                &r.payload,
            )
        };
        for id in 0..crate::REWRITE_MAX_ENTRIES as u16 {
            device.set_rewrite(&answer(id, 1)).unwrap();
        }
        assert!(!full(), "a table at the count is no refusal");
        // Past the crate's own check, the way a second host would reach the box.
        device
            .link
            .send(FrameType::Rewrite, &raw(&answer(99, 1)))
            .unwrap();
        assert!(full());
        device
            .link
            .send(FrameType::Rewrite, &raw(&answer(0, 1)))
            .unwrap();
        assert!(full(), "an identical re-set is no change");
        device.set_rewrite(&answer(0, 2)).unwrap();
        assert!(!full(), "an overwrite is a change");

        device.clear_rewrite().unwrap();
        for id in 0..4 {
            device.set_rewrite(&answer(id, 500)).unwrap();
        }
        device
            .link
            .send(FrameType::Rewrite, &raw(&answer(4, 49)))
            .unwrap();
        assert!(full(), "2049 bytes");
        assert_eq!(device.query_rewrite().unwrap().entries.len(), 4);
        device.remove_rewrite(&answer(3, 0)).unwrap();
        assert!(!full(), "a removal is a change");
        device
            .link
            .send(FrameType::Rewrite, &raw(&answer(4, 49)))
            .unwrap();
        device
            .link
            .send(FrameType::Rewrite, &raw(&answer(9, 501)))
            .unwrap();
        assert!(full());
        device.clear_rewrite().unwrap();
        assert!(!full());
    }

    #[test]
    fn imperfect_status_scripts_the_gate() {
        // A mock configured over-capacity but opt-in-on still admits the advanced control layer.
        let mock = MockBox::new().with_imperfect_status(ImperfectStatus {
            allowed: true,
            over_capacity: true,
            clone_imperfect: true,
        });
        let device = Device::with_mock(mock);
        let rule = RewriteRule::new(RewriteClass::Emit, 1, Direction::IN, RewriteAction::Drop);
        assert!(device.set_rewrite(&rule).is_ok());
    }

    #[test]
    fn opt_off_drops_held_rewrites() {
        // The box clears its rewrite table when the opt-in goes off (safety_clear); the crate drops
        // its copy, or the keepalive re-asserts the rules when the opt-in returns.
        let device = Device::with_mock(allowed_mock());
        let rule = RewriteRule::new(RewriteClass::Emit, 1, Direction::IN, RewriteAction::Drop);
        device.set_rewrite(&rule).unwrap();
        assert!(!device.link.desired().lock().held_rewrites().is_empty());
        assert!(device.query_health().unwrap().rewrite_on);
        device.allow_imperfect_clones(false).unwrap();
        assert!(
            device.link.desired().lock().held_rewrites().is_empty(),
            "opt-off must clear held rewrites so a re-enable cannot resurrect them"
        );
        // The box's table clears too (usbdev_set_imperfect_allowed): RESP(REWRITE) reads empty and
        // HEALTH's rewrite_on falls.
        assert!(
            device.query_rewrite().unwrap().entries.is_empty(),
            "opt-off must clear the box's rewrite table, not just the held copy"
        );
        assert!(!device.query_health().unwrap().rewrite_on);
    }

    #[test]
    fn oversized_payload_is_rejected_before_the_wire() {
        let device = Device::with_mock(allowed_mock());
        // A 100-byte Replace on Emit exceeds the box's 64-byte head; the crate rejects it before the
        // wire.
        let big = RewriteRule::new(RewriteClass::Emit, 1, Direction::IN, RewriteAction::Replace)
            .with_payload(vec![0u8; 100]);
        assert!(matches!(
            device.set_rewrite(&big),
            Err(Error::RewritePayloadTooLarge { .. })
        ));
        // A control Answer within the 8+2048 control image is admitted.
        let ok = RewriteRule::new(
            RewriteClass::Control,
            0,
            Direction::Both,
            RewriteAction::Answer,
        )
        .with_payload(vec![0u8; 64]);
        assert!(device.set_rewrite(&ok).is_ok());
    }

    #[test]
    fn large_offset_and_payload_survive_the_roundtrip() {
        // A control ReplyPatch at offset >= 0x0100 with a 300-byte payload round-trips through the
        // store, summary and entry readback; a u16->u8 regression anywhere drops it.
        let device = Device::with_mock(allowed_mock());
        let rule = RewriteRule::new(
            RewriteClass::Control,
            0,
            Direction::Both,
            RewriteAction::ReplyPatch,
        )
        .at_offset(258)
        .matching(vec![0x80, 0x06], vec![0xFF, 0xFF])
        .with_payload(vec![0xAB; 300]);
        device.set_rewrite(&rule).unwrap();

        let summary = device.query_rewrite().unwrap();
        assert_eq!(summary.entries.len(), 1);
        assert_eq!(summary.entries[0].offset, 258);
        assert_eq!(summary.entries[0].payload_len, 300);

        let read = device.query_rewrite_entry(0).unwrap();
        assert_eq!(read.offset, 258);
        assert_eq!(read.payload, vec![0xAB; 300]);
    }

    #[test]
    fn match_bytes_are_part_of_the_key() {
        // Same (class, id, direction), different match bytes: two rows, since match and mask are part
        // of the key.
        let device = Device::with_mock(allowed_mock());
        let a = RewriteRule::new(RewriteClass::Emit, 1, Direction::IN, RewriteAction::Drop)
            .matching(vec![0x01], vec![0xFF]);
        let b = RewriteRule::new(RewriteClass::Emit, 1, Direction::IN, RewriteAction::Drop)
            .matching(vec![0x02], vec![0xFF]);
        device.set_rewrite(&a).unwrap();
        device.set_rewrite(&b).unwrap();
        assert_eq!(
            device.query_rewrite().unwrap().entries.len(),
            2,
            "differing match bytes make two rows, not one overwrite"
        );
    }

    #[test]
    fn summary_is_in_insertion_order() {
        // RESP(REWRITE) lists installation order (usbdev_pack_rewrite), not the most-specific-first
        // selection order.
        let device = Device::with_mock(allowed_mock());
        device
            .set_rewrite(&RewriteRule::new(
                RewriteClass::Emit,
                1,
                Direction::IN,
                RewriteAction::Drop,
            ))
            .unwrap();
        device
            .set_rewrite(&RewriteRule::new(
                RewriteClass::HidIn,
                0,
                Direction::Both,
                RewriteAction::Drop,
            ))
            .unwrap();
        device
            .set_rewrite(&RewriteRule::new(
                RewriteClass::HidOut,
                2,
                Direction::OUT,
                RewriteAction::Drop,
            ))
            .unwrap();
        let classes: Vec<_> = device
            .query_rewrite()
            .unwrap()
            .entries
            .iter()
            .map(|e| e.class)
            .collect();
        assert_eq!(
            classes,
            vec![
                RewriteClass::Emit,
                RewriteClass::HidIn,
                RewriteClass::HidOut
            ]
        );
    }

    #[test]
    fn reply_actions_survive_the_roundtrip() {
        // ReplyPatch/ReplyReplace (control-only, one with an offset) store and read back intact.
        let device = Device::with_mock(allowed_mock());
        let rp = RewriteRule::new(
            RewriteClass::Control,
            0,
            Direction::Both,
            RewriteAction::ReplyPatch,
        )
        .at_offset(4)
        .matching(vec![0x80, 0x06], vec![0xFF, 0xFF])
        .with_payload(vec![0x12, 0x34]);
        let rr = RewriteRule::new(
            RewriteClass::Control,
            0,
            Direction::Both,
            RewriteAction::ReplyReplace,
        )
        .with_payload(vec![0x12, 0x01, 0x10]);
        device.set_rewrite(&rp).unwrap();
        device.set_rewrite(&rr).unwrap();

        let read0 = device.query_rewrite_entry(0).unwrap();
        assert_eq!(read0.action, RewriteAction::ReplyPatch);
        assert_eq!(read0.offset, 4);
        assert_eq!(read0.match_bytes, vec![0x80, 0x06]);
        assert_eq!(read0.payload, vec![0x12, 0x34]);

        let read1 = device.query_rewrite_entry(1).unwrap();
        assert_eq!(read1.action, RewriteAction::ReplyReplace);
        assert_eq!(read1.payload, vec![0x12, 0x01, 0x10]);
    }
}

#[test]
fn health_u16_high_bits_decode() {
    use crate::types::Health;
    // HEALTH is a u16 LE since proto 7: link_up (b0) plus rewrite_on (b8), patch_on (b9), transform_on (b10).
    let h = Health::from_flags(0x0701);
    assert!(h.link_up && !h.mouse_attached);
    assert!(h.rewrite_on && h.patch_on && h.transform_on);
    assert_eq!(h.to_flags(), 0x0701);
    // A box that answers only the low byte still decodes bits 0-7, high byte clear.
    let lo = Health::from_flags(0x80);
    assert!(lo.kbd_attached && !lo.rewrite_on && !lo.patch_on && !lo.transform_on);
}

#[test]
fn resp_health_u16_roundtrips_through_parse() {
    use crate::protocol::{Resp, parse_resp};
    // [what=1][flags u16 LE = 0x0300] -> rewrite_on + patch_on.
    let Some(Resp::Health(h)) = parse_resp(&[1, 0x00, 0x03]) else {
        panic!("not a RESP(HEALTH)");
    };
    assert!(h.rewrite_on && h.patch_on && !h.transform_on);
}

// The box refuses a match past its compare length and a rule its read-back reply cannot carry.
mod box_limits {
    use crate::device::rewrite::validate_rule;
    use crate::error::Error;
    use crate::types::{Direction, RewriteAction, RewriteClass, RewriteRule};

    const M: [u8; 2] = [0x07, 0x20];
    const K: [u8; 2] = [0xFF, 0x20];

    fn patch() -> RewriteRule {
        RewriteRule::new(RewriteClass::HidIn, 2, Direction::IN, RewriteAction::Patch)
            .matching(M, K)
            .with_payload([0xAA])
    }

    // The box refuses a rule its read-back reply cannot carry, and that reply's header is two bytes
    // wider than the command's.
    #[test]
    fn a_rule_the_box_cannot_read_back_is_refused() {
        let answer = |n: usize| {
            RewriteRule::new(
                RewriteClass::Control,
                0,
                Direction::IN,
                RewriteAction::Answer,
            )
            .with_payload(vec![0u8; n])
        };
        assert!(validate_rule(&answer(501)).is_ok());
        assert!(matches!(
            validate_rule(&answer(502)),
            Err(Error::RewritePayloadTooLarge {
                len: 502,
                cap: 501,
                ..
            })
        ));
        assert!(matches!(
            validate_rule(&answer(498).matching([1, 2], [0xFF, 0xFF])),
            Err(Error::RewritePayloadTooLarge {
                len: 498,
                cap: 497,
                ..
            })
        ));
    }

    #[test]
    fn a_match_past_the_box_cap_is_refused() {
        let fits = patch().matching([0x07; 16], [0xFF; 16]);
        assert!(validate_rule(&fits).is_ok());
        let over = patch().matching([0x07; 17], [0xFF; 17]);
        assert!(matches!(
            validate_rule(&over),
            Err(Error::RewriteMatchTooLong { len: 17, limit: 16 })
        ));
    }

    #[cfg(feature = "mock")]
    #[test]
    fn the_mock_refuses_what_the_box_does() {
        use crate::protocol::FrameType;
        use crate::protocol::command::rewrite_payload;
        use crate::{Device, MockBox};

        let mock = MockBox::new().with_imperfect(true);
        let device = Device::with_mock(mock.clone());
        let rule = patch();
        device.set_rewrite(&rule).unwrap();

        // Past the crate's own check, straight onto the link, where the mock refuses what the box does.
        let gen_before = device.query_rewrite().unwrap().generation;
        for bad in [
            rewrite_payload(4, 3, 1, 1, 9, 0, &M, &K, &[0, 0, 0]), // nine actions, 0 to 8
            rewrite_payload(4, 3, 1, 1, 10, 0, &M, &K, &[]),
            rewrite_payload(11, 0, 1, 1, 0, 0, &[], &[], &[]), // a class that is never rewritten
            rewrite_payload(4, 3, 3, 1, 0, 0, &M, &K, &[]),    // a relative direction
            rewrite_payload(8, 0, 1, 1, 4, 0, &[], &[], &[0u8; 502]), // past its own read-back
            rewrite_payload(4, 3, 1, 1, 2, 60, &M, &K, &[0u8; 5]), // a patch past the report head
            rewrite_payload(4, 2, 1, 1, 2, 60, &M, &K, &[0u8; 5]), // onto the held key: it stays
        ] {
            device.link.send(FrameType::Rewrite, &bad).unwrap();
        }
        let table = device.query_rewrite().unwrap();
        assert_eq!(table.entries.len(), 1);
        assert_eq!(table.generation, gen_before);
        assert_eq!(device.query_rewrite_entry(0).unwrap(), rule);
    }
}
