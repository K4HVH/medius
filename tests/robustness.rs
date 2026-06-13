//! Decoder robustness through the **public** API + `MockBox::push_raw` (feature `mock`).
//!
//! The hardware suite only exercises the happy path; these cover what it never feeds the box: garbage,
//! truncation, and bad-CRC frames. A bug here is silent (a dropped or mis-framed input → a stuck button
//! or missed motion), so this is the high-value, hardware-unreachable coverage.
#![cfg(feature = "mock")]

use std::time::{Duration, Instant};

use medius::{Device, LogLevel, MockBox};

/// Poll `f` until true or a 1 s deadline (the reader processes inbound bytes on its own thread).
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

    // Pure junk with no SOF — the decoder must skip it, not panic or wedge.
    mock.push_raw(&[0x00, 0xFF, 0x13, 0x37, 0xAB, 0xCD, 0xEF, 0x42]);
    // A valid LOG frame right after must still arrive (resync past the junk).
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

    // A well-formed RESP frame with a deliberately wrong CRC:
    // [SOF 0xA5][TYPE Resp=0x06][SEQ 0][LEN=2 LE][payload 0,1][CRC 0xFFFF — wrong].
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

    // SOF + a partial header, no body — must not panic the decoder. A truncated frame consumes at most
    // the next frame's opening bytes before the decoder resyncs on the following SOF, so of several
    // valid frames that follow, the reader must still deliver the rest (i.e. it recovers, not wedges).
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
