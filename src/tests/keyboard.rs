//! `INJECT` key/media (§3.2), `CAPS` (§4.4), and keyboard/media catch events.
//! Bytes are pinned to the firmware wire format in `ctrl_proto.h`.

use crate::protocol::command::inject_payload;
use crate::protocol::opcode::{INJ_KEY, INJ_MEDIA};
use crate::types::{Key, MediaKey};

#[test]
fn key_inject_bytes() {
    // INJECT [class=key][id u16 LE][action]: 'a' press, LeftShift modifier press.
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
    // INJECT [class=media][id u16 LE][action]: Vol+ = 0x00E9, press.
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
    // unified CAPS, keyboard half: n_keys=255 (NKRO bitmap), kbd_flags = NKRO|CONSUMER|REPORT_ID,
    // keyboard class change-driven
    let c = Caps::from_payload(&[3, 0, 0, 0, 0xFF, 0x0B, 0x02]).unwrap();
    let k = c.keyboard;
    assert_eq!(k.n_keys, 0xFF);
    assert!(k.nkro && k.has_consumer && k.has_report_id);
    assert!(!k.has_system);
    assert!(c.has_keyboard() && c.kbd_change_driven && !c.has_mouse());
    assert!(Caps::from_payload(&[3, 0]).is_none()); // needs 7
}

#[cfg(feature = "mock")]
#[test]
fn pushed_keyboard_and_media_events_arrive_on_the_stream() {
    use crate::{CatchEvent, CatchMask, Device, Key, KeyboardEvent, MediaEvent, MediaKey, MockBox};
    use std::time::Duration;
    let mock = MockBox::new();
    let device = Device::with_mock(mock.clone());
    let stream = device.catch_events(CatchMask::KEYS).unwrap();

    mock.push_kb_event(
        0,
        &KeyboardEvent {
            modifiers: 0x02, // LeftShift
            keys: vec![Key::A, Key::B],
        },
    );
    mock.push_cons_event(
        1,
        &MediaEvent {
            keys: vec![MediaKey::VOLUME_UP],
        },
    );

    let e1 = stream
        .recv_timeout(Duration::from_secs(1))
        .expect("a keyboard event arrived");
    let CatchEvent::Keyboard(kb) = e1 else {
        panic!("expected a keyboard event, got {e1:?}");
    };
    assert_eq!(kb.modifiers, 0x02);
    assert!(kb.is_pressed(Key::A));
    assert!(kb.is_pressed(Key::LEFT_SHIFT)); // read from the modifier bitmap
    assert!(!kb.is_pressed(Key::C));

    let e2 = stream
        .recv_timeout(Duration::from_secs(1))
        .expect("a media event arrived");
    let CatchEvent::Media(m) = e2 else {
        panic!("expected a media event, got {e2:?}");
    };
    assert!(m.is_pressed(MediaKey::VOLUME_UP));
}
