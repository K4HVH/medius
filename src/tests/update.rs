//! `RESP(FIRMWARE)` decoding (§4.16) and the chunk sequencing behind a staged image. Byte vectors are
//! read off the firmware's `ota_proto.h` and `control-protocol.md` §4.16, not written to match this
//! decoder: a wire decoder checked against its own author's expectations is a false green.

use crate::device::update::{ChunkPlan, begin_body};
use crate::protocol::opcode::{OTA_CHUNK, OTA_OP_DATA, Q_FIRMWARE};
use crate::protocol::{Resp, parse_resp};
use crate::types::{FirmwareInfo, ImageState, UpdateStatus, UpdateTarget};

fn payload(host_present: u8, staged: u8) -> [u8; 17] {
    [
        Q_FIRMWARE, // what
        3, 2, 0, // device version
        0,    // device slot ota_0
        2,    // device state valid
        host_present, 3, 2, 0,    // host version
        1,    // host slot ota_1
        1,    // host state pending-verify
        0x00, 0x00, 0x0F, 0x00, // slot size 0x000F0000 little-endian
        staged,
    ]
}

#[test]
fn decode_firmware_reports_both_chips() {
    let Some(Resp::Firmware(f)) = parse_resp(&payload(1, 0x03)) else {
        panic!("expected Firmware");
    };
    assert_eq!((f.device.major, f.device.minor, f.device.patch), (3, 2, 0));
    assert_eq!(f.device.slot, 0);
    assert_eq!(f.device.state, ImageState::Valid);
    let h = f.host.expect("host present");
    assert_eq!(h.slot, 1);
    assert_eq!(h.state, ImageState::PendingVerify);
    assert_eq!(f.slot_size, 0x000F_0000);
    assert!(f.device_staged && f.host_staged);
    assert!(f.any_pending(), "a pending host chip is still pending");
}

#[test]
fn decode_firmware_absent_host_is_none() {
    let Some(Resp::Firmware(f)) = parse_resp(&payload(0, 0)) else {
        panic!("expected Firmware");
    };
    assert!(f.host.is_none());
    assert!(!f.device_staged && !f.host_staged);
    assert!(!f.any_pending());
}

#[test]
fn decode_firmware_staged_bits_are_independent() {
    let dev_only = parse_resp(&payload(1, 0x01));
    let Some(Resp::Firmware(f)) = dev_only else {
        panic!("expected Firmware");
    };
    assert!(f.device_staged && !f.host_staged);
    let Some(Resp::Firmware(g)) = parse_resp(&payload(1, 0x02)) else {
        panic!("expected Firmware");
    };
    assert!(!g.device_staged && g.host_staged);
}

#[test]
fn decode_firmware_rejects_short_payload() {
    assert!(FirmwareInfo::from_payload(&payload(1, 0)[..16]).is_none());
    assert!(FirmwareInfo::from_payload(&[]).is_none());
    // A different selector in byte 0 is a different response, not a short one.
    let mut wrong = payload(1, 0);
    wrong[0] = 10;
    assert!(FirmwareInfo::from_payload(&wrong).is_none());
}

#[test]
fn image_states_round_trip_their_wire_values() {
    for (v, want) in [
        (0u8, ImageState::New),
        (1, ImageState::PendingVerify),
        (2, ImageState::Valid),
        (3, ImageState::Invalid),
        (4, ImageState::Aborted),
        (0xFF, ImageState::Unknown(0xFF)),
    ] {
        assert_eq!(ImageState::from_u8(v), want);
    }
    assert!(ImageState::PendingVerify.is_pending());
    assert!(!ImageState::Valid.is_pending());
}

#[test]
fn begin_body_is_the_length_then_the_digest() {
    let body = begin_body(&[1u8, 2, 3, 4]);
    assert_eq!(body.len(), 36);
    assert_eq!(&body[..4], &4u32.to_le_bytes());
    // Not the digest of anything else: a truncated or padded image must not produce the same 32 bytes.
    assert_ne!(&body[4..], &begin_body(&[1u8, 2, 3])[4..]);
}

#[test]
fn chunk_plan_fills_the_window_then_waits() {
    let image = vec![0u8; OTA_CHUNK * 20];
    let mut plan = ChunkPlan::new(&image, UpdateTarget::Device, 16);
    let mut frames = 0;
    while let Some(f) = plan.next_frame() {
        assert_eq!(f[0], OTA_OP_DATA);
        assert_eq!(f[1], 0);
        assert_eq!(u16::from_le_bytes([f[2], f[3]]), frames);
        frames += 1;
    }
    assert_eq!(frames, 16, "the window bounds what may be in flight");
    assert!(plan.awaiting_ack());
    assert!(!plan.done());
}

#[test]
fn chunk_plan_carries_on_after_an_ack() {
    let image = vec![0u8; OTA_CHUNK * 20];
    let mut plan = ChunkPlan::new(&image, UpdateTarget::Host, 16);
    while plan.next_frame().is_some() {}
    let p = plan.on_ack(UpdateStatus::ACK, 16).expect("window landed");
    assert_eq!(p.sent, OTA_CHUNK * 16);
    assert_eq!(p.total, image.len());
    assert_eq!(p.target, UpdateTarget::Host);
    let mut more = 0;
    while plan.next_frame().is_some() {
        more += 1;
    }
    assert_eq!(more, 4, "the tail is shorter than a full window");
    plan.on_ack(UpdateStatus::ACK, 20).expect("tail landed");
    assert!(plan.done());
}

#[test]
fn chunk_plan_refuses_an_offset_the_box_does_not_share() {
    let image = vec![0u8; OTA_CHUNK * 4];
    let mut plan = ChunkPlan::new(&image, UpdateTarget::Device, 16);
    while plan.next_frame().is_some() {}
    // The box says it wants chunk 2 while four have been sent: writing on would misplace every byte.
    let err = plan.on_ack(UpdateStatus::ACK, 2).unwrap_err();
    assert!(matches!(
        err,
        crate::Error::Update {
            status: UpdateStatus::SEQ_GAP,
            arg: 2,
            ..
        }
    ));
}

#[test]
fn chunk_plan_surfaces_a_refusal_verbatim() {
    let image = vec![0u8; OTA_CHUNK];
    let mut plan = ChunkPlan::new(&image, UpdateTarget::Device, 16);
    plan.next_frame();
    let err = plan.on_ack(UpdateStatus::WRITE_FAILED, 7).unwrap_err();
    assert!(matches!(
        err,
        crate::Error::Update {
            status: UpdateStatus::WRITE_FAILED,
            arg: 7,
            ..
        }
    ));
}

#[test]
fn a_short_final_chunk_is_sent_whole() {
    let image = vec![0u8; OTA_CHUNK + 9];
    let mut plan = ChunkPlan::new(&image, UpdateTarget::Device, 16);
    let first = plan.next_frame().expect("first");
    let second = plan.next_frame().expect("second");
    assert_eq!(first.len(), 4 + OTA_CHUNK);
    assert_eq!(second.len(), 4 + 9);
    assert!(plan.next_frame().is_none());
    assert!(plan.awaiting_ack(), "the last chunk always asks for an answer");
}

#[test]
fn a_zero_credit_falls_back_to_the_default() {
    let image = vec![0u8; OTA_CHUNK * 20];
    let mut plan = ChunkPlan::new(&image, UpdateTarget::Device, 0);
    let mut frames = 0;
    while plan.next_frame().is_some() {
        frames += 1;
    }
    assert_eq!(frames, 16);
}

#[test]
fn update_status_names_every_value_the_box_can_answer() {
    for s in [
        UpdateStatus::OK,
        UpdateStatus::READY,
        UpdateStatus::ACK,
        UpdateStatus::STAGED,
        UpdateStatus::BUSY,
        UpdateStatus::NO_SLOT,
        UpdateStatus::TOO_BIG,
        UpdateStatus::SEQ_GAP,
        UpdateStatus::WRITE_FAILED,
        UpdateStatus::BAD_SHA,
        UpdateStatus::BAD_IMAGE,
        UpdateStatus::LINK_DOWN,
        UpdateStatus::TIMEOUT,
        UpdateStatus::NOTHING_STAGED,
        UpdateStatus::BAD_STATE,
        UpdateStatus::ON_PROBATION,
    ] {
        assert_ne!(s.name(), "unknown", "{s} has no name");
    }
    assert_eq!(UpdateStatus(0x7F).name(), "unknown");
}

#[test]
fn update_targets_round_trip() {
    assert_eq!(UpdateTarget::from_u8(0), Some(UpdateTarget::Device));
    assert_eq!(UpdateTarget::from_u8(1), Some(UpdateTarget::Host));
    assert_eq!(UpdateTarget::from_u8(2), None);
    assert_eq!(UpdateTarget::Device.as_u8(), 0);
    assert_eq!(UpdateTarget::Host.as_u8(), 1);
}

#[cfg(feature = "mock")]
mod against_the_mock {
    use super::*;
    use crate::protocol::FrameType;

    fn image(n: usize) -> Vec<u8> {
        (0..n).map(|i| (i * 31 + 7) as u8).collect()
    }

    #[test]
    fn a_whole_image_stages_and_activates() {
        let mock = crate::MockBox::new();
        let dev = crate::Device::with_mock(mock.clone());
        let img = image(OTA_CHUNK * 18 + 13);
        let mut seen = Vec::new();
        let wrote = dev
            .stage_firmware(UpdateTarget::Device, &img, &mut |p| seen.push(p.sent))
            .expect("staged");
        assert_eq!(wrote, img.len() as u32);
        assert!(
            seen.last() == Some(&img.len()),
            "progress must end on the whole image, got {seen:?}"
        );
        assert!(dev.firmware_info().expect("info").device_staged);
        dev.activate_firmware().expect("activated");
        assert!(!dev.firmware_info().expect("info").device_staged);
    }

    #[test]
    fn every_image_byte_reaches_the_box_in_order() {
        let mock = crate::MockBox::new();
        let dev = crate::Device::with_mock(mock.clone());
        let img = image(OTA_CHUNK * 3 + 5);
        dev.stage_firmware(UpdateTarget::Device, &img, &mut |_| {})
            .expect("staged");
        let mut rebuilt = Vec::new();
        for f in mock.recorded_frames() {
            if f.ty == FrameType::Update && f.payload.first() == Some(&OTA_OP_DATA) {
                rebuilt.extend_from_slice(&f.payload[4..]);
            }
        }
        assert_eq!(rebuilt, img, "the bytes on the wire are the image");
    }

    #[test]
    fn activate_with_nothing_staged_is_refused() {
        let dev = crate::Device::with_mock(crate::MockBox::new());
        let err = dev.activate_firmware().unwrap_err();
        assert!(matches!(
            err,
            crate::Error::Update {
                status: UpdateStatus::NOTHING_STAGED,
                ..
            }
        ));
    }

    #[test]
    fn an_image_over_the_slot_is_refused_before_a_byte_is_sent() {
        let dev = crate::Device::with_mock(crate::MockBox::new());
        let huge = vec![0u8; 0xF_0000 + 1];
        let err = dev
            .stage_firmware(UpdateTarget::Device, &huge, &mut |_| {})
            .unwrap_err();
        assert!(matches!(
            err,
            crate::Error::Update {
                status: UpdateStatus::TOO_BIG,
                arg: 0xF_0000,
                ..
            }
        ));
    }

    #[test]
    fn an_empty_image_is_refused() {
        let dev = crate::Device::with_mock(crate::MockBox::new());
        assert!(
            dev.stage_firmware(UpdateTarget::Device, &[], &mut |_| {})
                .is_err()
        );
    }

    #[test]
    fn aborting_leaves_nothing_staged() {
        let dev = crate::Device::with_mock(crate::MockBox::new());
        let img = image(OTA_CHUNK * 2);
        dev.stage_firmware(UpdateTarget::Device, &img, &mut |_| {})
            .expect("staged");
        assert!(dev.firmware_info().expect("info").device_staged);
        dev.abort_update(UpdateTarget::Device).expect("aborted");
        assert!(!dev.firmware_info().expect("info").device_staged);
    }

    #[test]
    fn the_host_target_rides_the_same_path() {
        let mock = crate::MockBox::new();
        let dev = crate::Device::with_mock(mock.clone());
        let img = image(OTA_CHUNK + 1);
        dev.stage_firmware(UpdateTarget::Host, &img, &mut |_| {})
            .expect("staged");
        let targets: Vec<u8> = mock
            .recorded_frames()
            .iter()
            .filter(|f| f.ty == FrameType::Update)
            .map(|f| f.payload[1])
            .collect();
        assert!(
            targets.iter().all(|&t| t == 1),
            "every frame must name the host chip, got {targets:?}"
        );
    }
}
