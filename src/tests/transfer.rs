//! `RAW` and `TRANSFER` (§3.14): payload bytes, the setup packet, the status and stage bytes, the
//! checks made before the wire, and a reply assembled from its chunks.

use std::time::Duration;

use crate::device::raw::{transfer_timeout_ms, validate_transfer_data};
use crate::error::Error;
use crate::protocol::command::{raw_payload, transfer_payload};
use crate::types::{Direction, Setup, TransferStage, TransferStatus};

#[test]
fn raw_payload_bytes() {
    // [ep_num][dir][bytes...]: endpoint 3 IN, then the report; number and direction differ, so a
    // transpose shows.
    assert_eq!(
        raw_payload(3, Direction::IN, &[0x00, 0x01, 0x00, 0x00]),
        vec![0x03, 0x01, 0x00, 0x01, 0x00, 0x00]
    );
    // Endpoint 2 OUT, empty body. The ep is masked to its low nibble, so a direction packed into
    // bit 7 lands on the same number.
    assert_eq!(raw_payload(0x82, Direction::OUT, &[]), vec![0x02, 0x02]);
}

#[test]
fn setup_bytes_are_little_endian() {
    // GET_DESCRIPTOR(Device): bmRequestType 0x80, bRequest 0x06, wValue 0x0100, wIndex 0, wLength 18.
    let s = Setup::new(0x80, 0x06, 0x0100, 0x0000, 18);
    assert_eq!(
        s.to_bytes(),
        [0x80, 0x06, 0x00, 0x01, 0x00, 0x00, 0x12, 0x00]
    );
    assert!(s.is_in());
    assert!(!Setup::new(0x00, 0x09, 0, 0, 0).is_in()); // an OUT (host-to-device) request
}

#[test]
fn transfer_payload_bytes() {
    let s = Setup::new(0x80, 0x06, 0x0100, 0x0000, 18);
    // [ep 0][timeout 1000 LE][setup 8][no OUT data].
    assert_eq!(
        transfer_payload(0, 1000, s, &[]),
        vec![
            0x00, 0xE8, 0x03, 0x80, 0x06, 0x00, 0x01, 0x00, 0x00, 0x12, 0x00
        ]
    );
    // An OUT transfer carries its data after the setup packet; the timeout's bytes differ, so a
    // swapped pair shows.
    let out = Setup::new(0x00, 0x09, 0x0200, 0x0000, 2);
    assert_eq!(
        transfer_payload(3, 0x1234, out, &[0xAA, 0xBB]),
        vec![
            0x03, 0x34, 0x12, 0x00, 0x09, 0x00, 0x02, 0x00, 0x00, 0x02, 0x00, 0xAA, 0xBB
        ]
    );
}

#[test]
fn transfer_status_and_stage_wire() {
    for (byte, status) in [
        (0x00, TransferStatus::Ok),
        (0xF9, TransferStatus::Busy),
        (0xFA, TransferStatus::Suspended),
        (0xFB, TransferStatus::Error),
        (0xFC, TransferStatus::Refused),
        (0xFD, TransferStatus::Stall),
        (0xFE, TransferStatus::NoAnswer),
        (0xFF, TransferStatus::NoDevice),
        (0x42, TransferStatus::Other(0x42)),
    ] {
        assert_eq!(TransferStatus::from_u8(byte), status);
        assert_eq!(status.as_u8(), byte);
    }
    assert!(TransferStatus::Ok.is_ok());
    assert!(!TransferStatus::Busy.is_ok());
    for (byte, stage) in [
        (0, TransferStage::Setup),
        (1, TransferStage::Data),
        (2, TransferStage::Status),
        (3, TransferStage::Other(3)),
    ] {
        assert_eq!(TransferStage::from_u8(byte), stage);
        assert_eq!(stage.as_u8(), byte);
    }
}

#[test]
fn the_timeout_is_whole_milliseconds_the_box_takes() {
    let ms = |d: Duration| transfer_timeout_ms(d).ok();
    assert_eq!(ms(Duration::from_millis(1)), Some(1));
    assert_eq!(ms(Duration::from_micros(1_999)), Some(1));
    assert_eq!(ms(Duration::from_millis(65_535)), Some(65_535));
    // The box refuses 0, and the field is 16 bits: neither end is clamped into range.
    for d in [
        Duration::ZERO,
        Duration::from_micros(999),
        Duration::from_millis(65_536),
        Duration::MAX,
    ] {
        let refused = matches!(
            transfer_timeout_ms(d),
            Err(Error::TransferTimeoutRange { timeout }) if timeout == d
        );
        assert!(refused, "{d:?}");
    }
}

#[test]
fn out_data_is_wlength_bytes_for_out_and_none_for_in() {
    let set = Setup::new(0x21, 0x09, 0x0200, 0, 2);
    let get = Setup::new(0xA1, 0x01, 0x0100, 0, 8);
    assert!(validate_transfer_data(set, &[1, 2]).is_ok());
    assert!(validate_transfer_data(get, &[]).is_ok());
    assert!(matches!(
        validate_transfer_data(set, &[1]),
        Err(Error::TransferData { want: 2, got: 1 })
    ));
    assert!(matches!(
        validate_transfer_data(set, &[1, 2, 3]),
        Err(Error::TransferData { want: 2, got: 3 })
    ));
    assert!(matches!(
        validate_transfer_data(get, &[1]),
        Err(Error::TransferData { want: 0, got: 1 })
    ));
}

// Replies scripted a chunk at a time, as the box sends them or as a lossy link delivers them.
mod chunks {
    use std::sync::Arc;
    use std::time::Duration;

    use crate::Device;
    use crate::error::Error;
    use crate::protocol::opcode::XFER_LAST;
    use crate::protocol::{FrameType, encode};
    use crate::transport::mock::MockTransport;
    use crate::types::{Setup, TransferStage, TransferStatus};

    fn chunk(
        seq: u8,
        ep: u8,
        status: u8,
        stage: u8,
        flags: u8,
        offset: u16,
        data: &[u8],
    ) -> Vec<u8> {
        let mut p = vec![ep, status, stage, flags];
        p.extend_from_slice(&offset.to_le_bytes());
        p.extend_from_slice(data);
        encode(FrameType::TransferResp, seq, &p).unwrap()
    }

    fn device(reply: impl Fn(u8, u8) -> Vec<u8> + Send + Sync + 'static) -> Device {
        Device::from_transport(Arc::new(MockTransport::with_responder(
            move |ty, seq, payload| {
                if ty == FrameType::Transfer {
                    reply(seq, payload[0])
                } else {
                    Vec::new()
                }
            },
        )))
    }

    const GET_CONFIG: Setup = Setup {
        request_type: 0x80,
        request: 0x06,
        value: 0x0200,
        index: 0,
        length: 0xFFFF,
    };

    #[test]
    fn only_the_last_chunk_names_the_outcome() {
        let a: Vec<u8> = (0..448).map(|i| i as u8).collect();
        let b: Vec<u8> = (0..100).map(|i| !(i as u8)).collect();
        let (a2, b2) = (a.clone(), b.clone());
        // As the host chip sends them: chunks before the last say OK in the data stage.
        let dev = device(move |seq, ep| {
            let mut out = chunk(seq, ep, 0x00, 1, 0, 0, &a2);
            out.extend(chunk(seq, ep, 0x00, 1, 0, 448, &b2));
            out.extend(chunk(seq, ep, 0xFD, 1, XFER_LAST, 548, &[]));
            out
        });
        let reply = dev.transfer(0, GET_CONFIG, &[]).unwrap();
        assert_eq!(reply.status, TransferStatus::Stall);
        assert_eq!(reply.stage, TransferStage::Data);
        assert_eq!(reply.data, [a, b].concat());
    }

    #[test]
    fn a_reply_that_lost_a_chunk_is_an_error() {
        let dev = device(|seq, ep| {
            let mut out = chunk(seq, ep, 0, 1, 0, 0, &[0x11; 448]);
            out.extend(chunk(seq, ep, 0x00, 2, XFER_LAST, 896, &[0x22; 20]));
            out
        });
        assert!(matches!(
            dev.transfer(0, GET_CONFIG, &[]),
            Err(Error::TransferReplyIncomplete { got: 468, len: 916 })
        ));
    }

    #[test]
    fn frames_that_are_not_this_transfers_chunks_are_skipped() {
        let dev = device(|seq, ep| {
            // A chunk too short for its header, one echoing another endpoint and a query reply on
            // the same SEQ all leave the reply as the real chunks build it.
            let mut out = encode(FrameType::TransferResp, seq, &[ep, 0, 0, XFER_LAST]).unwrap();
            out.extend(chunk(seq, ep ^ 1, 0xFF, 0, XFER_LAST, 0, &[0x99]));
            out.extend(encode(FrameType::Resp, seq, &[ep, 0x00, 0x02, XFER_LAST, 0, 0]).unwrap());
            out.extend(chunk(seq, ep, 0x00, 2, XFER_LAST, 0, &[0x12, 0x01]));
            out
        });
        let reply = dev.transfer(5, GET_CONFIG, &[]).unwrap();
        assert_eq!(reply.status, TransferStatus::Ok);
        assert_eq!(reply.stage, TransferStage::Status);
        assert_eq!(reply.data, [0x12, 0x01]);
        assert_eq!(dev.link.pending_len(), 0);
    }

    #[cfg(feature = "mock")]
    #[test]
    fn the_wait_runs_past_the_box_timeout() {
        use crate::MockBox;

        let mock = MockBox::new().silent();
        let dev = Device::with_mock(mock.clone());
        let pusher = {
            let mock = mock.clone();
            std::thread::spawn(move || {
                let seq = loop {
                    if let Some(f) = mock
                        .recorded_frames()
                        .into_iter()
                        .find(|f| f.ty == FrameType::Transfer)
                    {
                        break f.seq;
                    }
                    std::thread::sleep(Duration::from_millis(5));
                };
                std::thread::sleep(Duration::from_millis(1200));
                mock.push_raw(&chunk(seq, 0, 0xFE, 0, XFER_LAST, 0, &[]));
            })
        };
        let reply = dev
            .transfer_timeout(0, GET_CONFIG, &[], Duration::from_millis(1))
            .unwrap();
        pusher.join().unwrap();
        assert_eq!(reply.status, TransferStatus::NoAnswer);
        assert_eq!(reply.stage, TransferStage::Setup);
    }

    #[test]
    fn a_reply_that_never_ends_times_out_and_frees_its_seq() {
        let dev = device(|seq, ep| chunk(seq, ep, 0, 1, 0, 0, &[0x33; 64]));
        let started = std::time::Instant::now();
        assert!(matches!(
            dev.transfer_timeout(0, GET_CONFIG, &[], Duration::from_millis(1)),
            Err(Error::QueryTimeout)
        ));
        assert!(started.elapsed() >= Duration::from_secs(2));
        assert_eq!(dev.link.pending_len(), 0);
    }
}

#[cfg(feature = "mock")]
mod mock_roundtrip {
    use std::time::Duration;

    use crate::error::Error;
    use crate::types::{Direction, Setup, TransferOutcome, TransferStage, TransferStatus};
    use crate::{Device, FrameType, MockBox};

    const GET_DEVICE: Setup = Setup {
        request_type: 0x80,
        request: 0x06,
        value: 0x0100,
        index: 0,
        length: 18,
    };

    #[test]
    fn raw_sends_without_reading_the_opt_in() {
        let mock = MockBox::new(); // opt-in off: the box drops the frame, the crate still sends it
        let device = Device::with_mock(mock.clone());
        device.raw(1, Direction::IN, &[0x00]).unwrap();
        assert!(mock.saw(FrameType::Raw));
        assert!(!mock.saw(FrameType::Query));
    }

    #[test]
    fn raw_sends_when_allowed() {
        let mock = MockBox::new().with_imperfect(true);
        let device = Device::with_mock(mock.clone());
        device
            .raw(1, Direction::IN, &[0x00, 0x01, 0x00, 0x00])
            .unwrap();
        assert!(mock.saw(FrameType::Raw));
    }

    #[test]
    fn raw_rejects_a_direction_that_is_not_a_flow() {
        let device = Device::with_mock(MockBox::new().with_imperfect(true));
        // Both names two flows and the bearing-relative pair has no bearing here; both are refused
        // before the wire.
        assert!(matches!(
            device.raw(1, Direction::Both, &[0x00]),
            Err(Error::RawDirection { .. })
        ));
        assert!(matches!(
            device.raw(1, Direction::With, &[0x00]),
            Err(Error::RelativeDirection { .. })
        ));
    }

    #[test]
    fn the_mock_sends_a_long_reply_in_chunks_with_its_status_and_stage() {
        let data: Vec<u8> = (0..1000u32).map(|i| (i * 7) as u8).collect();
        let mock = MockBox::new()
            .with_imperfect(true)
            .with_transfer_reply(TransferOutcome {
                status: TransferStatus::Stall,
                stage: TransferStage::Data,
                data: data.clone(),
            });
        let device = Device::with_mock(mock.clone());
        let reply = device.transfer(0, GET_DEVICE, &[]).unwrap();
        assert_eq!(reply.status, TransferStatus::Stall);
        assert_eq!(reply.stage, TransferStage::Data);
        assert_eq!(reply.data, data);
        let chunks: Vec<_> = mock
            .replied_frames()
            .into_iter()
            .filter(|f| f.ty == FrameType::TransferResp)
            .collect();
        assert_eq!(chunks.len(), 3, "448 + 448 + 104 bytes");
    }

    #[test]
    fn the_timeout_goes_on_the_wire() {
        let mock = MockBox::new().with_imperfect(true);
        let device = Device::with_mock(mock.clone());
        device.transfer(0, GET_DEVICE, &[]).unwrap();
        device
            .transfer_timeout(0, GET_DEVICE, &[], Duration::from_millis(40_000))
            .unwrap();
        let timeouts: Vec<u16> = mock
            .recorded_frames()
            .into_iter()
            .filter(|f| f.ty == FrameType::Transfer)
            .map(|f| u16::from_le_bytes([f.payload[1], f.payload[2]]))
            .collect();
        assert_eq!(timeouts, [1000, 40_000]);
    }

    #[test]
    fn transfer_refused_when_opt_in_off() {
        // No pre-check: the box itself replies REFUSED, which is more faithful than a local error.
        let device = Device::with_mock(MockBox::new());
        let reply = device.transfer(0, GET_DEVICE, &[]).unwrap();
        assert_eq!(reply.status, TransferStatus::Refused);
        assert_eq!(reply.stage, TransferStage::Setup);
        assert!(reply.data().is_empty());
    }

    #[test]
    fn a_transfer_the_box_would_refuse_never_reaches_the_wire() {
        let mock = MockBox::new().with_imperfect(true);
        let device = Device::with_mock(mock.clone());
        let set = Setup::new(0x21, 0x09, 0x0200, 0, 2);
        assert!(matches!(
            device.transfer(0, set, &[1]),
            Err(Error::TransferData { want: 2, got: 1 })
        ));
        assert!(matches!(
            device.transfer_timeout(0, GET_DEVICE, &[], Duration::ZERO),
            Err(Error::TransferTimeoutRange { .. })
        ));
        assert!(!mock.saw(FrameType::Transfer));
        // A data stage one frame cannot carry is refused at the frame, and its SEQ is freed.
        let big = Setup::new(0x21, 0x09, 0x0200, 0, 502);
        assert!(matches!(
            device.transfer(0, big, &[0; 502]),
            Err(Error::FrameTooLong)
        ));
        assert!(!mock.saw(FrameType::Transfer));
        assert_eq!(device.link.pending_len(), 0);
        let fits = Setup::new(0x21, 0x09, 0x0200, 0, 501);
        assert!(device.transfer(0, fits, &[0; 501]).unwrap().is_ok());
    }
}
