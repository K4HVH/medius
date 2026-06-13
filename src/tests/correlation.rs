use std::sync::Arc;
use std::time::Duration;

use crate::Device;
use crate::error::Error;
use crate::protocol::{FrameType, encode};
use crate::transport::mock::MockTransport;

#[test]
fn mismatched_resp_selector_does_not_corrupt_query() {
    let mock = Arc::new(MockTransport::with_responder(|ty, seq, payload| {
        if ty == FrameType::Query && payload.first() == Some(&1) {
            let mut out = encode(FrameType::Resp, seq, &[0, 1, 0, 1, 0]).unwrap();
            out.extend(encode(FrameType::Resp, seq, &[1, 0x0F]).unwrap());
            out
        } else {
            Vec::new()
        }
    }));
    let device = Device::from_transport(mock);
    let h = device.query_health().unwrap();
    assert!(h.link_up && h.mouse_attached && h.clone_configured && h.injection_active);
}

#[test]
fn pending_query_survives_seq_wrap_without_cross_delivery() {
    use std::sync::mpsc;

    let mock = Arc::new(MockTransport::with_responder(|ty, seq, payload| {
        if ty == FrameType::Query && payload.first().copied() == Some(1) {
            encode(FrameType::Resp, seq, &[1, 0x0B]).unwrap()
        } else {
            Vec::new()
        }
    }));
    let device = Device::from_transport(mock);

    let dev_a = device.clone();
    let (done_tx, done_rx) = mpsc::channel();
    let a = std::thread::spawn(move || {
        let r = dev_a.link.query_timeout(0, Duration::from_millis(400));
        let _ = done_tx.send(());
        r
    });
    std::thread::sleep(Duration::from_millis(20));

    for _ in 0..255 {
        device.move_rel(0, 0).unwrap();
    }

    let h = device.query_health().expect("B must resolve");
    assert!(h.link_up && h.mouse_attached && h.injection_active);

    assert!(
        done_rx.recv_timeout(Duration::from_millis(50)).is_err(),
        "A must NOT have completed early (no cross-delivery from B)"
    );
    assert!(
        matches!(a.join().unwrap(), Err(Error::QueryTimeout)),
        "A must time out"
    );
}

#[test]
fn stale_cancel_does_not_evict_newer_waiter() {
    let device = Device::from_transport(Arc::new(MockTransport::new()));

    let (seq_a, gen_a, _rx_a) = device.link.register_pending(0);
    device.link.cancel_query(seq_a, gen_a);
    assert_eq!(device.link.pending_len(), 0);

    for _ in 0..255 {
        let _ = device.link.next_seq();
    }

    let (seq_b, gen_b, rx_b) = device.link.register_pending(0);
    assert_eq!(seq_b, seq_a, "B reuses A's freed SEQ");
    assert_ne!(gen_b, gen_a, "B has a newer generation");

    device.link.cancel_query(seq_a, gen_a);
    assert_eq!(
        device.link.pending_len(),
        1,
        "a stale-gen cancel must not evict the newer waiter B"
    );

    device.link.cancel_query(seq_b, gen_b);
    assert_eq!(device.link.pending_len(), 0);
    drop(rx_b);
}
