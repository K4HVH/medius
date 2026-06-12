//! Tests for the paced [`MovementSession`](super::MovementSession).
//!
//! The per-tick *decision* logic (coalescing / carry / idle / velocity) is tested **directly** on the
//! pure [`Accumulator::tick_emit`](super::Accumulator) — no real-time thread, no wall-clock — so it is
//! fully deterministic. One short, tolerant integration test exercises the actual `medius-pacer`
//! thread against the mock to prove the wiring end to end.

use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::Device;
use crate::protocol::{DecodedFrame, FrameDecoder, FrameType};
use crate::transport::mock::MockTransport;

use super::Accumulator;

// ---- Pure tick_emit tests (deterministic, no thread) ----------------------------------------------

#[test]
fn idle_tick_emits_nothing() {
    let mut acc = Accumulator::default();
    assert_eq!(acc.tick_emit(), None);
    // Still nothing on a subsequent idle tick.
    assert_eq!(acc.tick_emit(), None);
}

#[test]
fn many_pushes_in_one_window_coalesce_into_one_move() {
    let mut acc = Accumulator::default();
    acc.push(3, -1);
    acc.push(4, 0);
    acc.push(-2, 5);
    // All three pushes landed before this tick → one MOVE of the sum.
    assert_eq!(acc.tick_emit(), Some((5, 4)));
    // Drained: the next tick is idle.
    assert_eq!(acc.tick_emit(), None);
}

#[test]
fn total_emitted_equals_total_pushed_within_i16() {
    let mut acc = Accumulator::default();
    let pushes = [(10, -20), (-5, 7), (100, 3), (-50, -50)];
    let (mut sum_x, mut sum_y) = (0i32, 0i32);
    for &(dx, dy) in &pushes {
        acc.push(dx, dy);
        sum_x += dx as i32;
        sum_y += dy as i32;
    }
    let (ex, ey) = acc.tick_emit().expect("non-zero");
    assert_eq!((ex as i32, ey as i32), (sum_x, sum_y));
}

#[test]
fn oversized_burst_is_paced_across_ticks_with_carry() {
    // Push more than fits in one i16 field; it must be split across ticks at the wire limit and the
    // TOTAL must be preserved exactly (carry retained in the accumulator).
    let mut acc = Accumulator::default();
    let big_x: i32 = 80_000; // > i16::MAX (32767) → needs ≥3 ticks
    let big_y: i32 = -50_000; // < i16::MIN (-32768)
    // Apply via several i16 pushes summing to the big totals.
    for chunk in [30_000i16, 30_000, 20_000] {
        acc.push(chunk, 0);
    }
    for chunk in [-30_000i16, -20_000] {
        acc.push(0, chunk);
    }

    let mut total_x: i32 = 0;
    let mut total_y: i32 = 0;
    let mut ticks = 0;
    // Drain to idle.
    while let Some((dx, dy)) = acc.tick_emit() {
        // No single emitted field ever exceeds the i16 range (it is an i16 by construction).
        total_x += dx as i32;
        total_y += dy as i32;
        ticks += 1;
        assert!(ticks < 100, "should drain in a bounded number of ticks");
    }
    assert_eq!(total_x, big_x, "X total preserved exactly across the carry");
    assert_eq!(total_y, big_y, "Y total preserved exactly across the carry");
    assert!(ticks >= 3, "an >i16 burst must take multiple ticks (paced)");
}

#[test]
fn first_emitted_field_is_clamped_to_i16_max() {
    let mut acc = Accumulator::default();
    acc.push(30_000, 0);
    acc.push(30_000, 0); // sum 60_000 > i16::MAX
    let (dx, _) = acc.tick_emit().unwrap();
    assert_eq!(dx, i16::MAX, "first tick carries exactly the i16 ceiling");
    // Remainder on the next tick.
    let (dx2, _) = acc.tick_emit().unwrap();
    assert_eq!(dx2 as i32, 60_000 - i16::MAX as i32);
}

#[test]
fn velocity_emits_every_tick() {
    let mut acc = Accumulator::default();
    acc.set_velocity(2, -3);
    // Each tick emits the velocity, indefinitely.
    for _ in 0..5 {
        assert_eq!(acc.tick_emit(), Some((2, -3)));
    }
    // Clearing it returns to idle.
    acc.clear_velocity();
    assert_eq!(acc.tick_emit(), None);
}

#[test]
fn velocity_and_push_combine_additively_in_one_tick() {
    let mut acc = Accumulator::default();
    acc.set_velocity(10, 10);
    acc.push(5, -4);
    // Velocity folded in + the push → one combined MOVE.
    assert_eq!(acc.tick_emit(), Some((15, 6)));
    // Next tick: only the velocity remains (push was one-shot).
    assert_eq!(acc.tick_emit(), Some((10, 10)));
}

#[test]
fn zero_velocity_with_no_push_is_idle() {
    let mut acc = Accumulator::default();
    acc.set_velocity(0, 0);
    assert_eq!(acc.tick_emit(), None);
}

// ---- Integration test: the real medius-pacer thread against the mock ------------------------------

fn decode_moves(mock: &MockTransport) -> Vec<DecodedFrame> {
    FrameDecoder::new()
        .feed_collect(&mock.written())
        .into_iter()
        .filter(|f| f.ty == FrameType::Move)
        .collect()
}

/// A short, tolerant end-to-end run: spin the real pacer thread at a high rate, push deltas, and
/// assert at least one MOVE frame reached the mock; then drop the session and assert it stopped.
#[test]
fn pacer_thread_emits_moves_then_stops_on_drop() {
    let mock = Arc::new(MockTransport::new());
    let device = Device::from_transport(mock.clone());

    // High rate so several ticks land inside the short window.
    let session = device.movement_at(2000);
    // Constant velocity guarantees an emission every tick regardless of push timing.
    session.set_velocity(1, 0);

    // Run for ~30 ms.
    std::thread::sleep(Duration::from_millis(30));

    let moves = decode_moves(&mock);
    assert!(
        !moves.is_empty(),
        "the pacer thread should have emitted at least one MOVE in 30ms"
    );
    // Each emitted MOVE is the velocity (1, 0).
    assert!(moves.iter().all(|f| f.payload == vec![1, 0, 0, 0]));

    // Drop stops and joins the pacer thread.
    drop(session);
    let _ = mock.written(); // clear anything emitted up to the drop

    // After the drop, no further MOVE frames appear.
    std::thread::sleep(Duration::from_millis(20));
    assert!(
        decode_moves(&mock).is_empty(),
        "no MOVE frames must be emitted after the session is dropped"
    );

    // The device itself still works (the session held only a clone).
    device.move_rel(7, 7).unwrap();
    let after = decode_moves(&mock);
    assert_eq!(after.len(), 1);
    assert_eq!(after[0].payload, vec![7, 0, 7, 0]);
}

/// Pushing deltas (no velocity) is paced out as MOVE frames whose total equals what was pushed.
#[test]
fn pushed_deltas_are_paced_and_total_preserved() {
    let mock = Arc::new(MockTransport::new());
    let device = Device::from_transport(mock.clone());
    let session = device.movement_at(2000);

    // Push a known total.
    let pushes: [(i16, i16); 4] = [(10, 0), (0, 10), (-3, 4), (5, -5)];
    let (mut tx, mut ty) = (0i32, 0i32);
    for &(dx, dy) in &pushes {
        session.push(dx, dy);
        tx += dx as i32;
        ty += dy as i32;
    }

    std::thread::sleep(Duration::from_millis(30));
    drop(session);

    let moves = decode_moves(&mock);
    assert!(!moves.is_empty(), "pushes should have produced MOVE frames");
    let (mut sx, mut sy) = (0i32, 0i32);
    for f in &moves {
        let dx = i16::from_le_bytes([f.payload[0], f.payload[1]]) as i32;
        let dy = i16::from_le_bytes([f.payload[2], f.payload[3]]) as i32;
        sx += dx;
        sy += dy;
    }
    assert_eq!((sx, sy), (tx, ty), "paced MOVE total equals total pushed");
}

#[test]
fn set_rate_changes_the_reported_rate() {
    let mock = Arc::new(MockTransport::new());
    let device = Device::from_transport(mock.clone());
    let session = device.movement(); // default 1 kHz
    assert_eq!(session.rate(), super::DEFAULT_RATE_HZ);
    session.set_rate(500);
    assert_eq!(session.rate(), 500);
}

/// With the `metrics` feature on, a brief run populates the stats.
#[cfg(feature = "metrics")]
#[test]
fn metrics_populate_after_running() {
    let mock = Arc::new(MockTransport::new());
    let device = Device::from_transport(mock.clone());
    let session = device.movement_at(2000);
    session.set_velocity(1, 0); // emit every tick → write-latency samples too

    std::thread::sleep(Duration::from_millis(30));
    let stats = session.stats();
    assert!(stats.ticks > 0, "the pacer should have recorded ticks");
    assert!(
        !stats.jitter.is_empty(),
        "jitter histogram should have samples after several ticks"
    );
    assert!(
        !stats.write_latency.is_empty(),
        "write-latency histogram should have samples (velocity emits every tick)"
    );
    drop(session);
}

/// A pacer with no activity still ticks (idle), emitting no MOVE frames but running cleanly.
#[test]
fn idle_pacer_emits_no_moves() {
    let start = Instant::now();
    let mock = Arc::new(MockTransport::new());
    let device = Device::from_transport(mock.clone());
    let session = device.movement_at(2000);

    std::thread::sleep(Duration::from_millis(20));
    drop(session);

    assert!(
        decode_moves(&mock).is_empty(),
        "an idle pacer must emit no MOVE frames (firmware frame clock handles stillness)"
    );
    // sanity: the test itself didn't hang.
    assert!(start.elapsed() < Duration::from_secs(5));
}

/// `MovementSession` is `Send + Sync` (a handle on the shared pacer state; callers may move it across
/// threads). Mirrors the guard tests for `Device`/`MockTransport`/`Counters`.
#[test]
fn movement_session_is_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<super::MovementSession>();
}
