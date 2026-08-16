//! `INJECT` key/media, `CAPS`, and keyboard/media catch events.

use crate::protocol::command::inject_payload;
use crate::protocol::opcode::{INJ_KEY, INJ_MEDIA};
use crate::types::{Key, MediaKey};

#[test]
fn key_inject_bytes() {
    assert_eq!(
        inject_payload(INJ_KEY, Key::A.usage() as u16, 1),
        [1, 0x04, 0x00, 1]
    );
    assert_eq!(
        inject_payload(INJ_KEY, Key::LEFT_SHIFT.usage() as u16, 1),
        [1, 0xE1, 0x00, 1]
    );
}

#[test]
fn media_inject_bytes() {
    assert_eq!(
        inject_payload(INJ_MEDIA, MediaKey::VOLUME_UP.usage(), 1),
        [2, 0xE9, 0x00, 1]
    );
}

#[test]
fn key_modifier_classification() {
    assert!(Key::LEFT_CTRL.is_modifier());
    assert!(Key::RIGHT_GUI.is_modifier());
    assert!(!Key::A.is_modifier());
    assert!(!Key::ENTER.is_modifier());
}

#[test]
fn kbd_caps_decodes() {
    use crate::Caps;
    let c = Caps::from_payload(&[3, 0, 0, 0, 0xFF, 0x0B, 0x02]).unwrap();
    let k = c.keyboard;
    assert_eq!(k.n_keys, 0xFF);
    assert!(k.nkro && k.has_consumer && k.has_report_id);
    assert!(!k.has_system);
    assert!(c.has_keyboard() && c.kbd_change_driven && !c.has_mouse());
    assert!(Caps::from_payload(&[3, 0]).is_none());
}

#[cfg(feature = "mock")]
#[test]
fn pushed_keyboard_and_media_events_arrive_on_the_stream() {
    use crate::{CatchClass, CatchEvent, CatchFilter, Device, Key, MediaKey, MockBox, Usage};
    use std::time::Duration;
    let mock = MockBox::new();
    let device = Device::with_mock(mock.clone());
    let stream = device
        .catch_events([
            CatchFilter::class(CatchClass::Key),
            CatchFilter::class(CatchClass::Media),
        ])
        .unwrap();

    mock.push_usages(
        0,
        1_000,
        &[
            Usage::from(Key::LEFT_SHIFT),
            Usage::from(Key::A),
            Usage::from(Key::B),
        ],
    );
    mock.push_usages(1, 2_000, &[Usage::from(MediaKey::VOLUME_UP)]);

    let CatchEvent::Usages(kb) = stream.recv_timeout(Duration::from_secs(1)).expect("keys") else {
        panic!("expected a usage event");
    };
    assert!(kb.is_held(Key::A));
    assert!(kb.is_held(Key::LEFT_SHIFT)); // modifiers are key usages 0xE0..0xE7
    assert!(!kb.is_held(Key::C));

    let CatchEvent::Usages(m) = stream.recv_timeout(Duration::from_secs(1)).expect("media") else {
        panic!("expected a usage event");
    };
    assert!(m.is_held(MediaKey::VOLUME_UP));
}
