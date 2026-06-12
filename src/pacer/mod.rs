//! The paced movement session — the headline 1 kHz frame pacer (§7 of the design spec).
//!
//! [`MovementSession`] is the whole reason this library exists over the Python reference client: a
//! dedicated real-time thread (named `medius-pacer`) that clocks **frame emission** at a fixed rate
//! (default 1 kHz) on a precise absolute-deadline clock. Each tick it drains a shared delta
//! accumulator and emits **at most one** `MOVE` frame for that window.
//!
//! ## It paces frames — it never invents motion
//!
//! There is **no humanization** anywhere in here. The session does not interpolate, ease, smooth, or
//! synthesize intermediate points; it only *clocks when frames go out* and *splits an oversized burst
//! across ticks at the wire field limit*. The firmware owns the real motion semantics — additive
//! "no-halving" merge, descriptor-clamped carry-remainder so a `MOVE 2000` lands as exactly 2000.
//! The session never re-implements any of that.
//!
//! ## What "carry" means here (wire-field pacing, not trajectory synthesis)
//!
//! The shared accumulator is an `i32` per axis, so many [`push`](MovementSession::push)es landing in
//! one tick window sum without overflow. Each tick the accumulator is drained, but the emitted `MOVE`
//! carries only what fits in an `i16` wire field; any beyond-`i16` remainder **stays in the
//! accumulator** for the next tick. So total motion is preserved exactly and an oversized burst is
//! *paced across ticks at the wire field limit* — this is wire-field pacing, not a trajectory the host
//! made up. (The firmware additionally carries against the mouse's native descriptor field width;
//! that is a separate, finer carry the host does not duplicate.)
//!
//! ## Velocity mode
//!
//! [`set_velocity`](MovementSession::set_velocity) emits a constant `(vx, vy)` **every tick** until
//! changed or [`clear_velocity`](MovementSession::clear_velocity)ed. It combines additively with
//! pushes: each tick the velocity is added into the accumulator *before* draining, so a push and a
//! velocity in the same window sum into one `MOVE`, and both flow through the same `i16` carry. With a
//! zero velocity and no pushes, a tick drains to zero and emits **nothing** (the firmware frame clock
//! handles stillness — §5.3).
//!
//! ## Thread lifecycle (stop/join, no cycle)
//!
//! The session holds a [`Device`] clone, the shared accumulator, a stop flag, and the pacer thread's
//! [`JoinHandle`]. It is **not** stored inside the device's `Inner`, so there is no reference cycle —
//! exactly the anti-cycle discipline the device threads use. `Drop` sets the stop flag and joins the
//! pacer thread; residual deltas are **not** force-flushed (fire-and-go, §7).

pub(crate) mod clock;

#[cfg(test)]
mod tests;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

use parking_lot::Mutex;

use crate::Device;

use clock::PrecisePacer;

#[cfg(feature = "metrics")]
use crate::pacer::metrics::{MetricsState, PacerStats};

#[cfg(feature = "metrics")]
pub(crate) mod metrics;

/// Default pacer rate (Hz) — the headline 1 kHz frame cadence.
pub const DEFAULT_RATE_HZ: u32 = 1000;

/// Convert a rate in Hz to a tick period. A zero rate is treated as 1 Hz (the clock further clamps a
/// zero period), so the pacer never divides by zero or spins on a 0 ns grid.
fn rate_to_period(hz: u32) -> Duration {
    let hz = hz.max(1);
    Duration::from_nanos(1_000_000_000u64 / hz as u64)
}

/// The shared per-tick movement state: an `i32` push accumulator (so many pushes in one window can't
/// overflow) plus the current constant velocity.
///
/// This is the **pure**, thread-free heart of the pacer. [`tick_emit`](Accumulator::tick_emit)
/// computes one tick's emission decision with no clock and no I/O, so the coalescing / carry / idle /
/// velocity logic is unit-tested directly without the real-time thread.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Accumulator {
    /// Pending relative X (pushes + carried `i16` remainder), drained each tick.
    acc_x: i32,
    /// Pending relative Y.
    acc_y: i32,
    /// Constant per-tick velocity X, added into the accumulator every tick until changed/cleared.
    vel_x: i16,
    /// Constant per-tick velocity Y.
    vel_y: i16,
}

impl Accumulator {
    /// Add a relative delta into the push accumulator (saturating, though `i32` headroom over `i16`
    /// inputs means realistic push streams never reach the bound).
    fn push(&mut self, dx: i16, dy: i16) {
        self.acc_x = self.acc_x.saturating_add(dx as i32);
        self.acc_y = self.acc_y.saturating_add(dy as i32);
    }

    /// Set the constant per-tick velocity.
    fn set_velocity(&mut self, vx: i16, vy: i16) {
        self.vel_x = vx;
        self.vel_y = vy;
    }

    /// Clear the constant velocity (back to push-only).
    fn clear_velocity(&mut self) {
        self.vel_x = 0;
        self.vel_y = 0;
    }

    /// Compute one tick's emission and update the accumulator — the pure tick decision.
    ///
    /// Steps, in order:
    /// 1. Fold the constant velocity into the accumulator (so velocity emits each tick and combines
    ///    additively with pushes through the same carry).
    /// 2. If the accumulator is now zero on **both** axes, emit **nothing** (`None`) — an idle tick
    ///    sends no frame; the firmware frame clock handles stillness (§5.3).
    /// 3. Otherwise clamp each axis to the `i16` wire-field range, **retain the beyond-`i16`
    ///    remainder** in the accumulator, and return the clamped `(dx, dy)` to emit as one `MOVE`.
    ///
    /// Because the remainder is retained, the sum of all emitted deltas equals the total pushed (+
    /// velocity per tick) exactly — an oversized burst is paced across ticks at the wire field limit.
    fn tick_emit(&mut self) -> Option<(i16, i16)> {
        // 1. Fold velocity in (saturating into the i32 accumulator).
        self.acc_x = self.acc_x.saturating_add(self.vel_x as i32);
        self.acc_y = self.acc_y.saturating_add(self.vel_y as i32);

        // 2. Idle tick → emit nothing.
        if self.acc_x == 0 && self.acc_y == 0 {
            return None;
        }

        // 3. Clamp to i16, retaining the remainder for the next tick.
        let dx = self.acc_x.clamp(i16::MIN as i32, i16::MAX as i32);
        let dy = self.acc_y.clamp(i16::MIN as i32, i16::MAX as i32);
        self.acc_x -= dx;
        self.acc_y -= dy;
        Some((dx as i16, dy as i16))
    }
}

/// Shared state between the public [`MovementSession`] handle and its pacer thread.
///
/// Held behind an [`Arc`] so the thread and the handle reference the same accumulator. The session
/// handle does **not** live in the device's `Inner`, so this `Arc` forms no cycle with the device.
#[derive(Debug)]
struct Shared {
    /// The push/velocity accumulator (the pure [`Accumulator`]), mutated by `push`/`set_velocity`
    /// from any thread and drained by the pacer thread each tick.
    acc: Mutex<Accumulator>,
    /// Tick period in nanoseconds; the pacer thread reads it each tick so `set_rate` retunes live.
    period_ns: AtomicU64,
    /// Set on drop; the pacer thread observes it and exits.
    stop: AtomicBool,
}

/// A paced movement session over a [`Device`] — the headline 1 kHz frame pacer.
///
/// Created by [`Device::movement`]. Spawns a dedicated real-time thread (`medius-pacer`) that clocks
/// frame emission on a precise absolute-deadline clock. See the [module docs](self) for the
/// no-humanization guarantee, the wire-field carry, velocity mode, and the stop/join lifecycle.
#[derive(Debug)]
pub struct MovementSession {
    shared: Arc<Shared>,
    pacer: Option<JoinHandle<()>>,
    #[cfg(feature = "metrics")]
    metrics: Arc<MetricsState>,
}

impl MovementSession {
    /// Spawn the pacer thread at `rate_hz` over a clone of `device`. Internal — use
    /// [`Device::movement`] / [`Device::movement_at`].
    fn spawn(device: Device, rate_hz: u32) -> MovementSession {
        let shared = Arc::new(Shared {
            acc: Mutex::new(Accumulator::default()),
            period_ns: AtomicU64::new(rate_to_period(rate_hz).as_nanos() as u64),
            stop: AtomicBool::new(false),
        });

        #[cfg(feature = "metrics")]
        let metrics = Arc::new(MetricsState::new());

        let thread_shared = Arc::clone(&shared);
        let thread_device = device;
        #[cfg(feature = "metrics")]
        let thread_metrics = Arc::clone(&metrics);

        let pacer = std::thread::Builder::new()
            .name("medius-pacer".into())
            .spawn(move || {
                pacer_loop(
                    &thread_device,
                    &thread_shared,
                    #[cfg(feature = "metrics")]
                    &thread_metrics,
                )
            })
            .expect("spawn medius-pacer thread");

        MovementSession {
            shared,
            pacer: Some(pacer),
            #[cfg(feature = "metrics")]
            metrics,
        }
    }

    /// Accumulate a relative delta into the shared accumulator (it is *not* sent immediately — the
    /// next tick drains and emits it). Many pushes within one tick window coalesce into a single
    /// `MOVE` of their sum.
    pub fn push(&self, dx: i16, dy: i16) {
        self.shared.acc.lock().push(dx, dy);
    }

    /// Set a constant per-tick velocity: `(vx, vy)` is emitted **every tick** (combined additively
    /// with any pushes) until changed or [`clear_velocity`](Self::clear_velocity)ed.
    pub fn set_velocity(&self, vx: i16, vy: i16) {
        self.shared.acc.lock().set_velocity(vx, vy);
    }

    /// Clear the constant velocity (back to push-only). Any already-accumulated pushes still drain.
    pub fn clear_velocity(&self) {
        self.shared.acc.lock().clear_velocity();
    }

    /// Change the tick rate in Hz (default [`DEFAULT_RATE_HZ`]). Takes effect on the next tick; the
    /// absolute-deadline grid is retuned without resetting (the clock advances by the new period).
    pub fn set_rate(&self, hz: u32) {
        self.shared
            .period_ns
            .store(rate_to_period(hz).as_nanos() as u64, Ordering::Relaxed);
    }

    /// The current tick rate in Hz.
    pub fn rate(&self) -> u32 {
        let ns = self.shared.period_ns.load(Ordering::Relaxed).max(1);
        (1_000_000_000u64 / ns) as u32
    }

    /// A snapshot of the pacer metrics (tick count, late ticks, jitter + write-latency histograms).
    ///
    /// Only available with the `metrics` feature; when the feature is off the pacer records nothing
    /// (zero-cost — no atomics, no branches in the hot loop).
    #[cfg(feature = "metrics")]
    pub fn stats(&self) -> PacerStats {
        self.metrics.snapshot()
    }
}

impl Drop for MovementSession {
    fn drop(&mut self) {
        // Signal stop and join the pacer thread. The pacer checks `stop` once per tick (≤ one period,
        // ≈1 ms at the default rate), so this never hangs. Residual deltas are NOT force-flushed
        // (fire-and-go, §7).
        self.shared.stop.store(true, Ordering::SeqCst);
        if let Some(h) = self.pacer.take() {
            let _ = h.join();
        }
    }
}

/// The pacer thread body: loop on the precise clock, draining the accumulator each tick and emitting
/// at most one `MOVE`. Factored out so it reads top-to-bottom; the per-tick *decision* lives in the
/// pure [`Accumulator::tick_emit`] (unit-tested without this thread).
fn pacer_loop(
    device: &Device,
    shared: &Shared,
    #[cfg(feature = "metrics")] metrics: &MetricsState,
) {
    let mut pacer = PrecisePacer::new(Duration::from_nanos(
        shared.period_ns.load(Ordering::Relaxed),
    ));

    loop {
        pacer.wait_next_tick();

        if shared.stop.load(Ordering::SeqCst) {
            return;
        }

        // Live rate change: re-read the period and retune the clock if it changed.
        let period = Duration::from_nanos(shared.period_ns.load(Ordering::Relaxed));
        if period != pacer.period() {
            pacer.set_period(period);
        }

        // `metrics` records the realized inter-tick interval vs the ideal period (jitter / late
        // ticks); compiled out entirely when the feature is off.
        #[cfg(feature = "metrics")]
        metrics.record_tick(period);

        // Drain the accumulator (pure decision) under the lock, then release before sending.
        let to_emit = shared.acc.lock().tick_emit();

        if let Some((dx, dy)) = to_emit {
            #[cfg(feature = "metrics")]
            let write_start = std::time::Instant::now();

            // One MOVE per tick (fire-and-go). A send error (port gone) is ignored — the next tick
            // retries, matching the device layer's fire-and-go model; reconnect heals a dead port.
            let _ = device.move_rel(dx, dy);

            #[cfg(feature = "metrics")]
            metrics.record_write_latency(write_start.elapsed());
        }
    }
}

impl Device {
    /// Open a [`MovementSession`] at the default rate ([`DEFAULT_RATE_HZ`], 1 kHz).
    ///
    /// Spawns a dedicated real-time pacer thread that clocks `MOVE` emission; [`push`] deltas into it
    /// and they are paced out one `MOVE` per tick. Drop the session to stop and join the thread.
    ///
    /// [`push`]: MovementSession::push
    pub fn movement(&self) -> MovementSession {
        MovementSession::spawn(self.clone(), DEFAULT_RATE_HZ)
    }

    /// Open a [`MovementSession`] at an explicit rate in Hz (see [`movement`](Self::movement)).
    pub fn movement_at(&self, rate_hz: u32) -> MovementSession {
        MovementSession::spawn(self.clone(), rate_hz)
    }
}
