//! `PATCH` (§3.14): payload bytes, section vocabulary, `RESP(PATCHES)` / `RESP(PATCH_ENTRY)` decode,
//! and the MockBox round-trip through the store lifecycle.

use crate::protocol::command::{patch_apply_payload, patch_clear_payload, patch_payload};
use crate::protocol::{Resp, parse_resp};
use crate::types::PatchSection;
use crate::types::patch::patch_entry_from_payload;

#[test]
fn patch_payload_bytes() {
    // Device section, offset 8 (idVendor), two little-endian bytes.
    let p = patch_payload(0, 0, 0, 8, &[0x34, 0x12]);
    assert_eq!(p, vec![0x00, 0x00, 0x00, 0x08, 0x00, 0x34, 0x12]);
}

#[test]
fn patch_payload_report_section_carries_cfg_and_index() {
    // Report section on interface 2 in config 1 at offset 4.
    let p = patch_payload(2, 1, 2, 4, &[0x00]);
    assert_eq!(p, vec![0x02, 0x01, 0x02, 0x04, 0x00, 0x00]);
}

#[test]
fn patch_apply_clear_bytes() {
    assert_eq!(patch_apply_payload(), [0xFE]);
    assert_eq!(patch_clear_payload(), [0xFF]);
}

#[test]
fn patch_section_wire() {
    assert_eq!(PatchSection::Device.as_u8(), 0);
    assert_eq!(PatchSection::Bos.as_u8(), 4);
    assert_eq!(PatchSection::from_u8(3), Some(PatchSection::String));
    assert_eq!(PatchSection::from_u8(0xFE), None); // APPLY is not a section
    assert_eq!(PatchSection::from_u8(5), None);
}

#[test]
fn resp_patches_decode() {
    // [14][flags applied|pending][n 1] then Device/cfg 0/index 0/off 8/len 2.
    let p = [14, 0x03, 1, 0x00, 0x00, 0x00, 0x08, 0x00, 0x02, 0x00];
    let Some(Resp::Patches(s)) = parse_resp(&p) else {
        panic!("not a RESP(PATCHES)");
    };
    assert!(s.applied && s.pending);
    assert!(!s.refused && !s.table_full);
    assert_eq!(s.entries.len(), 1);
    let e = s.entries[0];
    assert_eq!(e.section, PatchSection::Device);
    assert_eq!((e.cfg, e.index, e.offset, e.len), (0, 0, 8, 2));
}

#[test]
fn resp_patches_all_flags() {
    let p = [14, 0x0F, 0];
    let Some(Resp::Patches(s)) = parse_resp(&p) else {
        panic!("not a RESP(PATCHES)");
    };
    assert!(s.applied && s.pending && s.refused && s.table_full);
    assert!(s.entries.is_empty());
}

#[test]
fn resp_patches_high_bytes_decode() {
    // Hand-computed: [14][flags 0][n 1] then Report/cfg 1/index 5 at offset 0x0102 (258), len
    // 0x0100 (256). A u8 read of either changes it; a transpose swaps 258 and 256.
    let p = [14, 0x00, 1, 0x02, 0x01, 0x05, 0x02, 0x01, 0x00, 0x01];
    let Some(Resp::Patches(s)) = parse_resp(&p) else {
        panic!("not a RESP(PATCHES)");
    };
    assert_eq!(s.entries.len(), 1);
    let e = s.entries[0];
    assert_eq!(e.section, PatchSection::Report);
    assert_eq!((e.cfg, e.index), (1, 5));
    assert_eq!(e.offset, 258);
    assert_eq!(e.len, 256);
}

#[test]
fn resp_patches_refused_flag_alone() {
    // flags 0x04 is refused only: a refused<->table_full swap would misread it as full.
    let p = [14, 0x04, 0];
    let Some(Resp::Patches(s)) = parse_resp(&p) else {
        panic!("not a RESP(PATCHES)");
    };
    assert!(s.refused);
    assert!(!s.applied && !s.pending && !s.table_full);
}

#[test]
fn resp_patches_table_full_flag_alone() {
    // flags 0x08 is table_full only: the mirror of the refused-only case.
    let p = [14, 0x08, 0];
    let Some(Resp::Patches(s)) = parse_resp(&p) else {
        panic!("not a RESP(PATCHES)");
    };
    assert!(s.table_full);
    assert!(!s.applied && !s.pending && !s.refused);
}

#[test]
fn resp_patch_entry_replays_as_a_set() {
    // [15][index 0] then [section][cfg][index][offset u16][bytes].
    let p = [15, 0, 0x00, 0x00, 0x00, 0x08, 0x00, 0x34, 0x12];
    let patch = patch_entry_from_payload(&p).expect("decodes");
    assert_eq!(patch.section, PatchSection::Device);
    assert_eq!((patch.cfg, patch.index, patch.offset), (0, 0, 8));
    assert_eq!(patch.bytes, vec![0x34, 0x12]);

    // The decoded patch re-encodes to the command body (minus the two-byte prefix).
    let body = patch_payload(
        patch.section.as_u8(),
        patch.cfg,
        patch.index,
        patch.offset,
        &patch.bytes,
    );
    assert_eq!(body, &p[2..]);
}

#[cfg(feature = "mock")]
mod mock_roundtrip {
    use crate::error::Error;
    use crate::protocol::opcode::PATCH_MAX_ENTRIES;
    use crate::types::{Patch, PatchSection};
    use crate::{Device, MockBox};

    #[test]
    fn store_is_not_gated_on_the_opt_in() {
        // The box stores a patch whatever the opt-in; only APPLY is gated.
        let device = Device::with_mock(MockBox::new()); // opt-in off
        device
            .set_patch(&Patch::new(PatchSection::Device, 8, [0x34, 0x12]))
            .unwrap();
        let set = device.query_patches().unwrap();
        assert_eq!(set.entries.len(), 1);
        assert!(set.pending);
        assert!(!set.applied);
    }

    #[test]
    fn apply_requires_the_opt_in() {
        let device = Device::with_mock(MockBox::new());
        device
            .set_patch(&Patch::new(PatchSection::Device, 8, [0x34, 0x12]))
            .unwrap();
        assert!(matches!(
            device.apply_patch(),
            Err(Error::ImperfectRequired)
        ));
        assert!(!device.query_patches().unwrap().applied);
    }

    #[test]
    fn store_apply_and_clear_roundtrip() {
        let device = Device::with_mock(MockBox::new().with_imperfect(true));
        device
            .set_patch(&Patch::in_string(2, b"Medius".to_vec()))
            .unwrap();
        device.apply_patch().unwrap();
        let set = device.query_patches().unwrap();
        assert!(set.applied && !set.pending);
        assert_eq!(set.entries[0].section, PatchSection::String);
        assert!(device.query_health().unwrap().patch_on);

        device.clear_patch().unwrap();
        let set = device.query_patches().unwrap();
        assert!(set.entries.is_empty());
        assert!(!set.applied);
        assert!(!device.query_health().unwrap().patch_on);
    }

    #[test]
    fn empty_bytes_removes_a_patch() {
        let device = Device::with_mock(MockBox::new());
        device
            .set_patch(&Patch::new(PatchSection::Device, 8, [0x34, 0x12]))
            .unwrap();
        assert_eq!(device.query_patches().unwrap().entries.len(), 1);
        device
            .set_patch(&Patch::new(PatchSection::Device, 8, []))
            .unwrap();
        assert!(device.query_patches().unwrap().entries.is_empty());
    }

    #[test]
    fn query_entry_replays_a_stored_patch() {
        let device = Device::with_mock(MockBox::new());
        device
            .set_patch(&Patch::in_interface(1, 2, 4, [0xAB, 0xCD]))
            .unwrap();
        let read = device.query_patch_entry(0).unwrap();
        assert_eq!(read.section, PatchSection::Report);
        assert_eq!((read.cfg, read.index, read.offset), (1, 2, 4));
        assert_eq!(read.bytes, vec![0xAB, 0xCD]);
    }

    #[test]
    fn large_offset_and_long_patch_survive_the_roundtrip() {
        // No size gate: a 300-byte patch at offset >= 0x0100 round-trips through the store and the
        // entry readback (a u8 offset reads 258 as 2).
        let device = Device::with_mock(MockBox::new());
        device
            .set_patch(&Patch::in_interface(1, 2, 258, vec![0xAB; 300]))
            .unwrap();
        let set = device.query_patches().unwrap();
        assert_eq!(set.entries.len(), 1);
        assert_eq!(set.entries[0].offset, 258);
        assert_eq!(set.entries[0].len, 300);
        let read = device.query_patch_entry(0).unwrap();
        assert_eq!(read.offset, 258);
        assert_eq!(read.bytes, vec![0xAB; 300]);
    }

    // pending compares the stored set with the served one; applied is the served one being
    // non-empty. An edit or emptying after an apply leaves the clone as it was until the next apply.
    #[test]
    fn pending_is_the_stored_set_against_the_one_served() {
        let device = Device::with_mock(MockBox::new().with_imperfect(true));
        let flags = || {
            let set = device.query_patches().unwrap();
            (set.applied, set.pending)
        };
        let a = Patch::new(PatchSection::Device, 8, [0x34, 0x12]);
        device.set_patch(&a).unwrap();
        assert_eq!(flags(), (false, true), "stored, not yet applied");
        device.apply_patch().unwrap();
        assert_eq!(flags(), (true, false));

        device
            .set_patch(&Patch::new(PatchSection::Device, 10, [0x56]))
            .unwrap();
        assert_eq!(flags(), (true, true), "an edit after an apply is pending");
        device.apply_patch().unwrap();
        assert_eq!(flags(), (true, false));

        // Emptied over an applied clone: still served patched until the apply re-presents it bare.
        for off in [8, 10] {
            device
                .set_patch(&Patch::new(PatchSection::Device, off, []))
                .unwrap();
        }
        assert_eq!(flags(), (true, true), "an emptied set is pending");
        assert!(device.query_health().unwrap().patch_on);
        device.apply_patch().unwrap();
        assert_eq!(flags(), (false, false));
        assert!(!device.query_health().unwrap().patch_on);

        // With the opt-in off the clone is served bare and a stored set waits. The toggle presents
        // the clone on the box's next loop tick, after the command.
        let settle = |want: (bool, bool), what: &str| {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
            while flags() != want {
                assert!(std::time::Instant::now() < deadline, "{what}");
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        };
        device.set_patch(&a).unwrap();
        device.apply_patch().unwrap();
        device.allow_imperfect_clones(false).unwrap();
        settle((false, true), "the opt-in going off withdraws the set");
        device.allow_imperfect_clones(true).unwrap();
        settle((true, false), "the opt-in coming back re-presents the set");
    }

    #[test]
    fn clear_serves_the_clone_bare_in_place() {
        let device = Device::open_mock(MockBox::new().with_imperfect(true)).unwrap();
        device
            .set_patch(&Patch::new(PatchSection::Device, 8, [0x34, 0x12]))
            .unwrap();
        device.apply_patch().unwrap();
        device.clear_patch().unwrap();
        let set = device.query_patches().unwrap();
        assert!(set.entries.is_empty() && !set.applied && !set.pending);
        std::thread::sleep(std::time::Duration::from_millis(100));
        assert_eq!(device.counters().restarts, 0, "a clear is no restart");
    }

    // full means the last store was refused for room (count or pool); any accepted change resets
    // it. It is not a count of stored patches.
    #[test]
    fn table_full_is_the_last_store_refused_for_room() {
        let device = Device::with_mock(MockBox::new());
        let full = || device.query_patches().unwrap().table_full;
        for off in 0..PATCH_MAX_ENTRIES {
            device
                .set_patch(&Patch::new(PatchSection::Device, off as u16, [0xAB]))
                .unwrap();
        }
        assert_eq!(
            device.query_patches().unwrap().entries.len(),
            PATCH_MAX_ENTRIES
        );
        assert!(!full(), "a store at the count is no refusal");
        device
            .set_patch(&Patch::new(PatchSection::Device, 99, [0xAB]))
            .unwrap();
        assert!(full());
        assert_eq!(
            device.query_patches().unwrap().entries.len(),
            PATCH_MAX_ENTRIES
        );
        // Overwriting a stored key needs no new entry, so it goes in, and clears the flag.
        device
            .set_patch(&Patch::new(PatchSection::Device, 0, [0xCD]))
            .unwrap();
        assert!(!full());

        // The pool: 1024 bytes across every patch. An overwrite that does not fit keeps the old one.
        let device = Device::with_mock(MockBox::new());
        let full = || device.query_patches().unwrap().table_full;
        let big = |off: u16, len: usize| Patch::new(PatchSection::Device, off, vec![0x11; len]);
        device.set_patch(&big(0, 500)).unwrap();
        device.set_patch(&big(1, 500)).unwrap();
        device.set_patch(&big(2, 25)).unwrap();
        assert!(full(), "1025 bytes");
        device.set_patch(&big(2, 24)).unwrap();
        assert!(!full(), "1024 bytes fit");
        device.set_patch(&big(1, 501)).unwrap();
        assert!(full());
        assert_eq!(device.query_patch_entry(1).unwrap().bytes.len(), 500);
        device.set_patch(&big(2, 0)).unwrap();
        assert!(!full(), "a removal is a change");
    }

    // Re-storing a key's own bytes changes nothing: the patch keeps its place and a refusal for
    // room stays reported.
    #[test]
    fn re_storing_the_bytes_held_changes_nothing() {
        let device = Device::with_mock(MockBox::new());
        let first = Patch::new(PatchSection::Device, 8, [0x34, 0x12]);
        let second = Patch::new(PatchSection::Device, 10, [0x56]);
        device.set_patch(&first).unwrap();
        device.set_patch(&second).unwrap();
        device.set_patch(&first).unwrap();
        assert_eq!(device.query_patch_entry(0).unwrap(), first);
        // A new value for the key is a change, and moves it to the end.
        let changed = Patch::new(PatchSection::Device, 8, [0x35, 0x12]);
        device.set_patch(&changed).unwrap();
        assert_eq!(device.query_patch_entry(1).unwrap(), changed);

        for off in [12, 13, 14] {
            device
                .set_patch(&Patch::new(PatchSection::Device, off, vec![0x11; 500]))
                .unwrap();
        }
        assert!(device.query_patches().unwrap().table_full);
        device.set_patch(&second).unwrap();
        assert!(
            device.query_patches().unwrap().table_full,
            "an identical re-store is no change"
        );
    }
}
