//! `RAW` and `TRANSFER` (§3.14): payload bytes, the setup packet, `TransferStatus`, and the MockBox
//! round-trip including the opt-in gate.

use crate::protocol::command::{raw_payload, transfer_payload};
use crate::types::{Setup, TransferStatus};

#[test]
fn raw_payload_bytes() {
    assert_eq!(
        raw_payload(0x81, &[0x00, 0x01, 0x00, 0x00]),
        vec![0x81, 0x00, 0x01, 0x00, 0x00]
    );
    assert_eq!(raw_payload(0x02, &[]), vec![0x02]);
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
    // [ep 0][setup 8][no OUT data].
    assert_eq!(
        transfer_payload(0, s, &[]),
        vec![0x00, 0x80, 0x06, 0x00, 0x01, 0x00, 0x00, 0x12, 0x00]
    );
    // An OUT transfer carries its data after the setup packet.
    let out = Setup::new(0x00, 0x09, 0x0200, 0x0000, 2);
    assert_eq!(
        transfer_payload(0, out, &[0xAA, 0xBB]),
        vec![
            0x00, 0x00, 0x09, 0x00, 0x02, 0x00, 0x00, 0x02, 0x00, 0xAA, 0xBB
        ]
    );
}

#[test]
fn transfer_status_wire() {
    assert_eq!(TransferStatus::from_u8(0x00), TransferStatus::Ok);
    assert_eq!(TransferStatus::from_u8(0xFD), TransferStatus::Stall);
    assert_eq!(TransferStatus::from_u8(0xFE), TransferStatus::Nak);
    assert_eq!(TransferStatus::from_u8(0xFF), TransferStatus::NoDevice);
    assert_eq!(TransferStatus::from_u8(0xFC), TransferStatus::Refused);
    assert_eq!(TransferStatus::from_u8(0x42), TransferStatus::Other(0x42));
    assert!(TransferStatus::Ok.is_ok());
    assert!(!TransferStatus::Stall.is_ok());
    assert_eq!(TransferStatus::Refused.as_u8(), 0xFC);
    assert_eq!(TransferStatus::Other(0x42).as_u8(), 0x42);
}

#[cfg(feature = "mock")]
mod mock_roundtrip {
    use crate::error::Error;
    use crate::types::{Setup, TransferStatus};
    use crate::{Device, FrameType, MockBox};

    #[test]
    fn raw_requires_the_opt_in() {
        let device = Device::with_mock(MockBox::new());
        assert!(matches!(
            device.raw(0x81, &[0x00]),
            Err(Error::ImperfectRequired)
        ));
    }

    #[test]
    fn raw_sends_when_allowed() {
        let mock = MockBox::new().with_imperfect(true);
        let device = Device::with_mock(mock.clone());
        device.raw(0x81, &[0x00, 0x01, 0x00, 0x00]).unwrap();
        assert!(mock.saw(FrameType::Raw));
    }

    #[test]
    fn transfer_returns_the_devices_answer() {
        let mock = MockBox::new()
            .with_imperfect(true)
            .with_transfer_reply(0x00, &[0x12, 0x01, 0x00, 0x02]);
        let device = Device::with_mock(mock);
        let reply = device
            .transfer(0, Setup::new(0x80, 0x06, 0x0100, 0x0000, 18), &[])
            .unwrap();
        assert_eq!(reply.status, TransferStatus::Ok);
        assert_eq!(reply.data(), &[0x12, 0x01, 0x00, 0x02]);
        assert!(reply.is_ok());
    }

    #[test]
    fn transfer_refused_when_opt_in_off() {
        // No pre-check: the box itself answers 0xFC (refused), which is more faithful than a local error.
        let device = Device::with_mock(MockBox::new());
        let reply = device
            .transfer(0, Setup::new(0x80, 0x06, 0x0100, 0x0000, 18), &[])
            .unwrap();
        assert_eq!(reply.status, TransferStatus::Refused);
        assert!(reply.data().is_empty());
    }

    #[test]
    fn transfer_carries_a_stall_status() {
        let mock = MockBox::new()
            .with_imperfect(true)
            .with_transfer_reply(0xFD, &[]);
        let device = Device::with_mock(mock);
        let reply = device
            .transfer(0, Setup::new(0x80, 0x06, 0x0100, 0x0000, 18), &[])
            .unwrap();
        assert_eq!(reply.status, TransferStatus::Stall);
        assert!(!reply.is_ok());
    }

    #[test]
    fn transfer_non_ok_status_carries_no_data() {
        // The box sets in_len = 0 unless status == 0 (usbdev_transfer). A stall scripted with data must
        // come back with the data dropped, not passed through: no non-OK transfer ever carries bytes.
        let mock = MockBox::new()
            .with_imperfect(true)
            .with_transfer_reply(0xFD, &[0x12, 0x01, 0x00, 0x02]);
        let device = Device::with_mock(mock);
        let reply = device
            .transfer(0, Setup::new(0x80, 0x06, 0x0100, 0x0000, 18), &[])
            .unwrap();
        assert_eq!(reply.status, TransferStatus::Stall);
        assert!(reply.data().is_empty(), "a non-OK transfer carries no data");
    }
}
