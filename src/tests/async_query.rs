#![cfg(all(feature = "async", feature = "mock"))]

use futures::executor::block_on;

use crate::{Device, Error, MockBox, Version};

#[test]
fn async_query_returns_the_configured_version() {
    let mock = MockBox::new().with_version(Version {
        proto_ver: 1,
        fw_major: 1,
        fw_minor: 2,
        fw_patch: 3,
    });
    let device = Device::with_mock(mock).into_async();
    let v = block_on(device.query_version()).unwrap();
    assert_eq!((v.fw_major, v.fw_minor, v.fw_patch), (1, 2, 3));
}

#[test]
fn async_query_times_out_on_a_silent_box() {
    let device = Device::with_mock(MockBox::new().silent()).into_async();
    let err = block_on(device.query_version()).unwrap_err();
    assert!(matches!(err, Error::QueryTimeout), "got {err:?}");
}
