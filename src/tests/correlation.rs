//! SEQ correlation invariants (FIX 1) — internal tests over `MockTransport` (no feature needed).
//!
//! These pin the generation-tagged, selector-aware correlation against cross-delivery: an unsolicited
//! `RESP` landing on a pending query's `SEQ`, and a still-pending query surviving a full `SEQ`-namespace
//! wrap. The public concurrency test exercises the common path; these target the exact boundaries that
//! hardware can't reproduce on demand.

use std::sync::Arc;
use std::time::Duration;

use crate::Device;
use crate::error::Error;
use crate::protocol::{FrameType, encode};
use crate::transport::mock::MockTransport;

/// An unsolicited `RESP(VERSION)` (the firmware boot/first-contact hello) landing on a `HEALTH` query's
/// `SEQ` must NOT corrupt it: selector-aware correlation drops the mismatched `VERSION` frame and the
/// real `HEALTH` reply still fulfils the waiter. Without the selector check this returns `NoReply` (the
/// `VERSION` payload fails to parse as `HEALTH`).
#[test]
fn mismatched_resp_selector_does_not_corrupt_query() {
    let mock = Arc::new(MockTransport::with_responder(|ty, seq, payload| {
        if ty == FrameType::Query && payload.first() == Some(&1) {
            // HEALTH query: emit a VERSION hello FIRST (same seq, wrong selector), then HEALTH.
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

/// SEQ-namespace wrap: a still-pending query A must NOT capture query B's `RESP` even after the rolling
/// `SEQ` wraps a full 256 back onto A's value. `register_pending` picks a free `SEQ`, forcing B onto a
/// different one; B resolves to its own value and A times out (no cross-delivery).
#[test]
fn pending_query_survives_seq_wrap_without_cross_delivery() {
    use std::sync::mpsc;

    // A mock that answers ONLY QUERY(HEALTH) (selector 1) — VERSION stays pending forever.
    let mock = Arc::new(MockTransport::with_responder(|ty, seq, payload| {
        if ty == FrameType::Query && payload.first().copied() == Some(1) {
            encode(FrameType::Resp, seq, &[1, 0x0B]).unwrap() // link|mouse|inject
        } else {
            Vec::new()
        }
    }));
    let device = Device::from_transport(mock);

    // Query A = VERSION, never answered; on its own thread so it blocks on its timeout while we wrap.
    let dev_a = device.clone();
    let (done_tx, done_rx) = mpsc::channel();
    let a = std::thread::spawn(move || {
        let r = dev_a.query_timeout(0, Duration::from_millis(400)); // selector 0 = VERSION
        let _ = done_tx.send(());
        r
    });
    std::thread::sleep(Duration::from_millis(20)); // let A register its waiter before we advance SEQ

    // A drew one SEQ; 255 more fire-and-go draws wrap the counter back onto A's value.
    for _ in 0..255 {
        device.move_rel(0, 0).unwrap();
    }

    // B = HEALTH: register_pending skips A's occupied SEQ, so B gets a free one and resolves to ITS value.
    let h = device.query_health().expect("B must resolve");
    assert!(h.link_up && h.mouse_attached && h.injection_active);

    // A must still be pending (not stolen by B's RESP); it then times out.
    assert!(
        done_rx.recv_timeout(Duration::from_millis(50)).is_err(),
        "A must NOT have completed early (no cross-delivery from B)"
    );
    assert!(
        matches!(a.join().unwrap(), Err(Error::QueryTimeout)),
        "A must time out"
    );
}
