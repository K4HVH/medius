//! SEQ correlation under concurrent queries (feature `mock`).
#![cfg(feature = "mock")]

use std::sync::Arc;
use std::thread;

use crate::{Device, Health, MockBox, Version};

#[test]
fn concurrent_queries_never_cross_deliver() {
    let mock = MockBox::new()
        .with_version(Version {
            proto_ver: 2,
            fw_major: 2,
            fw_minor: 3,
            fw_patch: 4,
        })
        .with_health(Health::from_flags(0x01));
    let device = Arc::new(Device::with_mock(mock));

    let mut handles = Vec::new();
    for _ in 0..8 {
        let d = Arc::clone(&device);
        handles.push(thread::spawn(move || {
            for _ in 0..25 {
                let v = d.query_version().unwrap();
                assert_eq!((v.fw_major, v.fw_minor, v.fw_patch), (2, 3, 4));
                let h = d.query_health().unwrap();
                assert!(h.link_up);
            }
        }));
    }
    for h in handles {
        h.join().unwrap();
    }
}
