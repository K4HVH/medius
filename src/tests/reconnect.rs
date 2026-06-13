//! Reconnect-path invariant (FIX 3) — internal test over `MockTransport` (no feature needed).
//!
//! A transport swap (as `reconnect` does) must reset the reader's `FrameDecoder`, so a frame
//! interrupted mid-parse on the old port cannot mis-frame the first bytes of the new one. The hardware
//! reconnect never sends a partial frame before swapping, so this edge case lives here.

use std::sync::Arc;
use std::time::Duration;

use crate::Device;
use crate::protocol::{FrameType, encode};
use crate::transport::mock::MockTransport;
use crate::types::LogLevel;

#[test]
fn transport_swap_resets_decoder() {
    let mock_a = Arc::new(MockTransport::new());
    // Long cadence so the keepalive doesn't inject frames during the window.
    let device = Device::from_transport_with_cadence(mock_a.clone(), Duration::from_secs(60));
    let rx = device.logs();

    // Push only the first half of a LOG frame on A, leaving the decoder mid-frame.
    let partial = encode(FrameType::Log, 0, &[2, b'o', b'l', b'd']).unwrap();
    let cut = partial.len() / 2;
    mock_a.push_bytes(&partial[..cut]);
    std::thread::sleep(Duration::from_millis(20)); // let the reader consume the partial bytes

    // Swap in a fresh transport (as reconnect does) and push a COMPLETE LOG on it.
    let mock_b = Arc::new(MockTransport::new());
    device.transport_slot().swap(mock_b.clone());
    mock_b.push_frame(FrameType::Log, 0, &[2, b'n', b'e', b'w']);

    // The complete LOG arriving intact proves the decoder was reset (A's dangling prefix didn't corrupt it).
    let line = rx
        .recv_timeout(Duration::from_secs(1))
        .expect("the post-swap LOG must decode cleanly");
    assert_eq!(line.level, LogLevel::Info);
    assert_eq!(line.text, "new");
}
