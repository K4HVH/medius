#![cfg(feature = "mock")]

use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::protocol::opcode::{
    CLIP_SET_LOOP, CLIP_SET_RETAIN, CLIP_TRIG_F_PRESENT, Q_CAPS, Q_VERSION,
};
use crate::protocol::{FrameType, encode};
use crate::{
    Axis, Button, CatchFilter, ClipAction, ClipBuilder, ClipPacketTrigger, ClipTrigger, Device,
    Direction, Edge, MockBox, RebootTarget, RewriteAction, RewriteClass, RewriteRule, TrafficClass,
    Transform,
};

// Polls until the crate has recovered from `n` restarts.
fn await_restarts(device: &Device, n: u64) {
    let deadline = Instant::now() + Duration::from_secs(3);
    while device.counters().restarts < n {
        assert!(
            Instant::now() < deadline,
            "restarts stayed at {}",
            device.counters().restarts
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn sent(mock: &MockBox, ty: FrameType) -> Vec<Vec<u8>> {
    mock.recorded_frames()
        .into_iter()
        .filter(|f| f.ty == ty)
        .map(|f| f.payload)
        .collect()
}

#[test]
fn a_restart_re_sends_every_kind_of_held_state() {
    let mock = MockBox::new().with_imperfect(true);
    let device = Device::open_mock(mock.clone()).unwrap();
    let clip = device.clip();

    device.press(Button::SIDE1).unwrap();
    device.scale(Axis::X, Direction::Both, 40).unwrap();
    let _events = device.catch_events([CatchFilter::everything()]).unwrap();
    let rule = RewriteRule::new(RewriteClass::Emit, 1, Direction::IN, RewriteAction::Drop);
    device.set_rewrite(&rule).unwrap();
    device
        .transform(&Transform::swap(Axis::X, Axis::Y))
        .unwrap();
    clip.set_retain(true).unwrap();
    clip.set_loop(true).unwrap();
    let trigger = ClipTrigger::new(Button::SIDE2, Edge::Press, ClipAction::Toggle).consume();
    clip.bind(trigger).unwrap();
    let packet = ClipPacketTrigger::new(TrafficClass::HidIn, 0, Direction::IN, ClipAction::Start)
        .matching([0x01], [0xFF]);
    clip.bind_packet(&packet).unwrap();
    let mut one = ClipBuilder::new();
    one.move_by(1, 0);
    clip.append(&one).unwrap();

    mock.clear_recorded();
    mock.restart();
    await_restarts(&device, 1);

    // It waited for the clone before sending anything back.
    let frames = mock.recorded_frames();
    let first_caps = frames
        .iter()
        .position(|f| f.ty == FrameType::Query && f.payload == [Q_CAPS])
        .expect("the recovery asks for the clone");
    let first_state = frames
        .iter()
        .position(|f| f.ty == FrameType::Lock)
        .expect("the lock went back");
    assert!(first_caps < first_state);

    assert_eq!(sent(&mock, FrameType::Inject), vec![vec![0, 3, 0, 1]]);
    assert!(
        device
            .query_locks()
            .unwrap()
            .scale_of(Axis::X, Direction::Positive)
            == 40
    );
    assert!(!sent(&mock, FrameType::Catch).is_empty());
    assert_eq!(device.query_rewrite_entry(0).unwrap(), rule);
    assert_eq!(device.query_transforms().unwrap().entries.len(), 1);
    assert_eq!(
        sent(&mock, FrameType::ClipSet),
        vec![vec![CLIP_SET_LOOP, 1], vec![CLIP_SET_RETAIN, 1]]
    );
    let binds = sent(&mock, FrameType::ClipTrigger);
    assert!(binds.contains(&crate::device::clip::bind_payload(&trigger).to_vec()));
    assert!(binds.iter().all(|b| b[5] & CLIP_TRIG_F_PRESENT != 0));
    let held = clip.query_config().unwrap().packet_triggers;
    assert_eq!(held.len(), 1);
    assert_eq!(held[0].trigger, packet);

    // The ring is what cannot come back.
    assert!(clip.lost());
    assert!(sent(&mock, FrameType::ClipAppend).is_empty());
}

#[test]
fn both_hellos_of_one_boot_are_one_restart() {
    let mock = MockBox::new();
    let device = Device::open_mock(mock.clone()).unwrap();
    device.press(Button::LEFT).unwrap();
    mock.restart();
    await_restarts(&device, 1);
    // The recovery's first frame drew the second hello, and more traffic draws none.
    let hellos = mock
        .replied_frames()
        .iter()
        .filter(|f| f.ty == FrameType::Resp && f.seq == 0 && f.payload[0] == Q_VERSION)
        .count();
    assert_eq!(hellos, 2);
    for _ in 0..5 {
        device.query_health().unwrap();
    }
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(device.counters().restarts, 1);

    mock.restart();
    await_restarts(&device, 2);
}

#[test]
fn a_solicited_version_reply_is_no_restart() {
    let mock = MockBox::new();
    let device = Device::open_mock(mock.clone()).unwrap();
    device.press(Button::LEFT).unwrap();
    mock.clear_recorded();
    // Past a full turn of the SEQ counter, so a query would have landed on the hello's SEQ.
    for _ in 0..300 {
        assert_eq!(device.query_version().unwrap().proto_ver, crate::PROTO_VER);
    }
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(device.counters().restarts, 0);
    assert!(sent(&mock, FrameType::Inject).is_empty(), "nothing re-sent");
    let queried = mock
        .recorded_frames()
        .into_iter()
        .filter(|f| f.ty == FrameType::Query && f.payload == [Q_VERSION]);
    assert!(queried.clone().count() >= 300);
    assert!(queried.into_iter().all(|f| f.seq != 0));
}

#[test]
fn the_hello_of_a_box_that_just_booted_is_no_restart() {
    // The boot hello waiting on the port, and the first-contact one the handshake draws.
    let mock = MockBox::new();
    mock.restart();
    let device = Device::open_mock(mock.clone()).unwrap();
    device.press(Button::LEFT).unwrap();
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(device.counters().restarts, 0);
}

#[test]
fn a_hello_on_another_protocol_is_refused_until_the_box_is_back() {
    let mock = MockBox::new();
    let device = Device::open_mock(mock.clone()).unwrap();
    device.press(Button::LEFT).unwrap();
    let hello = |proto: u8| {
        let mut p = vec![Q_VERSION, proto, 3, 4, 3];
        p.extend_from_slice(&[0; 6]);
        encode(FrameType::Resp, 0, &p).unwrap()
    };
    let other = crate::PROTO_VER - 1;
    mock.push_raw(&hello(other));
    await_that(
        "the box was never refused",
        || matches!(device.press(Button::LEFT), Err(crate::Error::BadProtoVer { got }) if got == other),
    );
    assert!(matches!(
        device.query_version(),
        Err(crate::Error::BadProtoVer { .. })
    ));
    assert_eq!(device.counters().restarts, 0);
    // Back on this build's protocol, it is a box that booted.
    mock.push_raw(&hello(crate::PROTO_VER));
    await_restarts(&device, 1);
    device.press(Button::LEFT).unwrap();
}

#[test]
fn the_clip_reports_its_loss_until_reloaded_or_cleared() {
    let mock = MockBox::new();
    let device = Device::open_mock(mock.clone()).unwrap();
    let clip = device.clip();
    let mut one = ClipBuilder::new();
    one.move_by(1, 0);

    // Nothing appended, nothing lost.
    device.press(Button::LEFT).unwrap();
    mock.restart();
    await_restarts(&device, 1);
    assert!(!clip.lost());

    clip.append(&one).unwrap();
    assert!(!clip.lost());
    mock.restart();
    await_restarts(&device, 2);
    assert!(clip.lost());
    clip.append(&one).unwrap();
    assert!(!clip.lost(), "a reload answers it");

    mock.restart();
    await_restarts(&device, 3);
    assert!(clip.lost());
    clip.clear().unwrap();
    assert!(!clip.lost(), "so does a clear");
}

#[test]
fn a_device_chip_reboot_is_recovered_like_any_restart() {
    let mock = MockBox::new();
    let device = Device::open_mock(mock.clone()).unwrap();
    device.scale(Axis::Y, Direction::Both, 0).unwrap();
    mock.clear_recorded();
    device.reboot(RebootTarget::DeviceRun).unwrap();
    await_restarts(&device, 1);
    assert_eq!(sent(&mock, FrameType::Lock).len(), 1);
    assert!(
        device
            .query_locks()
            .unwrap()
            .scale_of(Axis::Y, Direction::Negative)
            == 0
    );
}

// A box power-cycled while the link was down answers the reconnect probe after its hello. Its clone
// is not up yet, so held state waits for the recovery.
#[test]
fn a_reconnect_that_finds_the_chip_rebooted_runs_the_recovery() {
    let device = Device::from_transport_with_cadence(
        Arc::new(crate::transport::mock::MockTransport::new()),
        Duration::from_secs(60),
    );
    device.scale(Axis::X, Direction::Both, 40).unwrap();
    let clip = device.clip();
    clip.set_loop(true).unwrap();
    let mut one = ClipBuilder::new();
    one.move_by(1, 0);
    clip.append(&one).unwrap();

    let rebooted = MockBox::new();
    rebooted.restart();
    device
        .link
        .adopt_reopened(vec![("/dev/ttyACM0".into(), rebooted.transport())])
        .unwrap();
    await_restarts(&device, 1);
    assert_eq!(device.counters().reconnects, 1);
    assert!(clip.lost());
    assert_eq!(
        sent(&rebooted, FrameType::ClipSet),
        vec![vec![CLIP_SET_LOOP, 1]]
    );
    assert!(
        device
            .query_locks()
            .unwrap()
            .scale_of(Axis::X, Direction::Positive)
            == 40
    );
}

#[test]
fn a_reconnect_to_a_box_that_kept_running_is_no_restart() {
    let device = Device::from_transport_with_cadence(
        Arc::new(crate::transport::mock::MockTransport::new()),
        Duration::from_secs(60),
    );
    device.scale(Axis::X, Direction::Both, 40).unwrap();
    let running = MockBox::new();
    device
        .link
        .adopt_reopened(vec![("/dev/ttyACM0".into(), running.transport())])
        .unwrap();
    // The held state goes straight back, with no wait for a clone.
    assert_eq!(sent(&running, FrameType::Lock).len(), 1);
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(device.counters().restarts, 0);
    assert_eq!(device.counters().reconnects, 1);
}

// A hello during the recovery's wait is the same boot's: the boot hello can trail the first-contact
// one and its reply.
#[test]
fn a_hello_while_the_clone_is_awaited_is_the_same_restart() {
    use crate::{Caps, MouseCaps};
    let caps = |n_hid| Caps {
        mouse: MouseCaps {
            n_buttons: 5,
            has_x: true,
            has_y: true,
            has_wheel: true,
            pan: false,
            has_report_id: false,
            n_hid,
        },
        ..Caps::default()
    };
    let mock = MockBox::new();
    let device = Device::open_mock(mock.clone()).unwrap();
    device.press(Button::LEFT).unwrap();
    let _ = mock.clone().with_caps(caps(0));
    mock.clear_recorded();
    mock.restart();
    let deadline = Instant::now() + Duration::from_secs(1);
    while sent(&mock, FrameType::Query).len() < 2 {
        assert!(
            Instant::now() < deadline,
            "the recovery never asked for the clone"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    let mut p = vec![Q_VERSION, crate::PROTO_VER, 0, 0, 0];
    p.extend_from_slice(&[0; 6]);
    mock.push_raw(&encode(FrameType::Resp, 0, &p).unwrap());
    std::thread::sleep(Duration::from_millis(100));
    let _ = mock.clone().with_caps(caps(1));
    await_restarts(&device, 1);
    std::thread::sleep(Duration::from_millis(150));
    assert_eq!(device.counters().restarts, 1);
}

// The box drops an append made before its clone is up, so one during the recovery's wait is lost;
// only a ring reported once the clone is up counts as kept.
#[test]
fn the_box_decides_whether_an_appended_clip_survived() {
    use crate::ClipStatus;
    for kept in [false, true] {
        let mock = MockBox::new();
        let device = Device::open_mock(mock.clone()).unwrap();
        let clip = device.clip();
        let mut one = ClipBuilder::new();
        one.move_by(1, 0);
        clip.append(&one).unwrap();
        mock.restart();
        // The booted chip has no clone yet, and drops the append.
        std::thread::sleep(Duration::from_millis(30));
        clip.append(&one).unwrap();
        assert!(!clip.lost(), "nothing is reported before the clone is up");
        if kept {
            mock.set_clip_status(ClipStatus {
                total: 5,
                ..ClipStatus::default()
            });
        }
        await_restarts(&device, 1);
        assert_eq!(clip.lost(), !kept, "kept={kept}");
    }
}

// Polls until `done` holds.
fn await_that(what: &str, done: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(3);
    while !done() {
        assert!(Instant::now() < deadline, "{what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

// A box serving a stored patch set, and a program holding one of every kind of session state.
fn holding_everything(mock: &MockBox) -> (Device, crate::ClipHandle, RewriteRule, ClipTrigger) {
    use crate::{Patch, PatchSection};
    let device = Device::open_mock(mock.clone()).unwrap();
    device
        .set_patch(&Patch::new(PatchSection::Device, 12, [0x00, 0x01]))
        .unwrap();
    device.apply_patch().unwrap();
    let clip = device.clip();
    device.press(Button::SIDE1).unwrap();
    device.scale(Axis::X, Direction::Both, 40).unwrap();
    // That apply presented the clone again: let its recovery finish before loading anything.
    await_that("the apply was never recovered", || {
        sent(mock, FrameType::Lock).len() == 2
    });
    let rule = RewriteRule::new(RewriteClass::Emit, 1, Direction::IN, RewriteAction::Drop);
    device.set_rewrite(&rule).unwrap();
    device
        .transform(&Transform::swap(Axis::X, Axis::Y))
        .unwrap();
    clip.set_retain(true).unwrap();
    let trigger = ClipTrigger::new(Button::SIDE2, Edge::Press, ClipAction::Toggle);
    clip.bind(trigger).unwrap();
    let mut one = ClipBuilder::new();
    one.move_by(1, 0);
    clip.append(&one).unwrap();
    assert!(!clip.lost());
    (device, clip, rule, trigger)
}

// Everything held went back to the clone presented after `sent`, the ring included in what is lost.
fn assert_everything_back(
    mock: &MockBox,
    device: &Device,
    clip: &crate::ClipHandle,
    rule: &RewriteRule,
    trigger: &ClipTrigger,
    sent: usize,
) {
    await_that("the clip was never reported lost", || clip.lost());
    let frames = mock.recorded_frames();
    let after = |ty: FrameType| frames[sent..].iter().filter(|f| f.ty == ty).count();
    assert_eq!(after(FrameType::Inject), 1);
    assert!(after(FrameType::ClipSet) >= 1 && after(FrameType::ClipTrigger) >= 1);
    assert!(
        device
            .query_locks()
            .unwrap()
            .scale_of(Axis::X, Direction::Positive)
            == 40
    );
    assert_eq!(device.query_rewrite_entry(0).unwrap(), *rule);
    assert_eq!(device.query_transforms().unwrap().entries.len(), 1);
    let binds = sent_after(mock, sent, FrameType::ClipTrigger);
    assert!(binds.contains(&crate::device::clip::bind_payload(trigger).to_vec()));
    assert_eq!(device.counters().restarts, 0, "a re-clone is no restart");
}

fn sent_after(mock: &MockBox, from: usize, ty: FrameType) -> Vec<Vec<u8>> {
    mock.recorded_frames()[from..]
        .iter()
        .filter(|f| f.ty == ty)
        .map(|f| f.payload.clone())
        .collect()
}

// The index of the one frame of `ty` recorded from `from` on.
fn at(mock: &MockBox, from: usize, ty: FrameType) -> usize {
    from + mock.recorded_frames()[from..]
        .iter()
        .position(|f| f.ty == ty)
        .expect("the frame went out")
}

// APPLY, CLEAR and the opt-in toggle re-present the clone, releasing the session like a replug, with
// no hello. The crate sees the game PC enumerate the new clone and re-sends what it holds after the
// frame that took the old clone down.
#[test]
fn a_clone_the_crate_presents_again_gets_back_what_it_held() {
    use crate::{Patch, PatchSection};
    for verb in ["apply", "clear", "opt-in off"] {
        let mock = MockBox::new().with_imperfect(true);
        let (device, clip, rule, trigger) = holding_everything(&mock);
        let from = mock.recorded();
        match verb {
            "apply" => {
                device
                    .set_patch(&Patch::new(PatchSection::Device, 12, [0x00, 0x02]))
                    .unwrap();
                device.apply_patch().unwrap();
            }
            "clear" => device.clear_patch().unwrap(),
            _ => device.allow_imperfect_clones(false).unwrap(),
        }
        // The frame that took the old clone down.
        let down = from
            + mock.recorded_frames()[from..]
                .iter()
                .rposition(|f| matches!(f.ty, FrameType::Patch | FrameType::Option))
                .expect("the verb went out");
        if verb == "opt-in off" {
            // The box drops the table with the opt-in, and so does the crate.
            await_that("the clip was never reported lost", || clip.lost());
            assert!(
                device
                    .query_locks()
                    .unwrap()
                    .scale_of(Axis::X, Direction::Positive)
                    == 40
            );
            assert!(at(&mock, down + 1, FrameType::Lock) > down);
            continue;
        }
        assert_everything_back(&mock, &device, &clip, &rule, &trigger, from);
        assert!(at(&mock, down + 1, FrameType::Lock) > down, "{verb}");
    }
}

// A re-presentation by another program's APPLY reaches the crate through the game PC's enumeration
// counters on the next keepalive tick.
#[test]
fn a_clone_presented_again_from_elsewhere_is_noticed_on_the_next_tick() {
    use crate::protocol::command::patch_apply_payload;
    use crate::{Patch, PatchSection};
    let mock = MockBox::new().with_imperfect(true);
    let (device, clip, rule, trigger) = holding_everything(&mock);
    device
        .set_patch(&Patch::new(PatchSection::Device, 12, [0x00, 0x02]))
        .unwrap();
    let from = mock.recorded();
    device
        .link
        .send(FrameType::Patch, &patch_apply_payload())
        .unwrap();
    assert_everything_back(&mock, &device, &clip, &rule, &trigger, from);
}

// A clone that stays put gets its state once; the keepalive's counter check re-sends nothing.
#[test]
fn a_clone_left_alone_is_not_recovered() {
    let mock = MockBox::new();
    let device = Device::open_mock(mock.clone()).unwrap();
    device.scale(Axis::X, Direction::Both, 40).unwrap();
    mock.clear_recorded();
    std::thread::sleep(Duration::from_millis(1200));
    assert!(sent(&mock, FrameType::Lock).is_empty());
    let looked = sent(&mock, FrameType::Query)
        .iter()
        .filter(|p| p[..] == [crate::protocol::opcode::Q_STATS])
        .count();
    assert!(looked >= 2, "the keepalive looked {looked} times");
}

// After a command that can re-present the clone, the keepalive checks every slice, so state returns
// well before its next tick.
#[test]
fn the_crate_watches_closely_after_a_command_that_presents_the_clone() {
    use crate::{Patch, PatchSection};
    let mock = MockBox::new().with_imperfect(true);
    let device = Device::from_transport_with_cadence(mock.transport(), Duration::from_secs(60));
    let stats = device.query_stats().unwrap();
    device.link.restart_watch().note_session(stats.session);
    device
        .set_patch(&Patch::new(PatchSection::Device, 12, [0x00, 0x01]))
        .unwrap();
    device.scale(Axis::X, Direction::Both, 40).unwrap();
    device.apply_patch().unwrap();
    await_that("the lock never went back", || {
        sent(&mock, FrameType::Lock).len() == 2
    });
    assert!(
        device
            .query_locks()
            .unwrap()
            .scale_of(Axis::X, Direction::Positive)
            == 40
    );
}

// A program holding a lock, a press and a loaded clip.
fn holding_a_lock(mock: &MockBox) -> (Device, crate::ClipHandle) {
    let device = Device::open_mock(mock.clone()).unwrap();
    let clip = device.clip();
    device.press(Button::SIDE1).unwrap();
    device.scale(Axis::X, Direction::Both, 40).unwrap();
    let mut one = ClipBuilder::new();
    one.move_by(1, 0);
    clip.append(&one).unwrap();
    (device, clip)
}

fn scale_x(device: &Device) -> i16 {
    device
        .query_locks()
        .unwrap()
        .scale_of(Axis::X, Direction::Positive)
}

// An inter-chip link drop, or a detach the device returns from within the grace, releases the
// session with no re-clone or hello. The box counts it and the crate re-sends at once, the clone
// being up.
#[test]
fn a_release_that_leaves_the_clone_up_is_recovered() {
    for event in ["link", "short detach"] {
        let mock = MockBox::new();
        let (device, clip) = holding_a_lock(&mock);
        let from = mock.recorded();
        match event {
            "link" => mock.link_lost(),
            _ => mock.detach(true),
        }
        assert_eq!(scale_x(&device), 100, "{event}: the box released the scale");
        await_that("the clip was never reported lost", || clip.lost());
        assert_eq!(scale_x(&device), 40, "{event}");
        assert_eq!(
            sent_after(&mock, from, FrameType::Inject),
            vec![vec![0, 3, 0, 1]]
        );
        assert_eq!(device.counters().restarts, 0);
    }
}

// A detach past the grace takes the clone down, and nothing can land until another device is
// cloned: the recovery waits, however long, then re-sends.
#[test]
fn a_detach_waits_for_the_next_clone() {
    let mock = MockBox::new();
    let (device, clip) = holding_a_lock(&mock);
    let from = mock.recorded();
    mock.detach(false);
    std::thread::sleep(Duration::from_millis(900));
    assert!(
        sent_after(&mock, from, FrameType::Lock).is_empty(),
        "sent to no clone"
    );
    assert!(!clip.lost(), "reported before a reload could be taken");
    mock.attach();
    await_that("the clip was never reported lost", || clip.lost());
    assert_eq!(scale_x(&device), 40);
}

// The crate's own RESET is counted by the box like any release, and what it released stays released.
#[test]
fn the_crates_own_reset_is_not_undone() {
    let mock = MockBox::new();
    let (device, clip) = holding_a_lock(&mock);
    device.reset().unwrap();
    let from = mock.recorded();
    std::thread::sleep(Duration::from_millis(1200));
    assert!(sent_after(&mock, from, FrameType::Lock).is_empty());
    assert!(sent_after(&mock, from, FrameType::Inject).is_empty());
    assert!(
        !clip.lost(),
        "the reset forgot the clip rather than losing it"
    );
}

// session_ctr.h: a release counts once, and only after a command that could have set something.
#[test]
fn the_mock_counts_a_release_only_after_a_command() {
    let mock = MockBox::new();
    // No keepalive tick, so no recovery re-send lands between the steps.
    let device = Device::from_transport_with_cadence(mock.transport(), Duration::from_secs(60));
    let session = || device.query_stats().unwrap().session;
    assert_eq!(session(), 0);
    mock.link_lost();
    assert_eq!(session(), 0, "nothing set, nothing to lose");
    device.scale(Axis::X, Direction::Both, 40).unwrap();
    mock.link_lost();
    mock.detach(true);
    assert_eq!(session(), 1, "one command, one release counted");
    // A detach counts at once; its teardown after the grace counts again only if a command came in
    // it, and a device returning to a torn-down clone starts a fresh one, releasing nothing.
    device.scale(Axis::X, Direction::Both, 40).unwrap();
    mock.detach(false);
    assert_eq!(session(), 2);
    std::thread::sleep(Duration::from_millis(300));
    mock.attach();
    assert_eq!(
        session(),
        2,
        "a quiet grace and a fresh clone count nothing"
    );
    device.scale(Axis::X, Direction::Both, 40).unwrap();
    mock.detach(false);
    device.scale(Axis::X, Direction::Both, 40).unwrap();
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(
        session(),
        4,
        "a command in the grace makes the teardown count"
    );
    // The device back inside the grace keeps the clone, and an identical re-attach counts nothing.
    mock.attach();
    device.scale(Axis::X, Direction::Both, 40).unwrap();
    mock.detach(false);
    mock.attach();
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(session(), 5);
    // A RESET counts what an earlier command set and leaves nothing marked. Sent bare, since
    // `reset()` first clears the catch table with its own command.
    let reset = || device.link.send(FrameType::Reset, &[0]).unwrap();
    reset();
    assert_eq!(session(), 5, "nothing set since the last count");
    device.scale(Axis::X, Direction::Both, 40).unwrap();
    reset();
    assert_eq!(session(), 6);
    mock.link_lost();
    assert_eq!(session(), 6);
    mock.restart();
    assert_eq!(session(), 0, "a boot starts it again");
    device.reboot(RebootTarget::DeviceRun).unwrap();
    mock.link_lost();
    assert_eq!(session(), 0, "and a commanded reboot boots clean");
}

// A release or unlock waits out a re-send in progress, so the re-send's stale press or lock cannot
// land after it.
#[test]
fn a_command_waits_for_a_re_send_in_progress() {
    for what in ["inject", "lock", "reset"] {
        let mock = MockBox::new();
        let device = Device::open_mock(mock.clone()).unwrap();
        let guard = device.link.reassert_guard();
        let from = mock.recorded();
        let d = device.clone();
        let user = std::thread::spawn(move || match what {
            "inject" => d.release(Button::LEFT),
            "lock" => d.unlock(Axis::X, Direction::Both),
            _ => d.reset(),
        });
        std::thread::sleep(Duration::from_millis(60));
        assert_eq!(mock.recorded(), from, "{what} went out under a re-send");
        drop(guard);
        user.join().unwrap().unwrap();
        assert!(mock.recorded() > from, "{what}");
    }
}

// A teardown between the re-send's counter reading and the re-send takes the re-send with it and
// counts nothing: the recovery sees the clone gone and waits for the next.
#[test]
fn a_re_send_that_lands_on_a_going_clone_is_sent_again() {
    let mock = MockBox::new();
    let (device, _clip) = holding_a_lock(&mock);
    mock.detach_torn_down_before_next_command();
    std::thread::sleep(Duration::from_millis(700));
    assert_eq!(scale_x(&device), 100, "the re-send went with the clone");
    mock.attach();
    await_that("the scale never went back", || scale_x(&device) == 40);
}

// The opt-in going off releases in two parts: rules at once, the clone on the box's next loop tick.
// The close watch stays open for the second, so the scale returns without waiting a tick.
#[test]
fn the_second_part_of_a_release_is_caught_by_the_close_watch() {
    use crate::{Patch, PatchSection};
    let mock = MockBox::new().with_imperfect(true);
    mock.set_represent_delay(Duration::from_millis(300));
    let device = Device::from_transport_with_cadence(mock.transport(), Duration::from_secs(60));
    let stats = device.query_stats().unwrap();
    device.link.restart_watch().note_session(stats.session);
    device
        .set_patch(&Patch::new(PatchSection::Device, 12, [0x00, 0x01]))
        .unwrap();
    device.apply_patch().unwrap();
    let rule = RewriteRule::new(RewriteClass::Emit, 1, Direction::IN, RewriteAction::Drop);
    device.set_rewrite(&rule).unwrap();
    device.scale(Axis::X, Direction::Both, 40).unwrap();
    await_that("the apply was never recovered", || {
        sent(&mock, FrameType::Lock).len() == 2
    });
    // Past that recovery's check that its re-send stuck.
    std::thread::sleep(Duration::from_millis(200));
    let presented = device.query_stats().unwrap().config_count;
    device.allow_imperfect_clones(false).unwrap();
    await_that("the clone was never presented again", || {
        device.query_stats().unwrap().config_count != presented
    });
    await_that("the scale never went back", || scale_x(&device) == 40);
}

// A boot while a recovery from a release is still waiting is a restart all the same.
#[test]
fn a_boot_during_another_recovery_is_counted() {
    let mock = MockBox::new();
    let (device, _clip) = holding_a_lock(&mock);
    mock.detach(false);
    std::thread::sleep(Duration::from_millis(700));
    mock.restart();
    await_restarts(&device, 1);
    assert_eq!(scale_x(&device), 40);
}

// A clip the box had emptied (a streaming clip stopped or played out) is no loss at a later session
// release.
#[test]
fn a_ring_the_box_had_emptied_is_not_lost() {
    use crate::ClipStatus;
    let mock = MockBox::new();
    let (device, clip) = holding_a_lock(&mock);
    mock.set_clip_status(ClipStatus::default());
    // A keepalive tick reads the ring.
    std::thread::sleep(Duration::from_millis(700));
    let from = mock.recorded();
    mock.link_lost();
    await_that("the scale never went back", || scale_x(&device) == 40);
    await_that("the recovery never finished", || {
        sent_after(&mock, from, FrameType::Query)
            .iter()
            .filter(|p| p[..] == [crate::protocol::opcode::Q_CLIP])
            .count()
            >= 1
    });
    std::thread::sleep(Duration::from_millis(50));
    assert!(!clip.lost());
}

// Rules and packet triggers return in the box's order, the tie-break between equally specific
// entries: a changed rule moves to the end, a changed packet trigger keeps its place.
#[test]
fn a_re_send_rebuilds_the_tables_in_the_box_s_order() {
    let mock = MockBox::new().with_imperfect(true);
    let device = Device::open_mock(mock.clone()).unwrap();
    let rule = |id: u16, payload: u8| {
        RewriteRule::new(
            RewriteClass::Emit,
            id,
            Direction::IN,
            RewriteAction::Replace,
        )
        .with_payload(vec![payload])
    };
    for id in [5, 1, 3] {
        device.set_rewrite(&rule(id, 0)).unwrap();
    }
    device.set_rewrite(&rule(1, 1)).unwrap();
    device.set_rewrite(&rule(5, 0)).unwrap();
    let clip = device.clip();
    let packet = |id: u16, action: ClipAction| {
        ClipPacketTrigger::new(TrafficClass::HidIn, id, Direction::IN, action)
    };
    for id in [7, 2] {
        clip.bind_packet(&packet(id, ClipAction::Start)).unwrap();
    }
    clip.bind_packet(&packet(7, ClipAction::Stop)).unwrap();
    let ids = |d: &Device| -> Vec<u16> {
        d.query_rewrite()
            .unwrap()
            .entries
            .iter()
            .map(|e| e.id)
            .collect()
    };
    let triggers = |c: &crate::ClipHandle| -> Vec<u16> {
        c.query_config()
            .unwrap()
            .packet_triggers
            .iter()
            .map(|e| e.trigger.id)
            .collect()
    };
    let (rules_before, triggers_before) = (ids(&device), triggers(&clip));
    assert_eq!(rules_before, vec![5, 3, 1]);
    assert_eq!(triggers_before, vec![7, 2]);
    mock.restart();
    await_restarts(&device, 1);
    assert_eq!(ids(&device), rules_before);
    assert_eq!(triggers(&clip), triggers_before);
}

// A recovery waiting on a box that no longer answers does not hold up dropping the device.
#[test]
fn dropping_the_device_does_not_wait_out_a_recovery() {
    let mock = MockBox::new();
    let device = Device::open_mock(mock.clone()).unwrap();
    device.scale(Axis::X, Direction::Both, 40).unwrap();
    let _ = mock.clone().silent();
    let mut p = vec![Q_VERSION, crate::PROTO_VER, 0, 0, 0];
    p.extend_from_slice(&[0; 6]);
    mock.push_raw(&encode(FrameType::Resp, 0, &p).unwrap());
    std::thread::sleep(Duration::from_millis(120));
    let start = Instant::now();
    drop(device);
    assert!(
        start.elapsed() < Duration::from_millis(120),
        "{:?}",
        start.elapsed()
    );
}

// A presentation requested inside a detach's grace waits for its end, and happens only if the device
// came back.
#[test]
fn the_mock_holds_a_presentation_through_a_detach_s_grace() {
    use crate::{Patch, PatchSection};
    for back in [false, true] {
        let mock = MockBox::new().with_imperfect(true);
        let device = Device::from_transport_with_cadence(mock.transport(), Duration::from_secs(60));
        device
            .set_patch(&Patch::new(PatchSection::Device, 12, [0x00, 0x01]))
            .unwrap();
        mock.detach(false);
        device.apply_patch().unwrap();
        assert!(
            !device.query_patches().unwrap().applied,
            "held in the grace"
        );
        if back {
            mock.attach();
        }
        std::thread::sleep(Duration::from_millis(300));
        assert_eq!(device.query_patches().unwrap().applied, back, "back={back}");
    }
}

// A ring read overtaken by a release shows it empty with nothing reported yet; that release's
// recovery reports the clip lost.
#[test]
fn a_ring_read_across_a_release_is_left_to_its_recovery() {
    let mock = MockBox::new();
    let (device, clip) = holding_a_lock(&mock);
    // A tick reads the ring while nothing has happened to it.
    std::thread::sleep(Duration::from_millis(700));
    assert!(!clip.lost());
    mock.link_lost_before_next_clip_query();
    await_that("the clip was never reported lost", || clip.lost());
    assert_eq!(scale_x(&device), 40);
}
