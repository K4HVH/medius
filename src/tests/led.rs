//! LED command (§3.7): payload bytes, enum wire values, and that `Device::led` sends a `LED` frame.

use crate::protocol::FrameType;
use crate::protocol::command::led_payload;
use crate::types::{LedMode, LedTarget};

#[test]
fn led_payload_bytes() {
    assert_eq!(led_payload(2, 3, 128), [2, 3, 128]);
}

#[test]
fn led_enums_pin_wire_values_and_roundtrip() {
    // Wire values pinned to ctrl_proto.h CTRL_LED_TGT_* / CTRL_LED_MODE_*.
    assert_eq!(
        (
            LedTarget::Device.as_u8(),
            LedTarget::Host.as_u8(),
            LedTarget::Both.as_u8()
        ),
        (0, 1, 2)
    );
    assert_eq!(
        (
            LedMode::Auto.as_u8(),
            LedMode::Off.as_u8(),
            LedMode::Solid.as_u8(),
            LedMode::Blink.as_u8()
        ),
        (0, 1, 2, 3)
    );
    for t in [LedTarget::Device, LedTarget::Host, LedTarget::Both] {
        assert_eq!(LedTarget::from_u8(t.as_u8()), Some(t));
    }
    assert_eq!(LedTarget::from_u8(3), None);
    for m in [LedMode::Auto, LedMode::Off, LedMode::Solid, LedMode::Blink] {
        assert_eq!(LedMode::from_u8(m.as_u8()), Some(m));
    }
    assert_eq!(LedMode::from_u8(4), None);
}

#[cfg(feature = "mock")]
#[test]
fn led_sends_a_led_frame() {
    use crate::{Device, MockBox};
    let mock = MockBox::new();
    let device = Device::with_mock(mock.clone());
    device.led(LedTarget::Both, LedMode::Blink, 128).unwrap();
    let frames = mock.recorded_frames();
    let led = frames
        .iter()
        .find(|f| f.ty == FrameType::Led)
        .expect("a LED frame was recorded");
    assert_eq!(led.payload, vec![2, 3, 128]);
}
