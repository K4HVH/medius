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
    assert_eq!(RewriteAction::Clip.as_u8(), 9);
    assert_eq!(RewriteAction::from_u8(9), Some(RewriteAction::Clip));
    assert_eq!(RewriteAction::from_u8(10), None);
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
    // Hand-computed [12][flags 0][gen 0][n 1] then one entry carrying a nonzero high byte in offset
    // (0x0102 = 258), payload_len (3) and hits (0xFFFF = 65535). A u8 truncation of any field, or a
    // transpose of offset/payload_len, fails here where every prior test kept those bytes zero.
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
    // A control ReplyPatch at offset 0x0102 (258) with a 300-byte payload: the offset must survive as a
    // u16 (a u8 read gives 2) and the whole payload must decode past the 256-byte boundary.
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
        // The box clears its rewrite table when the opt-in goes off (firmware safety_clear); the crate
        // drops the held copy to match, or the keepalive re-asserts the rules when the opt-in returns.
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
        // The box clears its own table too (usbdev_set_imperfect_allowed): RESP(REWRITE) reads empty and
        // HEALTH's rewrite_on falls, so a re-enable's keepalive has nothing to resurrect from the box side.
        assert!(
            device.query_rewrite().unwrap().entries.is_empty(),
            "opt-off must clear the box's rewrite table, not just the held copy"
        );
        assert!(!device.query_health().unwrap().rewrite_on);
    }

    #[test]
    fn oversized_payload_is_rejected_before_the_wire() {
        let device = Device::with_mock(allowed_mock());
        // A Replace on a report surface (Emit) with a 100-byte payload: the box holds 64, so it
        // refuses; the crate rejects it before the wire rather than hold a rule the box drops.
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
        // A control ReplyPatch with offset >= 0x0100 and a 300-byte payload must round-trip through the
        // mock's store, the summary and the entry readback intact: a u16->u8 regression anywhere drops it.
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
        // Two rules with the same (class, id, direction) but different match bytes are two table rows,
        // not an overwrite: match and mask are part of the key.
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
        // RESP(REWRITE) lists the table in installation order (usbdev_pack_rewrite), not the
        // most-specific-first order the box uses to select a match.
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
        // Only the admissibility of ReplyPatch/ReplyReplace is unit-tested elsewhere; prove they also
        // store and read back through the mock intact (control-only actions, one carrying an offset).
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
    // proto 7 HEALTH is a u16 LE: link_up (b0) plus rewrite_on (b8), patch_on (b9), transform_on (b10).
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

// RW_CLIP (§3.14): the rule that runs a clip verb. Its payload is [op][flags][slen].
mod clip_rule {
    use crate::device::rewrite::validate_rule;
    use crate::error::Error;
    use crate::protocol::command::rewrite_payload;
    use crate::types::{ClipAction, ClipVerb, Direction, RewriteAction, RewriteClass, RewriteRule};

    const M: [u8; 2] = [0x07, 0x20];
    const K: [u8; 2] = [0xFF, 0x20];

    fn held() -> RewriteRule {
        RewriteRule::clip(RewriteClass::HidIn, 2, Direction::IN, ClipAction::Start).matching(M, K)
    }

    #[test]
    fn the_builders_write_the_payload_the_box_reads() {
        assert_eq!(held().payload, [0, 0, 0]);
        assert_eq!(held().action, RewriteAction::Clip);
        assert_eq!(held().dropping().payload, [0, 0x01, 0]);
        assert_eq!(held().on_edge(1).payload, [0, 0x02, 1]);
        let both = RewriteRule::clip(RewriteClass::Emit, 1, Direction::IN, ClipAction::Toggle)
            .matching(M, K)
            .dropping()
            .on_edge(1);
        assert_eq!(both.payload, [5, 0x03, 1]);
        assert_eq!(
            both.clip_verb(),
            Some(ClipVerb {
                action: ClipAction::Toggle,
                drop: true,
                edge: true,
                selector_len: 1
            })
        );
        // The wire is the ordinary REWRITE frame: the verb rides where a PATCH's bytes would.
        assert_eq!(
            rewrite_payload(
                9,
                1,
                1,
                1,
                9,
                0,
                &both.match_bytes,
                &both.mask,
                &both.payload
            ),
            [
                9, 0x01, 0x00, 1, 1, 9, 0x00, 0x00, 2, 0x07, 0x20, 0xFF, 0x20, 5, 0x03, 1
            ]
        );
    }

    #[test]
    fn the_flag_builders_leave_any_other_rule_alone() {
        let patch = RewriteRule::new(RewriteClass::Emit, 1, Direction::IN, RewriteAction::Patch)
            .with_payload([0xAA, 0xBB, 0xCC]);
        assert_eq!(patch.clone().dropping().on_edge(2), patch);
        assert_eq!(patch.clip_verb(), None);
        // A clip action with anything but its three bytes decodes to nothing.
        let bare = RewriteRule::new(RewriteClass::Emit, 1, Direction::IN, RewriteAction::Clip);
        assert_eq!(bare.clip_verb(), None);
        assert_eq!(bare.clone().dropping().on_edge(1), bare);
        let bad_op = bare.with_payload([6, 0, 0]);
        assert_eq!(bad_op.clip_verb(), None);
    }

    fn refused(rule: RewriteRule) -> bool {
        matches!(validate_rule(&rule), Err(Error::RewriteClipRule { .. }))
    }

    #[test]
    fn each_refusal_names_its_own_fault() {
        let reason = |rule: RewriteRule| match validate_rule(&rule) {
            Err(Error::RewriteClipRule { reason }) => reason,
            other => panic!("not a clip rule refusal: {other:?}"),
        };
        assert!(reason(held().with_payload([0, 0])).contains("three payload bytes"));
        assert!(reason(held().with_payload([6, 0, 0])).contains("clip verb from 0"));
        assert!(reason(held().with_payload([0, 0x04, 0])).contains("drop and edge flags"));
        assert!(reason(held().at_offset(1)).contains("offset 0"));
        assert!(reason(held().with_payload([0, 0, 1])).contains("only with the edge flag"));
        assert!(reason(held().on_edge(2)).contains("past its selector"));
        let control = RewriteRule::clip(RewriteClass::Control, 0, Direction::IN, ClipAction::Start);
        assert!(reason(control.clone().dropping()).contains("cannot drop"));
        assert!(reason(control.matching(M, K).on_edge(1)).contains("one stream"));
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
        let fits = held().matching([0x07; 16], [0xFF; 16]);
        assert!(validate_rule(&fits).is_ok());
        let over = held().matching([0x07; 17], [0xFF; 17]);
        assert!(matches!(
            validate_rule(&over),
            Err(Error::RewriteMatchTooLong { len: 17, limit: 16 })
        ));
    }

    #[test]
    fn what_the_box_refuses_is_refused_before_the_wire() {
        assert!(validate_rule(&held()).is_ok());
        assert!(validate_rule(&held().dropping()).is_ok());
        assert!(validate_rule(&held().on_edge(1)).is_ok());
        assert!(validate_rule(&held().on_edge(0)).is_ok());
        let on = |class, id, direction| {
            RewriteRule::clip(class, id, direction, ClipAction::Start).matching(M, K)
        };
        // Every packet, from anywhere: legal with no flags.
        assert!(validate_rule(&on(RewriteClass::Control, 0, Direction::IN)).is_ok());
        assert!(validate_rule(&on(RewriteClass::Any, 0xFFFF, Direction::Both)).is_ok());
        // DROP where Drop is not an action.
        assert!(refused(
            on(RewriteClass::Control, 0, Direction::IN).dropping()
        ));
        assert!(refused(
            on(RewriteClass::Any, 0xFFFF, Direction::Both).dropping()
        ));
        // EDGE with no single stream to have a run over.
        assert!(refused(
            on(RewriteClass::Control, 0, Direction::IN).on_edge(1)
        ));
        assert!(refused(on(RewriteClass::Any, 2, Direction::IN).on_edge(1)));
        assert!(refused(
            on(RewriteClass::HidIn, 0xFFFF, Direction::IN).on_edge(1)
        ));
        assert!(refused(
            on(RewriteClass::HidIn, 2, Direction::Both).on_edge(1)
        ));
        // EDGE with nothing past the selector to stop matching on.
        assert!(refused(held().on_edge(2)));
        assert!(refused(held().on_edge(3)));
        assert!(refused(
            RewriteRule::clip(RewriteClass::HidIn, 2, Direction::IN, ClipAction::Start).on_edge(0)
        ));
        // A payload the builders did not write.
        assert!(refused(held().with_payload([0, 0])));
        assert!(refused(held().with_payload([6, 0, 0])));
        assert!(refused(held().with_payload([0, 0x04, 0])));
        assert!(refused(held().with_payload([0, 0, 1])));
        assert!(refused(held().at_offset(1)));
    }

    #[cfg(feature = "mock")]
    #[test]
    fn a_clip_rule_round_trips_and_the_mock_refuses_what_the_box_does() {
        use crate::protocol::FrameType;
        use crate::{Device, MockBox};

        let mock = MockBox::new().with_imperfect(true);
        let device = Device::with_mock(mock.clone());
        let rule = held().on_edge(1).dropping();
        device.set_rewrite(&rule).unwrap();
        let read = device.query_rewrite_entry(0).unwrap();
        assert_eq!(read, rule);
        assert_eq!(read.clip_verb().unwrap().action, ClipAction::Start);

        // Past the crate's own check, straight onto the link, where the mock refuses what the box does.
        let gen_before = device.query_rewrite().unwrap().generation;
        for bad in [
            rewrite_payload(8, 0, 1, 1, 9, 0, &[], &[], &[0, 0x01, 0]), // DROP on CONTROL
            rewrite_payload(4, 2, 0, 1, 9, 0, &M, &K, &[0, 0x02, 1]),   // EDGE with dir Both
            rewrite_payload(4, 3, 1, 1, 9, 0, &M, &K, &[0, 0x02, 2]), // the selector covers the whole match
            rewrite_payload(4, 3, 1, 1, 9, 0, &M, &K, &[6, 0, 0]),    // not a verb
            rewrite_payload(4, 3, 1, 1, 10, 0, &M, &K, &[]),          // not an action
            rewrite_payload(4, 3, 1, 1, 9, 1, &M, &K, &[0, 0, 0]),    // an offset
            rewrite_payload(4, 0xFFFF, 1, 1, 9, 0, &M, &K, &[0, 0x02, 1]), // EDGE on every id
            rewrite_payload(4, 3, 1, 1, 9, 0, &M, &K, &[0, 0, 1]),    // a selector without EDGE
            rewrite_payload(4, 3, 1, 1, 9, 0, &M, &K, &[0, 0x04, 0]), // a flag the box does not know
            rewrite_payload(11, 0, 1, 1, 9, 0, &[], &[], &[0, 0, 0]), // a class that is never rewritten
            rewrite_payload(4, 3, 3, 1, 0, 0, &M, &K, &[]),           // a relative direction
            rewrite_payload(8, 0, 1, 1, 4, 0, &[], &[], &[0u8; 502]), // past its own read-back
            rewrite_payload(4, 3, 1, 1, 2, 60, &M, &K, &[0u8; 5]), // a patch past the report head
            rewrite_payload(4, 2, 1, 1, 9, 0, &M, &K, &[6, 0, 0]), // onto the held key: it stays
        ] {
            device.link.send(FrameType::Rewrite, &bad).unwrap();
        }
        let table = device.query_rewrite().unwrap();
        assert_eq!(table.entries.len(), 1);
        assert_eq!(table.generation, gen_before);
        assert_eq!(device.query_rewrite_entry(0).unwrap(), rule);
    }
}
