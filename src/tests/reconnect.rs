use std::sync::Arc;
use std::time::Duration;

use crate::Device;
use crate::protocol::{FrameType, encode};
use crate::transport::mock::MockTransport;
use crate::types::LogLevel;

#[test]
fn transport_swap_resets_decoder() {
    let mock_a = Arc::new(MockTransport::new());
    let device = Device::from_transport_with_cadence(mock_a.clone(), Duration::from_secs(60));
    let rx = device.logs();

    let partial = encode(FrameType::Log, 0, &[2, b'o', b'l', b'd']).unwrap();
    let cut = partial.len() / 2;
    mock_a.push_bytes(&partial[..cut]);
    std::thread::sleep(Duration::from_millis(20));

    let mock_b = Arc::new(MockTransport::new());
    device.link.transport_slot().swap(mock_b.clone());
    mock_b.push_frame(FrameType::Log, 0, &[2, b'n', b'e', b'w']);

    let line = rx
        .recv_timeout(Duration::from_secs(1))
        .expect("the post-swap LOG must decode cleanly");
    assert_eq!(line.level, LogLevel::Info);
    assert_eq!(line.text, "new");
}
