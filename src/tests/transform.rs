//! `TRANSFORM` (§3.15): the payload bytes, the op/field vocabulary, `RESP(TRANSFORMS)` decode (no
//! per-entry state byte), and the MockBox round-trip through the whole table lifecycle.

use crate::protocol::command::transform_payload;
use crate::protocol::{Resp, parse_resp};
use crate::types::{
    Axis, Button, Class, Key, LockTarget, MediaKey, Transform, TransformOp, Usage,
};

#[test]
fn transform_payload_bytes() {
    // SCALE the wheel (axis class 3, id 2) by +200%, state add.
    let p = transform_payload(2, 3, 2, 3, 2, 200, 1);
    assert_eq!(p, [2, 3, 0x02, 0x00, 3, 0x02, 0x00, 0xC8, 0x00, 1]);
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
    let p = [16, 0, 1, 2, 3, 0x02, 0x00, 3, 0x02, 0x00, 0xC8, 0x00];
    let Some(Resp::Transforms(t)) = parse_resp(&p) else {
        panic!("not a RESP(TRANSFORMS)");
    };
    assert!(!t.table_full);
    assert_eq!(t.entries.len(), 1);
    let e = t.entries[0];
    assert_eq!(e.op, TransformOp::Scale);
    assert_eq!(e.source, LockTarget::Axis(Axis::Wheel));
    assert_eq!(e.dest, LockTarget::Axis(Axis::Wheel));
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
        LockTarget::Usage(Usage::new(Class::Button, 6))
    );
    assert_eq!(
        e.dest,
        LockTarget::Usage(Usage::new(Class::Media, 0x0233))
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
    assert_eq!(TransformOp::Scale.as_u8(), 2);
    assert_eq!(TransformOp::from_u8(0), Some(TransformOp::Remap));
    assert_eq!(TransformOp::from_u8(2), Some(TransformOp::Scale));
    assert_eq!(TransformOp::from_u8(3), None);
}

#[test]
fn transform_field_class_id_roundtrips() {
    // Axis carries class 3; a usage carries INJECT's class byte.
    assert_eq!(LockTarget::Axis(Axis::X).class_id(), (3, 0));
    assert_eq!(LockTarget::Axis(Axis::Pan).class_id(), (3, 3));
    assert_eq!(LockTarget::from(Button::new(6)).class_id(), (0, 6));
    assert_eq!(
        LockTarget::from(Usage::new(Class::Media, 0x0233)).class_id(),
        (2, 0x0233)
    );

    for (cls, id) in [(3, 0), (3, 3), (0, 6), (1, 0x04), (2, 0x0233)] {
        let f = LockTarget::from_class_id(cls, id).unwrap();
        assert_eq!(f.class_id(), (cls, id));
    }
    // An axis id past pan, and a class no field names, decode to nothing.
    assert_eq!(LockTarget::from_class_id(3, 4), None);
    assert_eq!(LockTarget::from_class_id(0x0A, 0), None);

    // as_axis pulls the axis back out, and is None for a momentary usage.
    assert_eq!(LockTarget::Axis(Axis::X).as_axis(), Some(Axis::X));
    assert_eq!(LockTarget::from(Button::new(6)).as_axis(), None);
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
    assert_eq!(a.key().source, LockTarget::Axis(Axis::Y));
    assert_eq!(a.key().dest, LockTarget::Axis(Axis::Y));
}

#[test]
fn invert_is_a_scale_of_minus_one_hundred() {
    // There is no invert op on the wire: the box returns a weigh of -100 exactly, so the constructor
    // builds the scale rather than a second op that would have to carry an ignored parameter.
    let inv = Transform::invert(Axis::Y);
    assert_eq!(inv.op, TransformOp::Scale);
    assert_eq!(inv.scale, -100);
    assert_eq!(inv, Transform::scale_axis(Axis::Y, -100));
    // And it round-trips through the wire as the same entry, so a readback compares equal to what the
    // caller built.
    let (sc, si) = inv.source.class_id();
    let (dc, di) = inv.dest.class_id();
    let p = transform_payload(inv.op.as_u8(), sc, si, dc, di, inv.scale, 1);
    let resp = [
        vec![16u8, 0, 1],
        p[..9].to_vec(),
    ]
    .concat();
    let Some(Resp::Transforms(t)) = parse_resp(&resp) else {
        panic!("not a RESP(TRANSFORMS)");
    };
    assert_eq!(t.entries[0], inv);
}

#[test]
fn transform_op_admits_mirrors_the_box() {
    use LockTarget::Axis as A;
    let x = A(Axis::X);
    let y = A(Axis::Y);
    let btn = LockTarget::from(Button::new(6));
    let key = LockTarget::from(Key::A);
    let media = LockTarget::from(MediaKey::VOLUME_UP);

    // Scale: one axis (source == dest).
    assert!(TransformOp::Scale.admits(x, x));
    assert!(!TransformOp::Scale.admits(x, y));
    assert!(!TransformOp::Scale.admits(x, btn));
    // Swap: two DIFFERENT axes; an axis with itself is not an exchange.
    assert!(TransformOp::Swap.admits(x, y));
    assert!(!TransformOp::Swap.admits(x, x));
    assert!(!TransformOp::Swap.admits(x, btn));
    // Remap: axis→axis, button→button, button→key, button→media; nothing else.
    assert!(TransformOp::Remap.admits(x, y));
    assert!(TransformOp::Remap.admits(btn, btn));
    assert!(TransformOp::Remap.admits(btn, key));
    assert!(TransformOp::Remap.admits(btn, media));
    assert!(!TransformOp::Remap.admits(x, btn));
    assert!(!TransformOp::Remap.admits(key, key));
}

#[test]
fn the_held_table_keeps_installation_order_through_every_mutation() {
    use crate::device::transform::to_stored;
    use crate::link::reconcile::DesiredState;

    let a = to_stored(&Transform::scale_axis(Axis::Y, 200)); // key (3,1,3,1)
    let b = to_stored(&Transform::remap(Axis::X, Axis::Y)); // key (3,0,3,1), sorts first
    let c = to_stored(&Transform::scale_axis(Axis::Wheel, 150)); // key (3,2,3,2)

    let mut d = DesiredState::default();
    d.apply_transform(a.clone());
    d.apply_transform(b.clone());
    d.apply_transform(c.clone());
    assert_eq!(d.held_transforms(), vec![a.clone(), b.clone(), c.clone()], "installation order, not key order");

    // An overwrite keeps the row's position, the way the box does.
    let a2 = to_stored(&Transform::scale_axis(Axis::Y, 150));
    d.apply_transform(a2.clone());
    assert_eq!(d.held_transforms(), vec![a2.clone(), b.clone(), c.clone()]);

    // A removal rolled back puts the entry where it was, not at the end.
    let undo = d.remove_transform(b.key());
    assert_eq!(d.held_transforms(), vec![a2.clone(), c.clone()]);
    d.restore_transform(undo);
    assert_eq!(d.held_transforms(), vec![a2.clone(), b.clone(), c.clone()]);

    // And an insert rolled back leaves nothing behind.
    let e = to_stored(&Transform::scale_axis(Axis::X, 120));
    let undo = d.apply_transform(e.clone());
    d.restore_transform(undo);
    assert_eq!(d.held_transforms(), vec![a2.clone(), b.clone(), c.clone()]);

    assert!(d.holds_transform(b.key()));
    assert_eq!(d.transform_count(), 3);
    d.clear_transforms();
    assert!(d.held_transforms().is_empty());
}

#[cfg(feature = "mock")]
mod mock_roundtrip {
    use crate::error::Error;
    use crate::types::{Axis, Button, Key, LockTarget, MouseCaps, Transform, TransformOp, Transforms};
    use crate::{Device, FrameType, MockBox};

    #[test]
    fn set_query_and_clear_roundtrip() {
        let device = Device::with_mock(MockBox::new());
        device.transform(&Transform::invert(Axis::Y)).unwrap();

        let table = device.query_transforms().unwrap();
        assert_eq!(table.entries.len(), 1);
        assert_eq!(table.entries[0].op, TransformOp::Scale);
        assert_eq!(table.entries[0].source, LockTarget::Axis(Axis::Y));
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
        device.transform_invert(Axis::Y).unwrap();
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
        device.transform_scale(Axis::Wheel, 200).unwrap();
        device.transform_swap(Axis::X, Axis::Y).unwrap();

        let entries = device.query_transforms().unwrap().entries;
        assert_eq!(entries.len(), 2);
        assert!(entries.iter().any(|e| e.op == TransformOp::Scale
            && e.source == LockTarget::Axis(Axis::Wheel)
            && e.scale == 200));
        assert!(entries.iter().any(|e| e.op == TransformOp::Swap
            && e.source == LockTarget::Axis(Axis::X)
            && e.dest == LockTarget::Axis(Axis::Y)));
    }

    #[test]
    fn same_key_overwrites_op_and_scale() {
        // invert(Y) and scale_axis(Y) share the key (Y, Y): the second overwrites, not appends.
        let device = Device::with_mock(MockBox::new());
        device.transform_invert(Axis::Y).unwrap();
        device.transform_scale(Axis::Y, 200).unwrap();
        let entries = device.query_transforms().unwrap().entries;
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].op, TransformOp::Scale);
        assert_eq!(entries[0].scale, 200);
    }

    #[test]
    fn untransform_drops_one_entry_by_key() {
        let device = Device::with_mock(MockBox::new());
        device.transform_invert(Axis::Y).unwrap();
        device.transform_scale(Axis::Wheel, 150).unwrap();
        assert_eq!(device.query_transforms().unwrap().entries.len(), 2);
        device.untransform(&Transform::invert(Axis::Y)).unwrap();
        let entries = device.query_transforms().unwrap().entries;
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].source, LockTarget::Axis(Axis::Wheel));
    }

    #[test]
    fn op_class_pair_is_rejected_before_the_wire() {
        let mock = MockBox::new();
        let device = Device::with_mock(mock.clone());
        // Invert across two different axes is not an op any class pair takes.
        let bad = Transform::new(TransformOp::Scale, Axis::X, Axis::Y, 100);
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
    fn a_scale_past_what_the_box_applies_is_rejected_before_the_wire() {
        // The wire carries an i16 but the box weighs at most LOCK_SCALE_MAX, so a wider number would be
        // applied at that bound while the readback echoed the number sent. Refuse instead.
        let mock = MockBox::new();
        let device = Device::with_mock(mock.clone());
        for bad in [1000i16, -1000, i16::MAX, i16::MIN] {
            assert!(
                matches!(
                    device.transform(&Transform::scale_axis(Axis::X, bad)),
                    Err(Error::TransformScaleRange { .. })
                ),
                "scale {bad} should be refused"
            );
        }
        assert!(!mock.saw(FrameType::Transform));
        // The bound itself is accepted.
        device
            .transform(&Transform::scale_axis(Axis::X, crate::LOCK_SCALE_MAX as i16))
            .unwrap();
    }

    #[test]
    fn a_percentage_on_a_button_source_is_rejected_before_the_wire() {
        // A button is one bit: it is pressed or it is not, so a percentage on one names something the
        // field cannot hold. The box refuses it too.
        let mock = MockBox::new();
        let device = Device::with_mock(mock.clone());
        let bad = Transform::remap(Button::SIDE1, Button::SIDE2).with_scale(50);
        assert!(matches!(
            device.transform(&bad),
            Err(Error::TransformUsageScale { .. })
        ));
        assert!(!mock.saw(FrameType::Transform));
        device
            .transform(&Transform::remap(Button::SIDE1, Button::SIDE2))
            .unwrap();
    }

    #[test]
    fn the_crate_refuses_a_set_past_the_ceiling_rather_than_holding_it_forever() {
        // Without this the host keeps an entry the box refused: the keepalive re-sends it every tick,
        // the state never reads idle, and after a reconnect the replay order decides which entries
        // actually land.
        let mock = MockBox::new();
        let device = Device::with_mock(mock.clone());
        for i in 0..Transforms::CAPACITY {
            let src = Button::new((i % 16) as u8);
            let dst = Button::new(((i + 1 + i / 16) % 16) as u8);
            device.transform(&Transform::remap(src, dst)).unwrap();
        }
        assert!(matches!(
            device.transform(&Transform::scale_axis(Axis::X, 200)),
            Err(Error::TransformTableFull { .. })
        ));
        // An overwrite of a key already held is not an insert, so it still goes.
        device
            .transform(&Transform::remap(Button::new(0), Button::new(1)))
            .unwrap();
        // And removing one makes room again.
        device
            .untransform(&Transform::remap(Button::new(0), Button::new(1)))
            .unwrap();
        device
            .transform(&Transform::scale_axis(Axis::X, 200))
            .unwrap();
    }

    #[test]
    fn an_undeclared_field_is_refused_by_the_box_not_the_crate() {
        // The default mock has no AC Pan and no keyboard, so the crate sends these (they are
        // structurally valid) and the box refuses them: absent from the readback, the frame still went.
        let mock = MockBox::new();
        let device = Device::with_mock(mock.clone());
        device.transform_invert(Axis::Pan).unwrap(); // pan not declared
        device.transform_remap(Button::LEFT, Key::A).unwrap(); // no keyboard collection
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
        // Distinct button→button remaps fill the table.
        for i in 0..Transforms::CAPACITY {
            let src = Button::new((i % 16) as u8);
            let dst = Button::new(((i + 1 + i / 16) % 16) as u8);
            device.transform(&Transform::remap(src, dst)).unwrap();
        }
        let table = device.query_transforms().unwrap();
        assert_eq!(table.entries.len(), Transforms::CAPACITY);
        assert!(!table.table_full);
        // One past is refused by the crate before it reaches the wire, so make the box say so itself.
        device
            .transform_send(&Transform::scale_axis(Axis::X, 200), 1)
            .unwrap();
        let table = device.query_transforms().unwrap();
        assert_eq!(table.entries.len(), Transforms::CAPACITY);
        assert!(table.table_full);
        // A removal makes room, and the flag stops advertising a table that has some.
        device
            .untransform(&Transform::remap(Button::new(0), Button::new(1)))
            .unwrap();
        let table = device.query_transforms().unwrap();
        assert_eq!(table.entries.len(), Transforms::CAPACITY - 1);
        assert!(!table.table_full);
    }

    #[test]
    fn reset_clears_the_transform_table() {
        let device = Device::with_mock(MockBox::new());
        device.transform_invert(Axis::Y).unwrap();
        device.reset().unwrap();
        assert!(device.query_transforms().unwrap().entries.is_empty());
    }

    // The (source, dest) bytes of every TRANSFORM frame the host has sent, in order.
    fn transform_keys(mock: &MockBox) -> Vec<(u8, u16, u8, u16)> {
        mock.recorded_frames()
            .into_iter()
            .filter(|f| f.ty == FrameType::Transform)
            .map(|f| {
                let p = f.payload;
                (
                    p[1],
                    u16::from_le_bytes([p[2], p[3]]),
                    p[4],
                    u16::from_le_bytes([p[5], p[6]]),
                )
            })
            .collect()
    }

    #[test]
    fn a_reapply_re_sends_held_transforms_in_installation_order() {
        // The box applies transforms in table order and two that write the same field do not commute,
        // so the replay is only correct if it rebuilds the ORDER, not just the set. Installed here so
        // the second entry sorts BELOW the first by wire key: a map-backed store would swap them.
        let mock = MockBox::new();
        let device = Device::with_mock(mock.clone());
        let scale_y = Transform::scale_axis(Axis::Y, 200); // key (3,1,3,1)
        let remap_xy = Transform::remap(Axis::X, Axis::Y); // key (3,0,3,1), sorts first
        device.transform(&scale_y).unwrap();
        device.transform(&remap_xy).unwrap();
        let installed = transform_keys(&mock);
        assert_eq!(installed, vec![(3, 1, 3, 1), (3, 0, 3, 1)]);

        mock.clear_recorded();
        device.reapply().unwrap();
        assert_eq!(
            transform_keys(&mock),
            installed,
            "a reconnect must replay the table in the order it was built, not in key order"
        );
    }

    #[test]
    fn an_overwrite_keeps_its_position_in_the_replay() {
        let mock = MockBox::new();
        let device = Device::with_mock(mock.clone());
        device.transform(&Transform::scale_axis(Axis::Y, 200)).unwrap();
        device.transform(&Transform::remap(Axis::X, Axis::Y)).unwrap();
        // Re-set the first entry at a new scale: it is the same key, so it stays first.
        device.transform(&Transform::scale_axis(Axis::Y, 150)).unwrap();
        mock.clear_recorded();
        device.reapply().unwrap();
        assert_eq!(transform_keys(&mock), vec![(3, 1, 3, 1), (3, 0, 3, 1)]);
        // And the replayed scale is the one that is live, not the one it replaced.
        let scales: Vec<i16> = mock
            .recorded_frames()
            .into_iter()
            .filter(|f| f.ty == FrameType::Transform)
            .map(|f| i16::from_le_bytes([f.payload[7], f.payload[8]]))
            .collect();
        assert_eq!(scales[0], 150);
    }



    #[test]
    fn the_keepalive_re_asserts_a_held_transform() {
        use std::time::Duration;
        let mock = MockBox::new();
        let device =
            Device::from_transport_with_cadence(mock.transport(), Duration::from_millis(60));
        device.transform_invert(Axis::Y).unwrap();
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

    use crate::types::{Axis, LockTarget, Transform, TransformOp};
    use crate::{Device, MockBox};

    #[test]
    fn async_set_and_query_transforms() {
        let device = Device::with_mock(MockBox::new()).into_async();
        device.transform(&Transform::invert(Axis::Y)).unwrap();
        let table = block_on(device.query_transforms()).unwrap();
        assert_eq!(table.entries.len(), 1);
        assert_eq!(table.entries[0].op, TransformOp::Scale);
        assert_eq!(table.entries[0].source, LockTarget::Axis(Axis::Y));

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
        device.transform_scale(Axis::Wheel, 200).unwrap();
        let table = block_on(device.query_transforms()).unwrap();
        assert_eq!(table.entries.len(), 1);
        assert_eq!(table.entries[0].op, TransformOp::Scale);
        assert_eq!(table.entries[0].scale, 200);
    }
}
