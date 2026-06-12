//! `AsyncDevice` queries against the hardware-free `mock` box, driven by `futures::executor::block_on`.
//!
//! Fully runnable with no hardware. Requires BOTH the `async` and `mock` features. The async wrapper
//! is runtime-agnostic — `flume`'s `recv_async()` future is pollable by any executor (here the tiny
//! `futures` block-on, but tokio / async-std / smol work identically). No tokio dependency is pulled
//! in by the crate.
//!
//! Run:
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

    // The async view is the SAME core as the sync Device (one Arc<Inner>, one reader, one transport).
    let device = Device::with_mock(mock).into_async();

    // Only the queries are `async`; fire-and-go methods are instant non-blocking writes.
    device.press(Button::Left).expect("press is instant");

    // Await the SEQ-correlated RESP on any executor.
    let version = block_on(device.query_version()).expect("async version query resolves");
    println!("async mock version: {version}");
    assert_eq!(
        (version.fw_major, version.fw_minor, version.fw_patch),
        (7, 8, 9)
    );

    println!("OK: async demo passed");
}
