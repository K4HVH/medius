//! Decoder resilience tests through the public API.
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

// Every reply the box builds is bounded, and past its bound the firmware appends what fits and
// answers anyway (ctrl_locks_append, ctrl_catch_append, ctrl_clip_held_append, ctrl_clip_trig_append,
// ctrl_usage_append, ctrl_pack_traffic_event, usbdev_box_name_copy, the DEVICE_INFO product copy).
// The mock's builders take caller-supplied lengths from public setters, so each has to truncate the
// same way: unbounded, the count byte wraps past 255 or the payload outgrows a frame, and the
// `encode` failure unwinds out of the caller's own query instead of answering it.
use crate::types::{
    CatchClass, CatchEntry, CatchFilter, CatchState, ClipAction, ClipSettings, ClipStatus,
    ClipTrigger, ClockDomain, DeviceInfo, Direction, Edge, Key, MediaKey, Usage, Version,
};

fn many_usages(n: usize) -> Vec<Usage> {
    (0..n)
        .map(|i| Usage::from(MediaKey::new(0x100 + i as u16)))
        .collect()
}

#[test]
fn a_name_past_the_wire_cap_still_answers_and_reads_back_cut() {
    // CTRL_NAME_MAX is 32, and 600 bytes is past what a frame carries at all.
    let mock = MockBox::new();
    mock.set_version(Version {
        proto_ver: crate::protocol::PROTO_VER,
        fw_major: 3,
        fw_minor: 2,
        fw_patch: 0,
        mac: [0; 6],
        name: "n".repeat(600),
    });
    let device = Device::with_mock(mock);
    assert_eq!(device.query_version().unwrap().name, "n".repeat(32));
}

#[test]
fn a_product_past_the_wire_cap_still_answers_and_reads_back_cut() {
    // CTRL_DEVICE_INFO_PRODUCT_MAX is 127.
    let mock = MockBox::new().with_device_info(DeviceInfo {
        product: "p".repeat(600),
        ..DeviceInfo::default()
    });
    let device = Device::with_mock(mock);
    assert_eq!(device.device_info().unwrap().product, "p".repeat(127));
}

#[test]
fn a_catch_table_past_the_wire_cap_still_answers() {
    // CTRL_CATCH_MAXN is 32: the box's table cannot hold more, so its reply cannot name more.
    let entries: Vec<CatchEntry> = (0..256u16)
        .map(|i| CatchEntry {
            filter: CatchFilter::traffic(crate::types::TrafficClass::HidIn, i),
            dropped: 0,
        })
        .collect();
    let mock = MockBox::new().with_catch_state(CatchState {
        table_full: false,
        dropped: 7,
        clock: crate::types::ClockEstimate {
            offset_us: 0,
            rate_ppb: None,
            delay_us: 0,
            age: None,
        },
        entries,
    });
    let device = Device::with_mock(mock);
    let got = device.query_catch().unwrap();
    assert_eq!(got.entries.len(), 32);
    assert_eq!(got.dropped, 7);
    // The first 32 are what survived, in order.
    assert_eq!(got.entries[0].filter.id(), Some(0));
    assert_eq!(got.entries[31].filter.id(), Some(31));
}

#[test]
fn a_clip_status_past_the_wire_caps_still_answers() {
    // CTRL_CLIP_HELD_MAX is 40 (= CTRL_USAGE_EVENT_MAX) and CLIP_TRIG_MAX is 8.
    let triggers: Vec<ClipTrigger> = (0..256u16)
        .map(|i| ClipTrigger {
            on: Usage::from(MediaKey::new(0x200 + i)),
            edge: Edge::Press,
            action: ClipAction::Start,
            consume: false,
        })
        .collect();
    let mock = MockBox::new();
    mock.set_clip_status(ClipStatus {
        ticks: 99,
        held: many_usages(256),
        ..ClipStatus::default()
    });
    mock.set_clip_settings(ClipSettings {
        triggers,
        ..ClipSettings::default()
    });
    let device = Device::with_mock(mock);
    let clip = device.clip();
    let status = clip.query_status().unwrap();
    assert_eq!(status.held.len(), 40);
    assert_eq!(status.ticks, 99);
    assert!(status.is_held(MediaKey::new(0x100)));
    assert!(!status.is_held(MediaKey::new(0x128)));
    assert_eq!(clip.query_config().unwrap().triggers.len(), 8);
}

#[test]
fn a_usage_snapshot_past_the_wire_cap_still_arrives() {
    // CTRL_USAGE_EVENT_MAX is 40.
    let mock = MockBox::new();
    let device = Device::with_mock(mock.clone());
    let s = device.catch_events([CatchFilter::everything()]).unwrap();
    mock.push_usages(
        0,
        1_000,
        crate::types::Class::Media,
        Direction::PRESS,
        &many_usages(256),
    );
    match s
        .recv_timeout(Duration::from_secs(1))
        .expect("the snapshot must arrive")
    {
        crate::CatchEvent::Usages(u) => {
            assert_eq!(u.usages.len(), 40);
            assert_eq!(u.usages[0], Usage::from(MediaKey::new(0x100)));
        }
        other => panic!("expected a usage snapshot, got {other:?}"),
    }
}

#[test]
fn a_traffic_packet_past_the_wire_cap_still_arrives() {
    // CTRL_TRAFFIC_DATA_MAX is 180; true_len still names the length before the cut.
    let mock = MockBox::new();
    let device = Device::with_mock(mock.clone());
    let s = device.catch_events([CatchFilter::everything()]).unwrap();
    let bytes: Vec<u8> = (0..600u16).map(|i| i as u8).collect();
    mock.push_traffic(
        0,
        1_000,
        ClockDomain::DeviceChip,
        CatchClass::Emit,
        0x81,
        Direction::IN,
        0,
        600,
        &bytes,
    );
    match s
        .recv_timeout(Duration::from_secs(1))
        .expect("the packet must arrive")
    {
        crate::CatchEvent::Traffic(t) => {
            assert_eq!(t.bytes.len(), 180);
            assert_eq!(t.bytes[..4], bytes[..4]);
            assert_eq!(t.true_len, 600);
            assert!(t.truncated());
        }
        other => panic!("expected traffic, got {other:?}"),
    }
}

#[test]
fn a_log_line_past_what_a_frame_carries_still_arrives() {
    // The protocol names no LOG text bound, so the frame's own is the one to hold: MAX_PAYLOAD less
    // the level byte.
    let mock = MockBox::new();
    let device = Device::with_mock(mock.clone());
    let rx = device.logs();
    mock.push_log(LogLevel::Warn, &"x".repeat(600));
    let line = rx
        .recv_timeout(Duration::from_secs(1))
        .expect("the line must arrive");
    assert_eq!(line.level, LogLevel::Warn);
    assert_eq!(line.text.len(), crate::protocol::opcode::MAX_PAYLOAD - 1);
}

#[test]
fn a_key_snapshot_at_the_cap_is_untouched() {
    // The caps truncate, they do not cut short what already fits: 40 usages is exactly the bound.
    let mock = MockBox::new();
    let device = Device::with_mock(mock.clone());
    let s = device.catch_events([CatchFilter::everything()]).unwrap();
    let usages: Vec<Usage> = (0..40u8).map(|i| Usage::from(Key::new(0x04 + i))).collect();
    mock.push_usages(
        0,
        1_000,
        crate::types::Class::Key,
        Direction::PRESS,
        &usages,
    );
    match s
        .recv_timeout(Duration::from_secs(1))
        .expect("the snapshot must arrive")
    {
        crate::CatchEvent::Usages(u) => assert_eq!(u.usages, usages),
        other => panic!("expected a usage snapshot, got {other:?}"),
    }
}
