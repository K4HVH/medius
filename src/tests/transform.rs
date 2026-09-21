//! `TRANSFORM` (§3.15): the payload bytes, the op/field vocabulary, `RESP(TRANSFORMS)` decode (no
//! per-entry state byte), and the MockBox round-trip through the whole table lifecycle.

use crate::protocol::command::transform_payload;
use crate::protocol::{Resp, parse_resp};
use crate::types::{Axis, Button, Class, Key, LockTarget, MediaKey, Transform, TransformOp, Usage};

#[test]
fn transform_payload_bytes() {
    // REMAP X (axis class 3, id 0) → Y (id 1), state add. No scale field: a transform moves a field,
    // it does not weigh one.
    let p = transform_payload(0, 3, 0, 3, 1, 1);
    assert_eq!(p, [0, 3, 0x00, 0x00, 3, 0x01, 0x00, 1]);
}

#[test]
fn transform_payload_high_ids() {
    // REMAP Button 6 (class 0) → Media 0x0233 (class 2): a u8 truncation of `did` fails here.
    let p = transform_payload(0, 0, 6, 2, 0x0233, 1);
    assert_eq!(p, [0, 0, 0x06, 0x00, 2, 0x33, 0x02, 1]);
}

#[test]
fn transform_clear_sentinel_bytes() {
    // The whole-table clear: op ignored, both classes 0xFF, both ids 0xFFFF, state 0.
    let p = transform_payload(0, 0xFF, 0xFFFF, 0xFF, 0xFFFF, 0);
    assert_eq!(p, [0, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00]);
}

#[test]
fn resp_transforms_decode() {
    // [16][flags 0][n 1] then one entry: Swap, X ↔ Y. No state byte and no scale per entry.
    let p = [16, 0, 1, 1, 3, 0x00, 0x00, 3, 0x01, 0x00];
    let Some(Resp::Transforms(t)) = parse_resp(&p) else {
        panic!("not a RESP(TRANSFORMS)");
    };
    assert!(!t.table_full);
    assert_eq!(t.entries.len(), 1);
    let e = t.entries[0];
    assert_eq!(e.op, TransformOp::Swap);
    assert_eq!(e.source, LockTarget::Axis(Axis::X));
    assert_eq!(e.dest, LockTarget::Axis(Axis::Y));
}

#[test]
fn resp_transforms_high_bytes_and_full_flag() {
    // Hand-computed: table-full flag set, one cross-class remap Button(6) → Media(0x0233). A u8
    // truncation of `did` (0x0233 → 0x33) fails here.
    let p = [16, 0x01, 1, 0, 0x00, 0x06, 0x00, 2, 0x33, 0x02];
    let Some(Resp::Transforms(t)) = parse_resp(&p) else {
        panic!("not a RESP(TRANSFORMS)");
    };
    assert!(t.table_full);
    assert_eq!(t.entries.len(), 1);
    let e = t.entries[0];
    assert_eq!(e.op, TransformOp::Remap);
    assert_eq!(e.source, LockTarget::Usage(Usage::new(Class::Button, 6)));
    assert_eq!(e.dest, LockTarget::Usage(Usage::new(Class::Media, 0x0233)));
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
    assert_eq!(TransformOp::from_u8(0), Some(TransformOp::Remap));
    assert_eq!(TransformOp::from_u8(1), Some(TransformOp::Swap));
    assert_eq!(TransformOp::from_u8(2), None);
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
    // Two transforms with the same (source, dest) share a key (the box overwrites the op in place),
    // and one that differs in either field does not.
    let a = Transform::remap(Axis::X, Axis::Y);
    let b = Transform::swap(Axis::X, Axis::Y);
    let c = Transform::remap(Axis::X, Axis::Wheel);
    assert_eq!(a.key(), b.key());
    assert_ne!(a.key(), c.key());
    assert_eq!(a.key().source, LockTarget::Axis(Axis::X));
    assert_eq!(a.key().dest, LockTarget::Axis(Axis::Y));
}

#[test]
fn a_built_transform_equals_its_own_readback() {
    // The readback is meant to be the command that rebuilds the entry, so what the caller built has to
    // compare equal to what comes back. Every field the frame carries is in the RESP row.
    let t = Transform::swap(Axis::X, Axis::Y);
    let (sc, si) = t.source.class_id();
    let (dc, di) = t.dest.class_id();
    let p = transform_payload(t.op.as_u8(), sc, si, dc, di, 1);
    let resp = [vec![16u8, 0, 1], p[..7].to_vec()].concat();
    let Some(Resp::Transforms(back)) = parse_resp(&resp) else {
        panic!("not a RESP(TRANSFORMS)");
    };
    assert_eq!(back.entries[0], t);
}

#[test]
fn transform_op_admits_mirrors_the_box() {
    use LockTarget::Axis as A;
    let x = A(Axis::X);
    let y = A(Axis::Y);
    let btn = LockTarget::from(Button::new(6));
    let key = LockTarget::from(Key::A);
    let media = LockTarget::from(MediaKey::VOLUME_UP);

    // Neither op takes one field as both ends: a move needs two.
    assert!(!TransformOp::Swap.admits(x, x));
    assert!(!TransformOp::Remap.admits(x, x));
    assert!(!TransformOp::Remap.admits(btn, btn));
    // Swap: two different axes.
    assert!(TransformOp::Swap.admits(x, y));
    assert!(!TransformOp::Swap.admits(x, btn));
    // Remap: axis→axis, button→button, button→key, button→media; nothing else.
    assert!(TransformOp::Remap.admits(x, y));
    assert!(TransformOp::Remap.admits(btn, LockTarget::from(Button::new(7))));
    assert!(TransformOp::Remap.admits(btn, key));
    assert!(TransformOp::Remap.admits(btn, media));
    assert!(!TransformOp::Remap.admits(x, btn));
    assert!(!TransformOp::Remap.admits(key, key));
}

#[test]
fn the_held_table_keeps_installation_order_through_every_mutation() {
    use crate::device::transform::to_stored;
    use crate::link::reconcile::DesiredState;

    let a = to_stored(&Transform::swap(Axis::Y, Axis::Wheel)); // key (3,1,3,2)
    let b = to_stored(&Transform::remap(Axis::X, Axis::Y)); // key (3,0,3,1), sorts first
    let c = to_stored(&Transform::remap(Axis::Wheel, Axis::X)); // key (3,2,3,0)

    let mut d = DesiredState::default();
    d.apply_transform(a.clone());
    d.apply_transform(b.clone());
    d.apply_transform(c.clone());
    assert_eq!(
        d.held_transforms(),
        vec![a.clone(), b.clone(), c.clone()],
        "installation order, not key order"
    );

    // An overwrite keeps the row's position, the way the box does.
    let a2 = to_stored(&Transform::remap(Axis::Y, Axis::Wheel));
    d.apply_transform(a2.clone());
    assert_eq!(d.held_transforms(), vec![a2.clone(), b.clone(), c.clone()]);

    // A removal rolled back puts the entry where it was, not at the end.
    let undo = d.remove_transform(b.key());
    assert_eq!(d.held_transforms(), vec![a2.clone(), c.clone()]);
    d.restore_transform(undo);
    assert_eq!(d.held_transforms(), vec![a2.clone(), b.clone(), c.clone()]);

    // And an insert rolled back leaves nothing behind.
    let e = to_stored(&Transform::remap(Axis::X, Axis::Wheel));
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
    use crate::types::{
        Axis, Button, Key, LockTarget, MouseCaps, Transform, TransformOp, Transforms,
    };
    use crate::{Device, FrameType, MockBox};

    #[test]
    fn set_query_and_clear_roundtrip() {
        let device = Device::with_mock(MockBox::new());
        device
            .transform(&Transform::swap(Axis::X, Axis::Y))
            .unwrap();

        let table = device.query_transforms().unwrap();
        assert_eq!(table.entries.len(), 1);
        assert_eq!(table.entries[0].op, TransformOp::Swap);
        assert_eq!(table.entries[0].source, LockTarget::Axis(Axis::X));
        assert_eq!(table.entries[0].dest, LockTarget::Axis(Axis::Y));
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
        device.transform_swap(Axis::X, Axis::Y).unwrap();
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
        device.transform_remap(Axis::Wheel, Axis::Pan).unwrap();
        device.transform_swap(Axis::X, Axis::Y).unwrap();

        let entries = device.query_transforms().unwrap().entries;
        assert_eq!(entries.len(), 2);
        assert!(entries.iter().any(|e| e.op == TransformOp::Remap
            && e.source == LockTarget::Axis(Axis::Wheel)
            && e.dest == LockTarget::Axis(Axis::Pan)));
        assert!(entries.iter().any(|e| e.op == TransformOp::Swap
            && e.source == LockTarget::Axis(Axis::X)
            && e.dest == LockTarget::Axis(Axis::Y)));
    }

    #[test]
    fn same_key_overwrites_the_op() {
        // remap(X → Y) and swap(X, Y) share the key (X, Y): the second overwrites, not appends.
        let device = Device::with_mock(MockBox::new());
        device.transform_remap(Axis::X, Axis::Y).unwrap();
        device.transform_swap(Axis::X, Axis::Y).unwrap();
        let entries = device.query_transforms().unwrap().entries;
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].op, TransformOp::Swap);
    }

    #[test]
    fn untransform_drops_one_entry_by_key() {
        let device = Device::with_mock(MockBox::new());
        device.transform_swap(Axis::X, Axis::Y).unwrap();
        device.transform_remap(Axis::Wheel, Axis::X).unwrap();
        assert_eq!(device.query_transforms().unwrap().entries.len(), 2);
        device
            .untransform(&Transform::swap(Axis::X, Axis::Y))
            .unwrap();
        let entries = device.query_transforms().unwrap().entries;
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].source, LockTarget::Axis(Axis::Wheel));
    }

    #[test]
    fn op_class_pair_is_rejected_before_the_wire() {
        let mock = MockBox::new();
        let device = Device::with_mock(mock.clone());
        // A swap of an axis with a button has no mechanism: only two axes exchange.
        let bad = Transform::new(TransformOp::Swap, Axis::X, Button::LEFT);
        assert!(matches!(
            device.transform(&bad),
            Err(Error::TransformOpFields { .. })
        ));
        // A remap of an axis onto a button has none either.
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
    fn a_field_onto_itself_is_rejected_before_the_wire() {
        // Both ops MOVE a value, so a source that is also the destination names no operation at all.
        // Weighing a field in place is the lock's, and it has its own command.
        let mock = MockBox::new();
        let device = Device::with_mock(mock.clone());
        for bad in [
            Transform::remap(Axis::X, Axis::X),
            Transform::swap(Axis::X, Axis::X),
            Transform::remap(Button::LEFT, Button::LEFT),
        ] {
            assert!(
                matches!(device.transform(&bad), Err(Error::TransformOpFields { .. })),
                "{bad:?} should be refused"
            );
        }
        assert!(!mock.saw(FrameType::Transform));
    }

    #[test]
    fn the_box_refuses_a_self_pair_on_its_own() {
        // The crate refuses one too, so go around it with the raw send: a host that lost its guard must
        // not be able to install a field onto itself, which would silently zero it.
        let mock = MockBox::new();
        let device = Device::with_mock(mock.clone());
        device
            .transform_send(&Transform::remap(Axis::X, Axis::X), 1)
            .unwrap();
        device
            .transform_send(&Transform::swap(Axis::X, Axis::X), 1)
            .unwrap();
        assert!(mock.saw(FrameType::Transform), "both reached the wire");
        assert!(
            device.query_transforms().unwrap().entries.is_empty(),
            "and the box held neither"
        );
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
            device.transform(&Transform::swap(Axis::X, Axis::Y)),
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
            .transform(&Transform::swap(Axis::X, Axis::Y))
            .unwrap();
    }

    #[test]
    fn an_undeclared_field_is_refused_by_the_box_not_the_crate() {
        // The default mock has no AC Pan and no keyboard, so the crate sends these (they are
        // structurally valid) and the box refuses them: absent from the readback, the frame still went.
        let mock = MockBox::new();
        let device = Device::with_mock(mock.clone());
        device.transform_remap(Axis::X, Axis::Pan).unwrap(); // pan not declared
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
            .transform_send(&Transform::swap(Axis::X, Axis::Y), 1)
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
        device.transform_swap(Axis::X, Axis::Y).unwrap();
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
        // The box applies transforms in table order and two that write the same field do not
        // commute, so the replay is only correct if it rebuilds the ORDER, not just the set.
        let mock = MockBox::new();
        let device = Device::with_mock(mock.clone());
        let swap_y_wheel = Transform::swap(Axis::Y, Axis::Wheel); // key (3,1,3,2)
        let remap_xy = Transform::remap(Axis::X, Axis::Y); // key (3,0,3,1), sorts first
        device.transform(&swap_y_wheel).unwrap();
        device.transform(&remap_xy).unwrap();
        let installed = transform_keys(&mock);
        assert_eq!(installed, vec![(3, 1, 3, 2), (3, 0, 3, 1)]);

        mock.clear_recorded();
        device.reapply().unwrap();
        assert_eq!(
            transform_keys(&mock),
            installed,
            "a reconnect must replay the table in the order it was built, not in key order"
        );
    }

    #[test]
    fn the_readback_comes_back_in_apply_order() {
        // The host→box half of the ordering claim is covered by the replay tests; this is the
        // box→host half.
        let device = Device::with_mock(MockBox::new());
        let first = Transform::swap(Axis::Y, Axis::Wheel); // key (3,1,3,2)
        let second = Transform::remap(Axis::X, Axis::Y); // key (3,0,3,1), sorts BELOW the first
        device.transform(&first).unwrap();
        device.transform(&second).unwrap();
        let table = device.query_transforms().unwrap();
        assert_eq!(
            table.entries,
            vec![first, second],
            "the readback must carry installation order, not key order"
        );
        // An overwrite keeps its row, so the order does not change and the new op is the one read.
        let reop = Transform::remap(Axis::Y, Axis::Wheel);
        device.transform(&reop).unwrap();
        let table = device.query_transforms().unwrap();
        assert_eq!(table.entries[0], reop);
        assert_eq!(table.entries[1], second);
    }

    #[test]
    fn an_overwrite_keeps_its_position_in_the_replay() {
        let mock = MockBox::new();
        let device = Device::with_mock(mock.clone());
        device
            .transform(&Transform::swap(Axis::Y, Axis::Wheel))
            .unwrap();
        device
            .transform(&Transform::remap(Axis::X, Axis::Y))
            .unwrap();
        // Re-set the first entry at a new op: it is the same key, so it stays first.
        device
            .transform(&Transform::remap(Axis::Y, Axis::Wheel))
            .unwrap();
        mock.clear_recorded();
        device.reapply().unwrap();
        assert_eq!(transform_keys(&mock), vec![(3, 1, 3, 2), (3, 0, 3, 1)]);
        // And the replayed op is the one that is live, not the one it replaced.
        let ops: Vec<u8> = mock
            .recorded_frames()
            .into_iter()
            .filter(|f| f.ty == FrameType::Transform)
            .map(|f| f.payload[0])
            .collect();
        assert_eq!(ops[0], TransformOp::Remap.as_u8());
    }

    #[test]
    fn the_keepalive_re_asserts_a_held_transform() {
        use std::time::Duration;
        let mock = MockBox::new();
        let device =
            Device::from_transport_with_cadence(mock.transport(), Duration::from_millis(60));
        device.transform_swap(Axis::X, Axis::Y).unwrap();
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
        device
            .transform(&Transform::swap(Axis::X, Axis::Y))
            .unwrap();
        let table = block_on(device.query_transforms()).unwrap();
        assert_eq!(table.entries.len(), 1);
        assert_eq!(table.entries[0].op, TransformOp::Swap);
        assert_eq!(table.entries[0].source, LockTarget::Axis(Axis::X));

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
        device.transform_remap(Axis::Wheel, Axis::X).unwrap();
        let table = block_on(device.query_transforms()).unwrap();
        assert_eq!(table.entries.len(), 1);
        assert_eq!(table.entries[0].op, TransformOp::Remap);
        assert_eq!(table.entries[0].dest, LockTarget::Axis(Axis::X));
    }
}
