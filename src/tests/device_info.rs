//! Device-info request decoding (§4.3–4.6) + the HEALTH `rate_confident` bit (§4.2).
//!
//! The byte vectors here are the EXACT wire bytes the firmware emits (mirrored from the firmware's
//! own packer test, medius-fw `tests/host/test_ctrl_proto.c`), so these tests pin the decoder to the
//! firmware wire format, not merely to our own encoder.

use crate::protocol::{Resp, parse_resp};
use crate::types::Health;

#[test]
fn rate_decodes_continuous_vs_change_driven() {
    // [what=4][native u16][poll u16][flags]. Continuous mouse: 1000us, confident, not change-driven.
    let Some(Resp::Rate(r)) = parse_resp(&[4, 0xE8, 0x03, 0xE8, 0x03, 0x01]) else {
        panic!("expected Rate");
    };
    assert_eq!(r.native_period_us, 1000);
    assert!(r.confident && !r.change_driven);
    assert_eq!(r.native_hz(), Some(1000.0));
    // Change-driven keyboard: native N/A (0), poll floor 1000us, CHANGE_DRIVEN set.
    let Some(Resp::Rate(k)) = parse_resp(&[4, 0x00, 0x00, 0xE8, 0x03, 0x02]) else {
        panic!("expected Rate");
    };
    assert_eq!(k.native_period_us, 0);
    assert!(k.change_driven && !k.confident);
    assert_eq!(k.native_hz(), None);
    assert_eq!(k.poll_period_us, 1000);
}

#[test]
fn decode_mouse_info_exact_bytes() {
    // vid 0x046D, pid 0xC08B, bcdDevice 0x0110, bcdUSB 0x0200, flags HAS_SERIAL|HAS_BOS
    let p = [2u8, 0x6D, 0x04, 0x8B, 0xC0, 0x10, 0x01, 0x00, 0x02, 0x03];
    let Some(Resp::MouseInfo(m)) = parse_resp(&p) else {
        panic!("expected MouseInfo");
    };
    assert_eq!(m.vid, 0x046D);
    assert_eq!(m.pid, 0xC08B);
    assert_eq!(m.bcd_device, 0x0110);
    assert_eq!(m.bcd_usb, 0x0200);
    assert!(m.has_serial && m.has_bos);
    assert_eq!(format!("{m}"), "046D:C08B");
}

#[test]
fn decode_caps_exact_bytes() {
    // unified CAPS: 5 buttons, X|Y|WHEEL (0x07), 2 HID interfaces; no keyboard; not change-driven
    let p = [3u8, 5, 0x07, 2, 0, 0, 0];
    let Some(Resp::Caps(c)) = parse_resp(&p) else {
        panic!("expected Caps");
    };
    assert_eq!(c.mouse.n_buttons, 5);
    assert!(c.mouse.has_x && c.mouse.has_y && c.mouse.has_wheel);
    assert!(!c.mouse.has_report_id);
    assert_eq!(c.mouse.n_hid, 2);
    assert!(c.is_composite());
    assert!(c.has_mouse() && !c.has_keyboard());
    assert!(!c.mouse_change_driven && !c.kbd_change_driven);
}

#[test]
fn decode_rate_exact_bytes() {
    // native 1000us, poll 1000us, CONFIDENT
    let p = [4u8, 0xE8, 0x03, 0xE8, 0x03, 0x01];
    let Some(Resp::Rate(r)) = parse_resp(&p) else {
        panic!("expected Rate");
    };
    assert_eq!(r.native_period_us, 1000);
    assert_eq!(r.poll_period_us, 1000);
    assert!(r.confident);
    assert_eq!(r.native_hz(), Some(1000.0));
}

#[test]
fn rate_unlearned_period_is_none() {
    let p = [4u8, 0x00, 0x00, 0xE8, 0x03, 0x00];
    let Some(Resp::Rate(r)) = parse_resp(&p) else {
        panic!("expected Rate");
    };
    assert_eq!(r.native_period_us, 0);
    assert_eq!(r.native_hz(), None); // truthful: 0 means "not learned yet"
    assert!(!r.confident);
}

#[test]
fn decode_stats_exact_bytes_with_saturation() {
    // Same vector as the firmware packer test: tx_drops/wakeups saturated to 0xFFFF, maxdepth to 0xFF.
    let p = [
        5u8, 0x04, 0x03, 0x02, 0x01, 0xFF, 0xFF, 0x0A, 0x00, 0xFF, 0x02, 0xFF, 0xFF, 0x07, 0x00,
        0x09, 0x00,
    ];
    let Some(Resp::Stats(s)) = parse_resp(&p) else {
        panic!("expected Stats");
    };
    assert_eq!(s.inject_emits, 0x0102_0304);
    assert_eq!(s.tx_drops, 0xFFFF);
    assert_eq!(s.tx_merges, 10);
    assert_eq!(s.tx_maxdepth, 0xFF);
    assert_eq!(s.tx_wedges, 2);
    assert_eq!(s.wakeups, 0xFFFF);
    assert_eq!(s.reset_count, 7);
    assert_eq!(s.config_count, 9);
}

#[test]
fn health_rate_confident_bit_roundtrips() {
    let h = Health::from_flags(0x10);
    assert!(h.rate_confident);
    assert!(!h.link_up && !h.mouse_attached && !h.clone_configured && !h.injection_active);
    assert_eq!(h.to_flags(), 0x10);
    // and it survives a full round-trip with the other bits set
    assert_eq!(Health::from_flags(0x1F).to_flags(), 0x1F);
}

#[test]
fn truncated_payloads_decode_to_none() {
    assert!(parse_resp(&[2, 0, 0]).is_none()); // MOUSE_INFO needs 10
    assert!(parse_resp(&[3, 5]).is_none()); // CAPS needs 4
    assert!(parse_resp(&[4, 0xE8, 0x03]).is_none()); // RATE needs 6
    assert!(parse_resp(&[5, 0, 0, 0]).is_none()); // STATS needs 17
}

#[cfg(feature = "mock")]
#[test]
fn device_queries_roundtrip_through_mock() {
    use crate::types::{MouseCaps, MouseInfo, Rate, Stats};
    use crate::{Device, MockBox};

    let mouse = MouseInfo {
        vid: 0x046D,
        pid: 0xC08B,
        bcd_device: 0x0110,
        bcd_usb: 0x0201,
        has_serial: true,
        has_bos: true,
    };
    let caps = MouseCaps {
        n_buttons: 5,
        has_x: true,
        has_y: true,
        has_wheel: true,
        has_report_id: false,
        n_hid: 2,
    };
    let rate = Rate {
        native_period_us: 1000,
        poll_period_us: 1000,
        confident: true,
        change_driven: false,
    };
    let stats = Stats {
        inject_emits: 1234,
        tx_drops: 0,
        tx_merges: 7,
        tx_maxdepth: 3,
        tx_wedges: 0,
        wakeups: 1,
        reset_count: 2,
        config_count: 4,
    };

    let mock = MockBox::new()
        .with_mouse_info(mouse)
        .with_mouse_caps(caps)
        .with_rate(rate)
        .with_stats(stats);
    let device = Device::with_mock(mock);

    assert_eq!(device.query_mouse_info().unwrap(), mouse);
    assert_eq!(device.caps().unwrap().mouse, caps);
    assert_eq!(device.query_rate().unwrap(), rate);
    assert_eq!(device.query_stats().unwrap(), stats);
}
