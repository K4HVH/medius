//! OPTION command and QUERY(OPTIONS): payload bytes, `parse_resp` decoding, and query roundtrips, pinned to the `ctrl_proto.h` wire format.

use std::time::Duration;

#[cfg(feature = "mock")]
use crate::protocol::FrameType;
use crate::protocol::command::{
    emit_pace_payload, imperfect_payload, move_ride_payload, name_payload,
};
use crate::protocol::{Resp, parse_resp};
use crate::types::{EmitPace, EmitPaceStatus, ImperfectStatus, RenderMode};

#[test]
fn option_payload_bytes() {
    assert_eq!(imperfect_payload(true), [0, 1]);
    assert_eq!(imperfect_payload(false), [0, 0]);
    assert_eq!(move_ride_payload(5), [1, 5, 0]);
    assert_eq!(move_ride_payload(0), [1, 0, 0]);
    assert_eq!(move_ride_payload(1000), [1, 0xE8, 0x03]);
    assert_eq!(emit_pace_payload(0, 0, 0, 0), [2, 0, 0, 0, 0, 0, 0]);
    assert_eq!(emit_pace_payload(1, 0, 0, 0), [2, 1, 0, 0, 0, 0, 0]);
    assert_eq!(emit_pace_payload(2, 1000, 0, 0), [2, 2, 0xE8, 0x03, 0, 0, 0]);
    assert_eq!(emit_pace_payload(0, 0, 125, 0), [2, 0, 0, 0, 0x7D, 0, 0]);
    assert_eq!(
        emit_pace_payload(2, 500, 1000, 0),
        [2, 2, 0xF4, 0x01, 0xE8, 0x03, 0]
    );
    // The render byte is the value's own trailing field: it never touches the pace byte.
    assert_eq!(emit_pace_payload(0, 0, 0, 1), [2, 0, 0, 0, 0, 0, 1]);
    assert_eq!(emit_pace_payload(2, 500, 0, 3), [2, 2, 0xF4, 0x01, 0, 0, 3]);
    assert_eq!(name_payload("AB"), vec![3, b'A', b'B']);
    assert_eq!(name_payload(""), vec![3]);
}

#[test]
fn emit_pace_wire_is_the_pace_alone() {
    use crate::device::options::emit_pace_wire;
    assert_eq!(emit_pace_wire(EmitPace::Learned), (0, 0));
    assert_eq!(emit_pace_wire(EmitPace::Interval), (1, 0));
    assert_eq!(emit_pace_wire(EmitPace::Fixed(500)), (2, 500));
}

#[test]
fn render_mode_round_trips_its_own_wire_byte() {
    for (m, w) in [
        (RenderMode::Off, 0u8),
        (RenderMode::Stock, 1),
        (RenderMode::Despiked, 2),
        (RenderMode::Unsmoothed, 3),
    ] {
        assert_eq!(m.to_wire(), w);
        assert_eq!(RenderMode::from_wire(w), Some(m));
    }
    // A value this crate does not know is refused rather than silently read as Off.
    assert_eq!(RenderMode::from_wire(4), None);
    assert_eq!(RenderMode::from_wire(0x80), None);
}

#[test]
fn decode_imperfect_through_parse_resp() {
    let Some(Resp::Imperfect(i)) = parse_resp(&[9, 0, 1, 1, 1]) else {
        panic!("expected Imperfect");
    };
    assert_eq!(
        i,
        ImperfectStatus {
            allowed: true,
            over_capacity: true,
            clone_imperfect: true
        }
    );
    let Some(Resp::Imperfect(none)) = parse_resp(&[9, 0, 0, 0, 0]) else {
        panic!("expected Imperfect");
    };
    assert_eq!(none, ImperfectStatus::default());
    assert!(parse_resp(&[9, 0, 0, 0]).is_none());
}

#[test]
fn decode_move_ride_through_parse_resp() {
    let Some(Resp::MovementRiding(w)) = parse_resp(&[9, 1, 5, 0]) else {
        panic!("expected MovementRiding");
    };
    assert_eq!(w, Some(Duration::from_millis(5)));
    let Some(Resp::MovementRiding(off)) = parse_resp(&[9, 1, 0, 0]) else {
        panic!("expected MovementRiding");
    };
    assert_eq!(off, None);
    assert!(parse_resp(&[9, 1, 0]).is_none());
}

#[test]
fn decode_emit_pace_through_parse_resp() {
    // Five distinct numbers, so swapping any two fields fails.
    let Some(Resp::EmitPace(s)) =
        parse_resp(&[9, 2, 2, 0xE8, 0x03, 0xFA, 0x00, 0x7D, 0x00, 0x64, 0x00, 1, 0])
    else {
        panic!("expected EmitPace");
    };
    assert_eq!(
        s,
        EmitPaceStatus {
            mode: EmitPace::Fixed(1000),
            render: RenderMode::Off,
            resolved_hz: 250,
            force_hz: Some(125),
            advertised_hz: 100,
            force_active: true,
        }
    );
    // Unforced still reports what the clone advertises, so a host can see the device's own rate.
    let Some(Resp::EmitPace(native)) = parse_resp(&[9, 2, 0, 0, 0, 0, 0, 0, 0, 0xE8, 0x03, 0, 0])
    else {
        panic!("expected EmitPace");
    };
    assert_eq!(
        native,
        EmitPaceStatus {
            mode: EmitPace::Learned,
            render: RenderMode::Off,
            resolved_hz: 0,
            force_hz: None,
            advertised_hz: 1000,
            force_active: false,
        }
    );
    let Some(Resp::EmitPace(nothing)) = parse_resp(&[9, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]) else {
        panic!("expected EmitPace");
    };
    // Not EmitPaceStatus::default(): that carries the box's factory render mode (De-spiked), while an
    // all-zero payload is a box reporting the renderer explicitly off.
    assert_eq!(
        nothing,
        EmitPaceStatus {
            mode: EmitPace::Learned,
            render: RenderMode::Off,
            resolved_hz: 0,
            force_hz: None,
            advertised_hz: 0,
            force_active: false,
        }
    );
    // A 12-byte value is the pre-render shape and is refused rather than read with a defaulted field.
    assert!(parse_resp(&[9, 2, 2, 0xE8, 0x03, 0xFA, 0x00, 0x7D, 0x00, 0x64, 0x00, 1]).is_none());
    // The render field is the last byte and the pace byte is only ever 0/1/2, so the two decode
    // independently: every pace is read the same whatever the render byte beside it says.
    for (pace_byte, pace) in [(0u8, EmitPace::Learned), (1, EmitPace::Interval)] {
        for (render_byte, render) in [
            (0u8, RenderMode::Off),
            (1, RenderMode::Stock),
            (2, RenderMode::Despiked),
            (3, RenderMode::Unsmoothed),
        ] {
            let Some(Resp::EmitPace(s)) =
                parse_resp(&[9, 2, pace_byte, 0, 0, 0, 0, 0, 0, 0, 0, 0, render_byte])
            else {
                panic!("expected EmitPace");
            };
            assert_eq!((s.mode, s.render), (pace, render));
        }
    }
    // Fixed carries its rate whatever the render byte is.
    let Some(Resp::EmitPace(fixed_rendered)) =
        parse_resp(&[9, 2, 2, 0xFA, 0x00, 0xFA, 0x00, 0, 0, 0, 0, 0, 1])
    else {
        panic!("expected EmitPace");
    };
    assert_eq!(fixed_rendered.mode, EmitPace::Fixed(250));
    assert_eq!(fixed_rendered.render, RenderMode::Stock);
    assert_eq!(fixed_rendered.resolved_hz, 250);
    // An unknown value in either field is refused rather than silently read as a default.
    assert!(parse_resp(&[9, 2, 3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]).is_none());
    assert!(parse_resp(&[9, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4]).is_none());
    // The old encoding's mode bytes are now just unknown paces, not rendered anything.
    assert!(parse_resp(&[9, 2, 0x80, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]).is_none());
    assert!(parse_resp(&[9, 2, 0x82, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]).is_none());
}

#[cfg(feature = "mock")]
#[test]
fn mock_emit_pace_matches_firmware_snap() {
    use crate::{Device, EmitPace, EmitPaceStatus, MockBox, RenderMode};
    // The mock models firmware pacing: Fixed(400) snaps to 1000/3 = 333 Hz on the 1 ms frame clock
    // (not raw 400) and Fixed(2000) clamps to 1 kHz; a naive echo would diverge from hardware.
    // An untouched mock models a fresh box, which boots rendering De-spiked.
    let mock = MockBox::new().with_emit_pace(EmitPace::Fixed(400));
    let device = Device::with_mock(mock.clone());
    assert_eq!(
        device.query_emit_pace().unwrap(),
        EmitPaceStatus {
            mode: EmitPace::Fixed(400),
            render: RenderMode::Despiked,
            resolved_hz: 333,
            force_hz: None,
            advertised_hz: 0,
            force_active: false,
        }
    );
    mock.set_emit_pace(EmitPace::Fixed(2000));
    assert_eq!(
        device.query_emit_pace().unwrap(),
        EmitPaceStatus {
            mode: EmitPace::Fixed(1000),
            render: RenderMode::Despiked,
            resolved_hz: 1000,
            force_hz: None,
            advertised_hz: 0,
            force_active: false,
        }
    );
    // Render is its own field beside the pace, sent over the real command path. Onto Learned it forces
    // resolved to 1 kHz; the round-trip returns the same (pace, render).
    device.set_emit_pace(EmitPace::Learned, RenderMode::Stock, None).unwrap();
    assert_eq!(
        device.query_emit_pace().unwrap(),
        EmitPaceStatus {
            mode: EmitPace::Learned,
            render: RenderMode::Stock,
            resolved_hz: 1000,
            force_hz: None,
            advertised_hz: 0,
            force_active: false,
        }
    );
    // Onto a Fixed rate the snapped value stands rather than jumping to 1 kHz.
    device.set_emit_pace(EmitPace::Fixed(250), RenderMode::Stock, None).unwrap();
    assert_eq!(
        device.query_emit_pace().unwrap(),
        EmitPaceStatus {
            mode: EmitPace::Fixed(250),
            render: RenderMode::Stock,
            resolved_hz: 250,
            force_hz: None,
            advertised_hz: 0,
            force_active: false,
        }
    );
    // De-spiked and unsmoothed compose the same way and round-trip their smoother field.
    device.set_emit_pace(EmitPace::Learned, RenderMode::Despiked, None).unwrap();
    assert_eq!(device.query_emit_pace().unwrap().render, RenderMode::Despiked);
    device.set_emit_pace(EmitPace::Fixed(250), RenderMode::Unsmoothed, None).unwrap();
    let s = device.query_emit_pace().unwrap();
    assert_eq!((s.mode, s.render, s.resolved_hz), (EmitPace::Fixed(250), RenderMode::Unsmoothed, 250));
    device.set_emit_pace(EmitPace::Learned, RenderMode::Off, None).unwrap();
    assert_eq!(
        device.query_emit_pace().unwrap(),
        EmitPaceStatus {
            mode: EmitPace::Learned,
            render: RenderMode::Off,
            resolved_hz: 0,
            force_hz: None,
            advertised_hz: 0,
            force_active: false,
        }
    );
}

#[cfg(feature = "mock")]
#[test]
fn mock_rate_force_matches_firmware_snap() {
    use crate::{Device, MockBox};
    // A clone that declares 125 Hz, with the opt-in on so a force can actually apply.
    let mock = MockBox::new()
        .with_advertised_hz(125)
        .with_rate_force(Some(300));
    let device = Device::with_mock(mock.clone());
    device.allow_imperfect_clones(true).unwrap();
    // 300 Hz resolves to bInterval 2, the interval a host would actually keep for 1000/300.
    let s = device.query_emit_pace().unwrap();
    assert_eq!(s.force_hz, Some(300));
    assert_eq!(s.advertised_hz, 500);
    assert!(s.force_active);
    // Above the frame-clock ceiling resolves to bInterval 1.
    mock.set_rate_force(Some(4000));
    let s = device.query_emit_pace().unwrap();
    assert_eq!(s.force_hz, Some(4000));
    assert_eq!(s.advertised_hz, 1000);
    // 125 is exactly a power-of-two interval, so it is the one rate that comes back unchanged.
    mock.set_rate_force(Some(125));
    assert_eq!(device.query_emit_pace().unwrap().advertised_hz, 125);
    // The slowest a full-speed clone can advertise: bInterval 128, the largest power of two in a byte.
    mock.set_rate_force(Some(1));
    assert_eq!(device.query_emit_pace().unwrap().advertised_hz, 7);
    // Some(0) is what the wire calls off, and dividing by it would panic.
    mock.set_rate_force(Some(0));
    let s = device.query_emit_pace().unwrap();
    assert_eq!(s.force_hz, None);
    assert!(!s.force_active);
    mock.set_rate_force(None);
    let s = device.query_emit_pace().unwrap();
    assert_eq!(s.force_hz, None);
    assert!(!s.force_active);
    assert_eq!(s.advertised_hz, 125);
}

#[cfg(feature = "mock")]
#[test]
fn mock_leaves_a_force_inert_without_the_opt_in() {
    use crate::{Device, EmitPace, MockBox};
    // The box gates a force on the imperfect opt-in, so a mock that reported it active regardless would
    // green-light host code that disagrees with every real box.
    let mock = MockBox::new().with_advertised_hz(125);
    let device = Device::with_mock(mock.clone());
    device
        .set_emit_pace(EmitPace::Learned, RenderMode::Off, Some(1000))
        .unwrap();
    let s = device.query_emit_pace().unwrap();
    assert_eq!(s.force_hz, Some(1000));
    assert!(!s.force_active);
    assert_eq!(s.advertised_hz, 125);
    device.allow_imperfect_clones(true).unwrap();
    let s = device.query_emit_pace().unwrap();
    assert!(s.force_active);
    assert_eq!(s.advertised_hz, 1000);
}

#[test]
fn unknown_option_id_and_missing_id_are_none() {
    assert!(parse_resp(&[9, 0xFF, 0, 0]).is_none());
    assert!(parse_resp(&[9]).is_none());
}

#[cfg(feature = "mock")]
#[test]
fn set_movement_riding_rounds_sub_ms_up_to_on() {
    use crate::{Device, MockBox};
    let mock = MockBox::new();
    let device = Device::with_mock(mock.clone());
    // a non-zero Some window under 1 ms must not silently round down to off
    device
        .set_movement_riding(Some(Duration::from_micros(500)))
        .unwrap();
    let frame = mock
        .recorded_frames()
        .into_iter()
        .find(|f| f.ty == FrameType::Option)
        .unwrap();
    assert_eq!(frame.payload, vec![1, 1, 0]);
}

#[cfg(feature = "mock")]
#[test]
fn set_movement_riding_saturates_at_u16_max() {
    use crate::{Device, MockBox};
    let mock = MockBox::new();
    let device = Device::with_mock(mock.clone());
    // a window past u16::MAX ms must saturate, not wrap
    device
        .set_movement_riding(Some(Duration::from_millis(100_000)))
        .unwrap();
    let frame = mock
        .recorded_frames()
        .into_iter()
        .find(|f| f.ty == FrameType::Option)
        .unwrap();
    assert_eq!(frame.payload, vec![1, 0xFF, 0xFF]);
}

#[cfg(feature = "mock")]
#[test]
fn set_name_sends_option_frame_and_clear_is_empty() {
    use crate::{Device, MockBox};
    let mock = MockBox::new();
    let device = Device::with_mock(mock.clone());
    device.set_name("Left PC").unwrap();
    let frame = mock
        .recorded_frames()
        .into_iter()
        .find(|f| f.ty == FrameType::Option)
        .unwrap();
    assert_eq!(frame.payload, b"\x03Left PC".to_vec());
    device.clear_name().unwrap();
    let cleared = mock
        .recorded_frames()
        .into_iter()
        .rfind(|f| f.ty == FrameType::Option)
        .unwrap();
    assert_eq!(cleared.payload, vec![3]);
}

#[cfg(feature = "mock")]
#[test]
fn set_name_sends_the_value_raw_for_the_box_to_sanitize() {
    use crate::{Device, MockBox};
    // set_name does no host-side validation: it sends the string as-is and the box sanitizes, so a
    // control byte rides through on the wire (the box drops it) rather than raising a host error.
    let mock = MockBox::new();
    let device = Device::with_mock(mock.clone());
    device.set_name("A\tB").unwrap();
    let frame = mock
        .recorded_frames()
        .into_iter()
        .find(|f| f.ty == FrameType::Option)
        .unwrap();
    assert_eq!(frame.payload, vec![3, b'A', b'\t', b'B']);
}

#[test]
fn bearing_payload_bytes() {
    use crate::protocol::command::bearing_payload;
    use crate::protocol::opcode::{BEARING_PER_AXIS, BEARING_VECTOR};
    assert_eq!(bearing_payload(20, BEARING_PER_AXIS), [4, 20, 0, 0]);
    assert_eq!(bearing_payload(500, BEARING_VECTOR), [4, 0xF4, 0x01, 1]);
    assert_eq!(bearing_payload(0, BEARING_PER_AXIS), [4, 0, 0, 0]);
}

#[test]
fn bearing_decodes_from_a_resp() {
    use crate::protocol::{Resp, parse_resp};
    use crate::types::BearingMode;
    use std::time::Duration;
    let Some(Resp::Bearing(b)) = parse_resp(&[9, 4, 20, 0, 0]) else {
        panic!("expected Bearing");
    };
    assert_eq!(b.window, Some(Duration::from_millis(20)));
    assert_eq!(b.mode, BearingMode::PerAxis);
    assert!(b.is_live());

    // A zero window is off, not a zero-length one: the relative directions do nothing at all.
    let Some(Resp::Bearing(off)) = parse_resp(&[9, 4, 0, 0, 1]) else {
        panic!("expected Bearing");
    };
    assert_eq!(off.window, None);
    assert_eq!(off.mode, BearingMode::Vector);
    assert!(!off.is_live());

    // An unknown mode is not silently read as per-axis.
    assert!(parse_resp(&[9, 4, 20, 0, 7]).is_none());
    assert!(parse_resp(&[9, 4, 20, 0]).is_none());
}

#[cfg(feature = "mock")]
#[test]
fn set_bearing_sends_the_option_frame() {
    use crate::types::BearingMode;
    use crate::{Device, MockBox};
    use std::time::Duration;
    let mock = MockBox::new();
    let device = Device::with_mock(mock.clone());
    device
        .set_bearing(Some(Duration::from_millis(20)), BearingMode::Vector)
        .unwrap();
    let frame = mock
        .recorded_frames()
        .into_iter()
        .find(|f| f.ty == FrameType::Option)
        .unwrap();
    assert_eq!(frame.payload, vec![4, 20, 0, 1]);

    device.set_bearing(None, BearingMode::PerAxis).unwrap();
    let off = mock
        .recorded_frames()
        .into_iter()
        .rfind(|f| f.ty == FrameType::Option)
        .unwrap();
    assert_eq!(off.payload, vec![4, 0, 0, 0]);
}

#[cfg(feature = "mock")]
#[test]
fn query_bearing_round_trips_through_the_mock() {
    use crate::types::{Bearing, BearingMode};
    use crate::{Device, MockBox};
    use std::time::Duration;
    let want = Bearing {
        window: Some(Duration::from_millis(35)),
        mode: BearingMode::Vector,
    };
    let mock = MockBox::new().with_bearing(want);
    let device = Device::with_mock(mock);
    assert_eq!(device.query_bearing().unwrap(), want);
}

#[cfg(feature = "mock")]
#[test]
fn scale_sends_the_lock_frame_with_the_percentage() {
    use crate::{Axis, Device, Direction, MockBox};
    let mock = MockBox::new();
    let device = Device::with_mock(mock.clone());
    device.scale(Axis::X, Direction::Against, 40).unwrap();
    device.scale(Axis::Y, Direction::With, 130).unwrap();
    let frames: Vec<Vec<u8>> = mock
        .recorded_frames()
        .into_iter()
        .filter(|f| f.ty == FrameType::Lock)
        .map(|f| f.payload)
        .collect();
    assert_eq!(frames, vec![vec![3, 0, 0, 4, 40], vec![3, 1, 0, 3, 130]]);
}

#[cfg(feature = "mock")]
#[test]
fn lock_and_unlock_are_the_two_ends_of_the_scale() {
    use crate::{Axis, Device, Direction, MockBox};
    let mock = MockBox::new();
    let device = Device::with_mock(mock.clone());
    device.lock(Axis::X, Direction::Positive).unwrap();
    device.unlock(Axis::X, Direction::Positive).unwrap();
    let frames: Vec<Vec<u8>> = mock
        .recorded_frames()
        .into_iter()
        .filter(|f| f.ty == FrameType::Lock)
        .map(|f| f.payload)
        .collect();
    assert_eq!(frames, vec![vec![3, 0, 0, 1, 0], vec![3, 0, 0, 1, 100]]);
}

#[cfg(feature = "mock")]
#[test]
fn a_relative_direction_is_refused_on_a_catch_subscription() {
    use crate::{CatchFilter, Device, Direction, Error, MockBox, TrafficClass};
    let device = Device::with_mock(MockBox::new());
    let err = device
        .catch_events([
            CatchFilter::traffic(TrafficClass::VendorBulk, 0x83).with_direction(Direction::Against)
        ])
        .unwrap_err();
    assert!(
        matches!(
            err,
            Error::RelativeDirection {
                direction: Direction::Against,
                ..
            }
        ),
        "expected RelativeDirection, got {err:?}"
    );
}
