//! Fully runnable, HARDWARE-FREE demo of the `mock` feature.
//!
//! Builds a scriptable `MockBox`, drives a real `Device` over it via `Device::with_mock`, runs
//! queries + a press, and asserts on the commands the host recorded. No box required.
//!
//! Run:
//!     cargo run --example mock --features mock

use medius::mock::MockBox;
use medius::protocol::FrameType;
use medius::{Button, Device, Health, Version};

fn main() {
    // Configure the fake box: the Version/Health it will answer queries with.
    let mock = MockBox::new()
        .with_version(Version {
            proto_ver: 1,
            fw_major: 2,
            fw_minor: 3,
            fw_patch: 4,
        })
        .with_health(Health::from_flags(0x0F)); // link | mouse | clone | inject

    // Drive the REAL device stack over the fake box. `mock.clone()` keeps a handle for assertions
    // (MockBox shares its state/transport via Arc).
    let device = Device::with_mock(mock.clone());

    // Queries resolve against the configured values (the same SEQ-correlated path as real hardware).
    let version = device.query_version().expect("version query resolves");
    let health = device.query_health().expect("health query resolves");
    println!("mock version: {version}");
    assert_eq!(
        (version.fw_major, version.fw_minor, version.fw_patch),
        (2, 3, 4)
    );
    assert!(
        health.link_up
            && health.mouse_attached
            && health.clone_configured
            && health.injection_active
    );

    // A fire-and-go command is recorded as a decoded frame.
    device.press(Button::Left).expect("press records");

    let frames = mock.recorded_frames();
    println!("recorded {} frame(s):", frames.len());
    for f in &frames {
        println!("  {:?} seq={} payload={:?}", f.ty, f.seq, f.payload);
    }

    // The press is a BUTTON frame with payload [id=Left(0)][action=press(1)].
    let button = frames
        .iter()
        .find(|f| f.ty == FrameType::Button)
        .expect("a BUTTON frame was recorded");
    assert_eq!(button.payload, vec![0, 1]);
    assert!(mock.saw(FrameType::Button));

    println!("OK: mock demo passed");
}
