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
}
