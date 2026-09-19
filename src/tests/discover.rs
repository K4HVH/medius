#![cfg(feature = "mock")]

use crate::device::discover::{BoxInfo, may_clone, pick_by_id, pick_where, probe_transport};
use crate::{DeviceInfo, DeviceKind, Error, MockBox, PROTO_VER, PortInfo, Version};

// v3.4.0 firmware answers protocol 7.
const V3_4_0: u8 = 7;

fn port(n: u8) -> PortInfo {
    PortInfo {
        path: format!("/dev/ttyACM{n}"),
        vid: 0x1A86,
        pid: 0x55D3,
        serial: Some(format!("SER{n}")),
    }
}

// What discovery lists for a box on `port(n)` answering `proto`, with MAC 5a4e000000nn, cloning `kind`.
fn listed(n: u8, proto: u8, kind: DeviceKind) -> BoxInfo {
    let mock = MockBox::new()
        .with_version(Version {
            proto_ver: proto,
            fw_major: 3,
            fw_minor: 4,
            fw_patch: 0,
            mac: [0x5A, 0x4E, 0, 0, 0, n],
            name: format!("box{n}"),
        })
        .with_device_info(DeviceInfo {
            kind,
            ..DeviceInfo::default()
        });
    probe_transport(&port(n), mock.transport()).expect("the mock answers VERSION")
}

#[test]
fn list_keeps_a_box_on_another_protocol_with_its_version() {
    let old = listed(1, V3_4_0, DeviceKind::Mouse);
    assert_eq!(old.version.proto_ver, V3_4_0);
    assert_eq!(old.id(), "5a4e00000001");
    assert_eq!(old.name(), "box1");
    assert_eq!(
        old.device, None,
        "its clone is read only over this build's protocol"
    );

    let current = listed(2, PROTO_VER, DeviceKind::Mouse);
    assert_eq!(current.version.proto_ver, PROTO_VER);
    assert_eq!(current.device.map(|d| d.kind), Some(DeviceKind::Mouse));
}

#[test]
fn open_by_id_refuses_the_matched_box_for_its_protocol() {
    let boxes = [
        listed(1, PROTO_VER, DeviceKind::Mouse),
        listed(2, V3_4_0, DeviceKind::Mouse),
    ];
    for id in ["5a4e00000002", "5A:4E:00:00:00:02", "SER2", "ser2"] {
        let err = pick_by_id(&boxes, id).unwrap_err();
        assert!(
            matches!(err, Error::BadProtoVer { got: V3_4_0 }),
            "{id}: {err:?}"
        );
    }
    assert_eq!(
        pick_by_id(&boxes, "SER1").unwrap().port.path,
        "/dev/ttyACM1"
    );
    assert!(matches!(
        pick_by_id(&boxes, "5a4e00000009"),
        Err(Error::NotFound)
    ));
}

#[test]
fn find_on_only_a_v3_4_0_box_is_bad_proto_ver() {
    let boxes = [listed(1, V3_4_0, DeviceKind::Mouse)];
    for kind in [DeviceKind::Mouse, DeviceKind::Keyboard] {
        let err = pick_where(&boxes, may_clone(kind)).unwrap_err();
        assert!(
            matches!(err, Error::BadProtoVer { got: V3_4_0 }),
            "{kind}: {err:?}"
        );
    }
    assert!(matches!(
        pick_where(&[], may_clone(DeviceKind::Mouse)),
        Err(Error::NotFound)
    ));
}

#[test]
fn find_prefers_a_box_this_build_speaks_to() {
    // The old box comes first on the bus, and a current box clones a mouse.
    let boxes = [
        listed(1, V3_4_0, DeviceKind::Mouse),
        listed(2, PROTO_VER, DeviceKind::Mouse),
    ];
    let picked = pick_where(&boxes, may_clone(DeviceKind::Mouse)).unwrap();
    assert_eq!(picked.port.path, "/dev/ttyACM2");

    // No current box clones a keyboard, and the old box's clone is unknown: it could be the one.
    let err = pick_where(&boxes, may_clone(DeviceKind::Keyboard)).unwrap_err();
    assert!(matches!(err, Error::BadProtoVer { got: V3_4_0 }), "{err:?}");
}
