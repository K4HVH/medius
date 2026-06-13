//! `AsyncDevice` queries against the hardware-free `mock` box, driven by `futures::executor::block_on`.
//!
//! No hardware; requires the `async` and `mock` features. The async wrapper is runtime-agnostic —
//! `flume`'s `recv_async()` future is pollable by any executor (here `futures` block-on; tokio /
//! async-std / smol work identically). The crate pulls in no tokio dependency.
//!
//!     cargo run --example async --features async,mock

use futures::executor::block_on;

use medius::mock::MockBox;
use medius::{Button, Device, Version};

fn main() {
    let mock = MockBox::new().with_version(Version {
        proto_ver: 1,
        fw_major: 7,
        fw_minor: 8,
        fw_patch: 9,
    });

    // Same core as the sync Device (one Arc<Inner>, one reader, one transport).
    let device = Device::with_mock(mock).into_async();

    // Only queries are `async`; fire-and-go methods are instant non-blocking writes.
    device.press(Button::Left).expect("press is instant");

    let version = block_on(device.query_version()).expect("async version query resolves");
    println!("async mock version: {version}");
    assert_eq!(
        (version.fw_major, version.fw_minor, version.fw_patch),
        (7, 8, 9)
    );

    println!("OK: async demo passed");
}
