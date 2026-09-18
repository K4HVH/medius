use std::sync::Arc;
use std::time::Duration;

use std::time::Instant;

use crate::Device;
use crate::link::reconnect::probe_clip;
use crate::protocol::opcode::Q_CLIP;
use crate::protocol::{FrameType, encode};
use crate::transport::Disconnected;
use crate::transport::mock::MockTransport;
use crate::types::{Button, ClipAction, ClipBuilder, ClipTrigger, Edge, LogLevel};

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

// RESP(CLIP) with `total` bytes loaded, nothing held and no triggers.
fn clip_reply(total: u32) -> Vec<u8> {
    let mut p = vec![0u8; 31];
    p[0] = Q_CLIP;
    p[6..10].copy_from_slice(&total.to_le_bytes());
    p.extend_from_slice(&[0, 0, 0]);
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

    device.link.transport_slot().swap(Arc::new(Disconnected));
    assert!(clip.append(&one).is_err());
    assert!(clip.set_ride(true).is_err());
    assert!(clip.bind(trigger).is_err());
    assert!(idle());

    device.link.transport_slot().swap(mock.clone());
    clip.append(&one).unwrap();
    device.link.transport_slot().swap(Arc::new(Disconnected));
    assert!(clip.clear().is_err());
    assert!(
        !idle(),
        "a clear that never went out leaves the clip loaded"
    );

    device.link.transport_slot().swap(mock);
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
}
