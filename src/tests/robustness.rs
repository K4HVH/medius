//! Decoder robustness tests through the public API.
#![cfg(feature = "mock")]

use std::time::{Duration, Instant};

use crate::{Device, LogLevel, MockBox};

fn wait_until(mut f: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(1);
    while Instant::now() < deadline {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    f()
}

#[test]
fn garbage_then_valid_frame_resyncs_without_panicking() {
    let mock = MockBox::new();
    let device = Device::with_mock(mock.clone());
    let rx = device.logs();

    mock.push_raw(&[0x00, 0xFF, 0x13, 0x37, 0xAB, 0xCD, 0xEF, 0x42]);
    mock.push_log(LogLevel::Info, "alive");

    let line = rx
        .recv_timeout(Duration::from_secs(1))
        .expect("a valid frame must survive preceding junk");
    assert_eq!(line.text, "alive");
}

#[test]
fn bad_crc_frame_is_dropped_and_counted() {
    let mock = MockBox::new();
    let device = Device::with_mock(mock.clone());
    let before = device.counters().crc_drops;

    mock.push_raw(&[0xA5, 0x06, 0x00, 0x02, 0x00, 0x00, 0x01, 0xFF, 0xFF]);

    assert!(
        wait_until(|| device.counters().crc_drops > before),
        "a bad-CRC frame must be dropped and counted (crc_drops did not rise)"
    );
}

#[test]
fn truncated_frame_does_not_panic_and_reader_recovers() {
    let mock = MockBox::new();
    let device = Device::with_mock(mock.clone());
    let rx = device.logs();

    mock.push_raw(&[0xA5, 0x06, 0x00]);
    for i in 0..4u8 {
        mock.push_log(LogLevel::Info, &format!("m{i}"));
    }

    let mut got = Vec::new();
    while let Some(line) = rx.recv_timeout(Duration::from_millis(300)) {
        got.push(line.text);
    }
    assert!(
        !got.is_empty(),
        "reader must recover after a truncated frame, got {got:?}"
    );
}
