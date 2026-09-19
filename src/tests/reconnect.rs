use std::sync::Arc;
use std::time::Duration;

use std::time::Instant;

use crate::Device;
use crate::link::reconnect::probe_clip;
use crate::protocol::opcode::Q_CLIP;
use crate::protocol::{FrameType, encode};
use crate::transport::Disconnected;
use crate::transport::mock::MockTransport;
use crate::types::{
    Button, ClipAction, ClipBuilder, ClipPacketTrigger, ClipTrigger, Direction, Edge, LogLevel,
    TrafficClass,
};

#[test]
fn transport_swap_resets_decoder() {
    let mock_a = Arc::new(MockTransport::new());
    let device = Device::from_transport_with_cadence(mock_a.clone(), Duration::from_secs(60));
    let rx = device.logs();

    let partial = encode(FrameType::Log, 0, &[2, b'o', b'l', b'd']).unwrap();
    let cut = partial.len() / 2;
    mock_a.push_bytes(&partial[..cut]);
    std::thread::sleep(Duration::from_millis(20));

    let mock_b = Arc::new(MockTransport::new());
    device.link.transport_slot().swap(mock_b.clone());
    mock_b.push_frame(FrameType::Log, 0, &[2, b'n', b'e', b'w']);

    let line = rx
        .recv_timeout(Duration::from_secs(1))
        .expect("the post-swap LOG must decode cleanly");
    assert_eq!(line.level, LogLevel::Info);
    assert_eq!(line.text, "new");
}

// RESP(CLIP) with `total` bytes loaded, nothing held and no triggers of either kind.
fn clip_reply(total: u32) -> Vec<u8> {
    let mut p = vec![0u8; 31];
    p[0] = Q_CLIP;
    p[6..10].copy_from_slice(&total.to_le_bytes());
    p.extend_from_slice(&[0, 0, 0, 0]);
    p
}

fn answering(replies: Vec<Vec<u8>>) -> MockTransport {
    MockTransport::with_responder(move |ty, seq, _| {
        let mut out = Vec::new();
        if ty == FrameType::Query {
            for r in &replies {
                out.extend(encode(FrameType::Resp, seq, r).unwrap());
            }
        }
        out
    })
}

// A reconnect reads the box's clip off the reopened port before the reader thread is on it.
#[test]
fn the_clip_probe_takes_the_first_reply_for_its_selector() {
    // A reply for another selector ahead of it is some other query's.
    let other = vec![1u8, 7, 3, 4, 1];
    let got = probe_clip(&answering(vec![other, clip_reply(40)])).expect("a clip reply");
    assert_eq!(got.0.total, 40);

    // A later reply never replaces the one already taken, readable or not.
    let got = probe_clip(&answering(vec![
        clip_reply(40),
        clip_reply(9),
        vec![Q_CLIP, 0],
    ]))
    .expect("the first reply stands");
    assert_eq!(got.0.total, 40);
}

// What the probe reads is what the reconnect adopts, and a packet trigger the box still holds is
// enough on its own to keep the keepalive running.
#[test]
fn the_clip_probe_reads_a_packet_trigger_the_reconnect_adopts() {
    let mut reply = clip_reply(0);
    *reply.last_mut().unwrap() = 1;
    reply.extend_from_slice(&[
        0x04, 0x02, 0x00, 0x01, 0x00, 0x04, 0x01, 0x02, 0x09, 0x00, 0x07, 0x20, 0xFF, 0x20,
    ]);
    let (status, settings) = probe_clip(&answering(vec![reply])).expect("a clip reply");
    let held = ClipPacketTrigger::new(TrafficClass::HidIn, 2, Direction::IN, ClipAction::Start)
        .matching([0x07, 0x20], [0xFF, 0x20])
        .once_per_run(1);
    assert_eq!(settings.packet_triggers.len(), 1);
    assert_eq!(settings.packet_triggers[0].trigger, held);
    assert_eq!(settings.packet_triggers[0].hits, 9);

    let mock = Arc::new(MockTransport::new());
    let device = Device::from_transport_with_cadence(mock, Duration::from_secs(60));
    let idle = || device.link.desired().lock().is_idle();
    device.link.desired().lock().clip_adopt(&status, &settings);
    assert!(!idle(), "the adopted trigger is held");
    // The adopted key is the one the caller's own trigger names, so its unbind lets the keepalive idle.
    device.clip().unbind_packet(&held).unwrap();
    assert!(idle());
}

// A box that answers in a shape this crate cannot read answers the same way to every re-send, so the
// probe ends on the first one and the reconnect does not wait the deadline out.
#[test]
fn a_clip_reply_the_crate_cannot_read_ends_the_probe_at_once() {
    let mut older = vec![0u8; 25];
    older[0] = Q_CLIP;
    older.extend_from_slice(&[0, 0, 0]);
    let start = Instant::now();
    assert!(probe_clip(&answering(vec![older])).is_none());
    assert!(
        start.elapsed() < Duration::from_millis(300),
        "{:?}",
        start.elapsed()
    );
}

// What the keepalive holds of a clip is recorded once the frame is out, so a call that failed on a
// dropped link leaves nothing behind to keep alive.
#[test]
fn a_clip_call_that_never_went_out_records_nothing() {
    let mock = Arc::new(MockTransport::new());
    let device = Device::from_transport_with_cadence(mock.clone(), Duration::from_secs(60));
    let clip = device.clip();
    let idle = || device.link.desired().lock().is_idle();
    let mut one = ClipBuilder::new();
    one.move_by(1, 0);
    let trigger = ClipTrigger::new(Button::SIDE1, Edge::Press, ClipAction::Start);
    let packet = ClipPacketTrigger::new(TrafficClass::Emit, 1, Direction::IN, ClipAction::Start);

    device.link.transport_slot().swap(Arc::new(Disconnected));
    assert!(clip.append(&one).is_err());
    assert!(clip.set_ride(true).is_err());
    assert!(clip.bind(trigger).is_err());
    assert!(clip.bind_packet(&packet).is_err());
    assert!(idle());

    device.link.transport_slot().swap(mock.clone());
    clip.append(&one).unwrap();
    device.link.transport_slot().swap(Arc::new(Disconnected));
    assert!(clip.clear().is_err());
    assert!(
        !idle(),
        "a clear that never went out leaves the clip loaded"
    );

    device.link.transport_slot().swap(mock.clone());
    clip.clear().unwrap();
    assert!(idle());
    clip.bind(trigger).unwrap();
    device.link.transport_slot().swap(Arc::new(Disconnected));
    assert!(clip.unbind(Button::SIDE1, Edge::Press).is_err());
    assert!(clip.clear_triggers().is_err());
    assert!(
        !idle(),
        "an unbind that never went out leaves the trigger bound"
    );

    // The same for a packet trigger, and one clear that went out forgets both kinds.
    device.link.transport_slot().swap(mock.clone());
    clip.unbind(Button::SIDE1, Edge::Press).unwrap();
    clip.bind_packet(&packet).unwrap();
    assert!(!idle());
    device.link.transport_slot().swap(Arc::new(Disconnected));
    assert!(clip.unbind_packet(&packet).is_err());
    assert!(clip.clear_triggers().is_err());
    assert!(
        !idle(),
        "an unbind that never went out leaves the packet trigger bound"
    );
    device.link.transport_slot().swap(mock);
    clip.bind(trigger).unwrap();
    clip.clear_triggers().unwrap();
    assert!(idle());
}

// RESP(VERSION) for a box on `proto` with base MAC `mac`.
fn version_reply(proto: u8, mac: [u8; 6]) -> Vec<u8> {
    let mut p = vec![crate::protocol::opcode::Q_VERSION, proto, 3, 4, 0];
    p.extend_from_slice(&mac);
    p
}

// The same box answering after a reflash. Every other query gets a reply the probe cannot read, which
// ends that probe at once.
fn reflashed(proto: u8, mac: [u8; 6]) -> Arc<dyn crate::transport::Transport> {
    Arc::new(MockTransport::with_responder(
        move |ty, seq, payload| match (ty, payload.first()) {
            (FrameType::Query, Some(&crate::protocol::opcode::Q_VERSION)) => {
                encode(FrameType::Resp, seq, &version_reply(proto, mac)).unwrap()
            }
            (FrameType::Query, Some(&what)) => encode(FrameType::Resp, seq, &[what]).unwrap(),
            _ => Vec::new(),
        },
    ))
}

#[test]
fn a_rescan_refuses_the_box_back_on_another_protocol() {
    let mac = [0x5A, 0x4E, 0x00, 0x11, 0x1E, 0x28];
    let device = Device::from_transport_with_cadence(
        Arc::new(MockTransport::new()),
        Duration::from_secs(60),
    );
    device
        .link
        .set_identity(crate::link::reconnect::BoxIdentity { serial: None, mac });

    // v3.4.0 firmware answers protocol 7, and a later one 9. Neither is taken back.
    for proto in [7, crate::PROTO_VER + 1] {
        let port = reflashed(proto, mac);
        let err = device
            .link
            .adopt_reopened(vec![("/dev/ttyACM0".into(), Arc::clone(&port))])
            .unwrap_err();
        assert!(
            matches!(err, crate::Error::BadProtoVer { got } if got == proto),
            "{proto}: {err:?}"
        );
        assert_eq!(Arc::strong_count(&port), 1, "{proto}: the port was adopted");
        assert_eq!(device.counters().reconnects, 0);
    }

    // On this build's protocol the same box is taken back.
    let port = reflashed(crate::PROTO_VER, mac);
    device
        .link
        .adopt_reopened(vec![("/dev/ttyACM0".into(), Arc::clone(&port))])
        .expect("the box on this build's protocol is adopted");
    assert_eq!(device.counters().reconnects, 1);
}
