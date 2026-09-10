//! `TRANSFORM` (§3.15): the payload bytes, the op/field vocabulary, `RESP(TRANSFORMS)` decode (no
//! per-entry state byte), and the MockBox round-trip through the whole table lifecycle.

use crate::protocol::command::transform_payload;
use crate::protocol::{Resp, parse_resp};
use crate::types::{
    Axis, Button, Class, Key, MediaKey, Transform, TransformField, TransformOp, Usage,
};

#[test]
fn transform_payload_bytes() {
    // SCALE the wheel (axis class 3, id 2) by +200%, state add.
    let p = transform_payload(3, 3, 2, 3, 2, 200, 1);
    assert_eq!(p, [3, 3, 0x02, 0x00, 3, 0x02, 0x00, 0xC8, 0x00, 1]);
}

#[test]
fn transform_payload_negative_scale_and_remap() {
    // REMAP X (axis 0) → Y (axis 1) with scale -100: the sign must survive as an i16 low+high byte.
    let p = transform_payload(0, 3, 0, 3, 1, -100, 1);
    assert_eq!(p, [0, 3, 0x00, 0x00, 3, 0x01, 0x00, 0x9C, 0xFF, 1]);
}

#[test]
fn transform_clear_sentinel_bytes() {
    // The whole-table clear: op ignored, both classes 0xFF, both ids 0xFFFF, state 0.
    let p = transform_payload(0, 0xFF, 0xFFFF, 0xFF, 0xFFFF, 0, 0);
    assert_eq!(p, [0, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00, 0x00, 0x00]);
}

#[test]
fn resp_transforms_decode() {
    // [16][flags 0][n 1] then one entry: Scale, wheel→wheel, scale 200. No state byte per entry.
    let p = [16, 0, 1, 3, 3, 0x02, 0x00, 3, 0x02, 0x00, 0xC8, 0x00];
    let Some(Resp::Transforms(t)) = parse_resp(&p) else {
        panic!("not a RESP(TRANSFORMS)");
    };
    assert!(!t.table_full);
    assert_eq!(t.entries.len(), 1);
    let e = t.entries[0];
    assert_eq!(e.op, TransformOp::Scale);
    assert_eq!(e.source, TransformField::Axis(Axis::Wheel));
    assert_eq!(e.dest, TransformField::Axis(Axis::Wheel));
    assert_eq!(e.scale, 200);
}

#[test]
fn resp_transforms_high_bytes_and_full_flag() {
    // Hand-computed: table-full flag set, one cross-class remap Button(6) → Media(0x0233) with scale
    // -50. A u8 truncation of `did` (0x0233 → 0x33), or a sign error on the scale, fails here.
    let p = [16, 0x01, 1, 0, 0x00, 0x06, 0x00, 2, 0x33, 0x02, 0xCE, 0xFF];
    let Some(Resp::Transforms(t)) = parse_resp(&p) else {
        panic!("not a RESP(TRANSFORMS)");
    };
    assert!(t.table_full);
    assert_eq!(t.entries.len(), 1);
    let e = t.entries[0];
    assert_eq!(e.op, TransformOp::Remap);
    assert_eq!(
        e.source,
        TransformField::Usage(Usage::new(Class::Button, 6))
    );
    assert_eq!(
        e.dest,
        TransformField::Usage(Usage::new(Class::Media, 0x0233))
    );
    assert_eq!(e.scale, -50);
}

#[test]
fn resp_transforms_empty() {
    let Some(Resp::Transforms(t)) = parse_resp(&[16, 0x00, 0]) else {
        panic!("not a RESP(TRANSFORMS)");
    };
    assert!(!t.table_full);
    assert!(t.entries.is_empty());
}

#[test]
fn transform_op_wire() {
    assert_eq!(TransformOp::Remap.as_u8(), 0);
    assert_eq!(TransformOp::Swap.as_u8(), 1);
    assert_eq!(TransformOp::Invert.as_u8(), 2);
    assert_eq!(TransformOp::Scale.as_u8(), 3);
    assert_eq!(TransformOp::from_u8(0), Some(TransformOp::Remap));
    assert_eq!(TransformOp::from_u8(3), Some(TransformOp::Scale));
    assert_eq!(TransformOp::from_u8(4), None);
}

#[test]
fn transform_field_class_id_roundtrips() {
    // Axis carries class 3; a usage carries INJECT's class byte.
    assert_eq!(TransformField::Axis(Axis::X).class_id(), (3, 0));
    assert_eq!(TransformField::Axis(Axis::Pan).class_id(), (3, 3));
    assert_eq!(TransformField::from(Button::new(6)).class_id(), (0, 6));
    assert_eq!(
        TransformField::from(Usage::new(Class::Media, 0x0233)).class_id(),
        (2, 0x0233)
    );

    for (cls, id) in [(3, 0), (3, 3), (0, 6), (1, 0x04), (2, 0x0233)] {
        let f = TransformField::from_class_id(cls, id).unwrap();
        assert_eq!(f.class_id(), (cls, id));
    }
    // An axis id past pan, and a class no field names, decode to nothing.
    assert_eq!(TransformField::from_class_id(3, 4), None);
    assert_eq!(TransformField::from_class_id(0x0A, 0), None);

    // as_axis pulls the axis back out, and is None for a momentary usage.
    assert_eq!(TransformField::Axis(Axis::X).as_axis(), Some(Axis::X));
    assert_eq!(TransformField::from(Button::new(6)).as_axis(), None);
}

#[test]
fn transform_key_identifies_the_entry() {
    // Two transforms with the same (source, dest) share a key (the box overwrites), and one that
    // differs in either field does not.
    let a = Transform::invert(Axis::Y);
    let b = Transform::scale_axis(Axis::Y, 200);
    let c = Transform::invert(Axis::X);
    assert_eq!(a.key(), b.key());
    assert_ne!(a.key(), c.key());
    assert_eq!(a.key().source, TransformField::Axis(Axis::Y));
    assert_eq!(a.key().dest, TransformField::Axis(Axis::Y));
}

#[test]
fn transform_constructors_shape_the_fields() {
    let inv = Transform::invert(Axis::Y);
    assert_eq!(inv.op, TransformOp::Invert);
    assert_eq!(inv.source, TransformField::Axis(Axis::Y));
    assert_eq!(inv.dest, TransformField::Axis(Axis::Y));
    assert_ne!(
        inv.scale, 0,
        "invert must carry a non-zero scale the box accepts"
    );

    assert_eq!(Transform::scale_axis(Axis::Wheel, 200).scale, 200);

    let sw = Transform::swap(Axis::X, Axis::Y);
    assert_eq!(sw.op, TransformOp::Swap);
    assert_eq!(sw.source, TransformField::Axis(Axis::X));
    assert_eq!(sw.dest, TransformField::Axis(Axis::Y));

    let rm = Transform::remap(Button::SIDE1, Key::A);
    assert_eq!(rm.op, TransformOp::Remap);
    assert_eq!(rm.source, TransformField::from(Button::SIDE1));
    assert_eq!(rm.dest, TransformField::from(Key::A));

    assert_eq!(
        Transform::swap(Axis::X, Axis::Y).with_scale(-100).scale,
        -100
    );
}

#[test]
fn transform_op_admits_mirrors_the_box() {
    use TransformField::Axis as A;
    let x = A(Axis::X);
    let y = A(Axis::Y);
    let btn = TransformField::from(Button::new(6));
    let key = TransformField::from(Key::A);
    let media = TransformField::from(MediaKey::VOLUME_UP);

    // Invert/Scale: one axis (source == dest).
    assert!(TransformOp::Invert.admits(x, x));
    assert!(!TransformOp::Invert.admits(x, y));
    assert!(!TransformOp::Scale.admits(x, btn));
    // Swap: two axes.
    assert!(TransformOp::Swap.admits(x, y));
    assert!(!TransformOp::Swap.admits(x, btn));
    // Remap: axis→axis, button→button, button→key, button→media; nothing else.
    assert!(TransformOp::Remap.admits(x, y));
    assert!(TransformOp::Remap.admits(btn, btn));
    assert!(TransformOp::Remap.admits(btn, key));
    assert!(TransformOp::Remap.admits(btn, media));
    assert!(!TransformOp::Remap.admits(x, btn));
    assert!(!TransformOp::Remap.admits(key, key));
}

#[cfg(feature = "mock")]
mod mock_roundtrip {
    use crate::error::Error;
    use crate::types::{Axis, Button, Key, MouseCaps, Transform, TransformField, TransformOp};
    use crate::{Device, FrameType, MockBox};

    #[test]
    fn set_query_and_clear_roundtrip() {
        let device = Device::with_mock(MockBox::new());
        device.transform(&Transform::invert(Axis::Y)).unwrap();

        let table = device.query_transforms().unwrap();
        assert_eq!(table.entries.len(), 1);
        assert_eq!(table.entries[0].op, TransformOp::Invert);
        assert_eq!(table.entries[0].source, TransformField::Axis(Axis::Y));
        assert!(device.query_health().unwrap().transform_on);

        device.clear_transforms().unwrap();
        assert!(device.query_transforms().unwrap().entries.is_empty());
        assert!(!device.query_health().unwrap().transform_on);
    }

    #[test]
    fn transform_is_ungated_no_opt_in_needed() {
        // The default mock has the imperfect opt-in OFF; a transform still installs, because it is
        // faithful and never needed the gate the rewrite/raw/patch layer does.
        let device = Device::with_mock(MockBox::new());
        assert!(!device.query_imperfect().unwrap().allowed);
        device.invert(Axis::Y).unwrap();
        assert_eq!(device.query_transforms().unwrap().entries.len(), 1);
    }

    #[test]
    fn ergonomic_helpers_install_the_right_entries() {
        let caps = MouseCaps {
            n_buttons: 5,
            has_x: true,
            has_y: true,
            has_wheel: true,
            pan: true,
            has_report_id: false,
            n_hid: 1,
        };
        let device = Device::with_mock(MockBox::new().with_mouse_caps(caps));
        device.scale_transform(Axis::Wheel, 200).unwrap();
        device.swap(Axis::X, Axis::Y).unwrap();

        let entries = device.query_transforms().unwrap().entries;
        assert_eq!(entries.len(), 2);
        assert!(entries.iter().any(|e| e.op == TransformOp::Scale
            && e.source == TransformField::Axis(Axis::Wheel)
            && e.scale == 200));
        assert!(entries.iter().any(|e| e.op == TransformOp::Swap
            && e.source == TransformField::Axis(Axis::X)
            && e.dest == TransformField::Axis(Axis::Y)));
    }

    #[test]
    fn same_key_overwrites_op_and_scale() {
        // invert(Y) and scale_axis(Y) share the key (Y, Y): the second overwrites, not appends.
        let device = Device::with_mock(MockBox::new());
        device.invert(Axis::Y).unwrap();
        device.scale_transform(Axis::Y, 200).unwrap();
        let entries = device.query_transforms().unwrap().entries;
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].op, TransformOp::Scale);
        assert_eq!(entries[0].scale, 200);
    }

    #[test]
    fn untransform_drops_one_entry_by_key() {
        let device = Device::with_mock(MockBox::new());
        device.invert(Axis::Y).unwrap();
        device.scale_transform(Axis::Wheel, 150).unwrap();
        assert_eq!(device.query_transforms().unwrap().entries.len(), 2);
        device.untransform(&Transform::invert(Axis::Y)).unwrap();
        let entries = device.query_transforms().unwrap().entries;
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].source, TransformField::Axis(Axis::Wheel));
    }

    #[test]
    fn op_class_pair_is_rejected_before_the_wire() {
        let mock = MockBox::new();
        let device = Device::with_mock(mock.clone());
        // Invert across two different axes is not an op any class pair takes.
        let bad = Transform::new(TransformOp::Invert, Axis::X, Axis::Y, 100);
        assert!(matches!(
            device.transform(&bad),
            Err(Error::TransformOpFields { .. })
        ));
        // A remap of an axis onto a button has no mechanism.
        let bad = Transform::remap(Axis::X, Button::LEFT);
        assert!(matches!(
            device.transform(&bad),
            Err(Error::TransformOpFields { .. })
        ));
        assert!(
            !mock.saw(FrameType::Transform),
            "a refused op never reaches the wire"
        );
    }

    #[test]
    fn zero_scale_on_invert_is_rejected_before_the_wire() {
        let mock = MockBox::new();
        let device = Device::with_mock(mock.clone());
        let bad = Transform::invert(Axis::Y).with_scale(0);
        assert!(matches!(
            device.transform(&bad),
            Err(Error::TransformInvertZeroScale)
        ));
        assert!(!mock.saw(FrameType::Transform));
    }

    #[test]
    fn an_undeclared_field_is_refused_by_the_box_not_the_crate() {
        // The default mock has no AC Pan and no keyboard, so the crate sends these (they are
        // structurally valid) and the box refuses them: absent from the readback, the frame still went.
        let mock = MockBox::new();
        let device = Device::with_mock(mock.clone());
        device.invert(Axis::Pan).unwrap(); // pan not declared
        device.remap(Button::LEFT, Key::A).unwrap(); // no keyboard collection
        assert!(
            mock.saw(FrameType::Transform),
            "structurally-valid entries reach the wire"
        );
        assert!(
            device.query_transforms().unwrap().entries.is_empty(),
            "the box declares neither field, so it holds neither entry"
        );
        assert!(!device.query_health().unwrap().transform_on);
    }

    #[test]
    fn the_table_full_flag_is_reported() {
        let caps = MouseCaps {
            n_buttons: 16,
            has_x: true,
            has_y: true,
            has_wheel: true,
            pan: false,
            has_report_id: false,
            n_hid: 1,
        };
        let device = Device::with_mock(MockBox::new().with_mouse_caps(caps));
        // Eight distinct button→button remaps fill the table (the box holds eight).
        for i in 0..8u8 {
            device
                .transform(&Transform::remap(Button::new(i), Button::new(i + 1)))
                .unwrap();
        }
        let table = device.query_transforms().unwrap();
        assert_eq!(table.entries.len(), 8);
        assert!(!table.table_full);
        // A ninth is refused, and the flag says so.
        device
            .transform(&Transform::remap(Button::new(9), Button::new(10)))
            .unwrap();
        let table = device.query_transforms().unwrap();
        assert_eq!(table.entries.len(), 8);
        assert!(table.table_full);
    }

    #[test]
    fn reset_clears_the_transform_table() {
        let device = Device::with_mock(MockBox::new());
        device.invert(Axis::Y).unwrap();
        device.reset().unwrap();
        assert!(device.query_transforms().unwrap().entries.is_empty());
    }

    #[test]
    fn a_reapply_re_sends_held_transforms() {
        let mock = MockBox::new();
        let device = Device::with_mock(mock.clone());
        device.invert(Axis::Y).unwrap();
        mock.clear_recorded();
        device.reapply().unwrap();
        assert!(
            mock.saw(FrameType::Transform),
            "a reconnect must re-assert the held transform table"
        );
    }

    #[test]
    fn the_keepalive_re_asserts_a_held_transform() {
        use std::time::Duration;
        let mock = MockBox::new();
        let device =
            Device::from_transport_with_cadence(mock.transport(), Duration::from_millis(60));
        device.invert(Axis::Y).unwrap();
        mock.clear_recorded();
        std::thread::sleep(Duration::from_millis(220));
        assert!(
            mock.saw(FrameType::Transform),
            "the keepalive must re-send the TRANSFORM entry (feeds the silence timer and rebuilds a \
             box-side clear); saw {} frames",
            mock.recorded()
        );
        drop(device);
    }
}

#[cfg(all(feature = "async", feature = "mock"))]
mod async_roundtrip {
    use futures::executor::block_on;

    use crate::types::{Axis, Transform, TransformField, TransformOp};
    use crate::{Device, MockBox};

    #[test]
    fn async_set_and_query_transforms() {
        let device = Device::with_mock(MockBox::new()).into_async();
        device.transform(&Transform::invert(Axis::Y)).unwrap();
        let table = block_on(device.query_transforms()).unwrap();
        assert_eq!(table.entries.len(), 1);
        assert_eq!(table.entries[0].op, TransformOp::Invert);
        assert_eq!(table.entries[0].source, TransformField::Axis(Axis::Y));

        device.clear_transforms().unwrap();
        assert!(
            block_on(device.query_transforms())
                .unwrap()
                .entries
                .is_empty()
        );
    }

    #[test]
    fn async_ergonomics_forward_to_the_core() {
        let device = Device::with_mock(MockBox::new()).into_async();
        device.scale_transform(Axis::Wheel, 200).unwrap();
        let table = block_on(device.query_transforms()).unwrap();
        assert_eq!(table.entries.len(), 1);
        assert_eq!(table.entries[0].op, TransformOp::Scale);
        assert_eq!(table.entries[0].scale, 200);
    }
}
