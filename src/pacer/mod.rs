//! The paced movement session — the headline 1 kHz frame pacer (§7 of the design spec).
//!
//! [`MovementSession`] runs a dedicated real-time thread (`medius-pacer`) that clocks **frame
//! emission** at a fixed rate (default 1 kHz) on a precise absolute-deadline clock. Each tick it
//! drains a shared delta accumulator and emits **at most one** `MOVE` for that window.
//!
//! It paces frames; it never invents motion. No humanization, interpolation, or easing — it only
//! clocks when frames go out and splits an oversized burst across ticks at the wire field limit. The
//! firmware owns motion semantics (additive no-halving merge, descriptor-clamped carry-remainder).
//!
//! The `i32`-per-axis accumulator lets many [`push`](MovementSession::push)es in one window sum
//! without overflow. Each tick drains it, but the emitted `MOVE` carries only what fits in an `i16`
//! wire field; the beyond-`i16` remainder stays in the accumulator for the next tick, so total motion
//! is preserved exactly. (The firmware's separate, finer carry against the mouse's native descriptor
//! width is not duplicated here.)
//!
//! [`set_velocity`](MovementSession::set_velocity) folds a constant `(vx, vy)` into the accumulator
//! *before* draining every tick, so it combines additively with pushes through the same carry until
//! changed or [`clear_velocity`](MovementSession::clear_velocity)ed. Zero velocity + no pushes drains
//! to zero and emits nothing — the firmware frame clock handles stillness (§5.3).
//!
//! The session holds a [`Device`] clone and is **not** stored in the device's `Inner`, so there is no
//! reference cycle. `Drop` sets the stop flag and joins the thread; residual deltas are not
//! force-flushed (fire-and-go, §7).

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

/// Rate in Hz to tick period. A zero rate is treated as 1 Hz so we never divide by zero.
fn rate_to_period(hz: u32) -> Duration {
    let hz = hz.max(1);
    Duration::from_nanos(1_000_000_000u64 / hz as u64)
}

/// The shared per-tick movement state: an `i32` push accumulator plus the current constant velocity.
///
/// The pure, thread-free heart of the pacer: [`tick_emit`](Accumulator::tick_emit) computes one tick's
/// emission with no clock or I/O, so coalescing / carry / idle / velocity is unit-tested directly.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Accumulator {
    /// Pending relative X (pushes + carried `i16` remainder), drained each tick.
    acc_x: i32,
    acc_y: i32,
    /// Constant per-tick velocity X, folded into the accumulator every tick until changed/cleared.
    vel_x: i16,
    vel_y: i16,
}

impl Accumulator {
    /// Add a relative delta (saturating; `i32` headroom means realistic streams never reach the bound).
    fn push(&mut self, dx: i16, dy: i16) {
        self.acc_x = self.acc_x.saturating_add(dx as i32);
        self.acc_y = self.acc_y.saturating_add(dy as i32);
    }

    fn set_velocity(&mut self, vx: i16, vy: i16) {
        self.vel_x = vx;
        self.vel_y = vy;
    }

    fn clear_velocity(&mut self) {
        self.vel_x = 0;
        self.vel_y = 0;
    }

    /// One tick's emission decision: fold velocity in, emit `None` if both axes are now zero (idle tick
    /// → no frame, firmware handles stillness §5.3), else clamp each axis to the `i16` wire field and
    /// retain the remainder for the next tick. The retained remainder makes the emitted total exact.
    fn tick_emit(&mut self) -> Option<(i16, i16)> {
        self.acc_x = self.acc_x.saturating_add(self.vel_x as i32);
        self.acc_y = self.acc_y.saturating_add(self.vel_y as i32);

        if self.acc_x == 0 && self.acc_y == 0 {
            return None;
        }

        let dx = self.acc_x.clamp(i16::MIN as i32, i16::MAX as i32);
        let dy = self.acc_y.clamp(i16::MIN as i32, i16::MAX as i32);
        self.acc_x -= dx;
        self.acc_y -= dy;
        Some((dx as i16, dy as i16))
    }
}

/// Shared state between the [`MovementSession`] handle and its pacer thread (behind an [`Arc`]; forms
/// no cycle since the session is not stored in the device's `Inner`).
#[derive(Debug)]
struct Shared {
    acc: Mutex<Accumulator>,
    /// Tick period in nanoseconds; re-read each tick so `set_rate` retunes live.
    period_ns: AtomicU64,
    /// Set on drop; the pacer thread observes it and exits.
    stop: AtomicBool,
}

/// A paced movement session over a [`Device`] — the headline 1 kHz frame pacer.
///
/// Created by [`Device::movement`]. See the [module docs](self) for the no-humanization guarantee, the
/// wire-field carry, velocity mode, and the stop/join lifecycle.
#[derive(Debug)]
pub struct MovementSession {
    shared: Arc<Shared>,
    pacer: Option<JoinHandle<()>>,
    #[cfg(feature = "metrics")]
    metrics: Arc<MetricsState>,
}

impl MovementSession {
    /// Spawn the pacer thread at `rate_hz` over a clone of `device`.
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

    /// Accumulate a relative delta; the next tick drains and emits it. Pushes within one tick window
    /// coalesce into a single `MOVE` of their sum.
    pub fn push(&self, dx: i16, dy: i16) {
        self.shared.acc.lock().push(dx, dy);
    }

    /// Set a constant per-tick velocity: `(vx, vy)` is emitted every tick (combined additively with
    /// pushes) until changed or [`clear_velocity`](Self::clear_velocity)ed.
    pub fn set_velocity(&self, vx: i16, vy: i16) {
        self.shared.acc.lock().set_velocity(vx, vy);
    }

    /// Clear the constant velocity (back to push-only). Already-accumulated pushes still drain.
    pub fn clear_velocity(&self) {
        self.shared.acc.lock().clear_velocity();
    }

    /// Change the tick rate in Hz. Takes effect next tick; the absolute-deadline grid retunes without
    /// resetting.
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
    #[cfg(feature = "metrics")]
    pub fn stats(&self) -> PacerStats {
        self.metrics.snapshot()
    }
}

impl Drop for MovementSession {
    fn drop(&mut self) {
        // The pacer checks `stop` once per tick (≤ one period), so the join never hangs.
        self.shared.stop.store(true, Ordering::SeqCst);
        if let Some(h) = self.pacer.take() {
            let _ = h.join();
        }
    }
}

/// The pacer thread body: loop on the precise clock, draining the accumulator each tick and emitting
/// at most one `MOVE`. The per-tick decision lives in the pure [`Accumulator::tick_emit`].
fn pacer_loop(
    device: &Device,
    shared: &Shared,
    #[cfg(feature = "metrics")] metrics: &MetricsState,
) {
    let mut pacer = PrecisePacer::new(Duration::from_nanos(
        shared.period_ns.load(Ordering::Relaxed),
    ));

    // ~1/sec tracing aggregate (Task 5.2 hot-path safety): the pacer never traces per tick. The whole
    // mechanism is cfg-gated, so with tracing off there is not even a counter in the loop.
    #[cfg(feature = "tracing")]
    let mut agg = PacerAggregate::new();

    loop {
        pacer.wait_next_tick();

        if shared.stop.load(Ordering::SeqCst) {
            return;
        }

        // Live rate change: retune the clock if the period changed.
        let period = Duration::from_nanos(shared.period_ns.load(Ordering::Relaxed));
        if period != pacer.period() {
            pacer.set_period(period);
        }

        #[cfg(feature = "metrics")]
        metrics.record_tick(period);

        // Drain under the lock, then release before sending.
        let to_emit = shared.acc.lock().tick_emit();
        #[cfg(feature = "tracing")]
        {
            agg.ticks += 1;
        }

        if let Some((dx, dy)) = to_emit {
            #[cfg(feature = "metrics")]
            let write_start = std::time::Instant::now();

            // Fire-and-go: a send error (port gone) is ignored — the next tick retries, and reconnect
            // heals a dead port.
            let _ = device.move_rel(dx, dy);
            #[cfg(feature = "tracing")]
            {
                agg.frames += 1;
            }

            #[cfg(feature = "metrics")]
            metrics.record_write_latency(write_start.elapsed());
        }

        #[cfg(feature = "tracing")]
        agg.maybe_flush(
            #[cfg(feature = "metrics")]
            metrics,
        );
    }
}

/// Per-second tracing aggregate (Task 5.2): one DEBUG event per ~1 s window under
/// `target: "medius::pacer"`, never per tick.
#[cfg(feature = "tracing")]
struct PacerAggregate {
    window_start: std::time::Instant,
    ticks: u64,
    frames: u64,
}

#[cfg(feature = "tracing")]
impl PacerAggregate {
    const WINDOW: Duration = Duration::from_secs(1);

    fn new() -> Self {
        PacerAggregate {
            window_start: std::time::Instant::now(),
            ticks: 0,
            frames: 0,
        }
    }

    /// Emit and reset the window once [`WINDOW`](Self::WINDOW) has elapsed (with jitter p50/p99 when
    /// the `metrics` feature is on).
    fn maybe_flush(&mut self, #[cfg(feature = "metrics")] metrics: &MetricsState) {
        if self.window_start.elapsed() < Self::WINDOW {
            return;
        }
        #[cfg(feature = "metrics")]
        {
            let stats = metrics.snapshot();
            trace_event!(
                target: "medius::pacer",
                tracing::Level::DEBUG,
                frames = self.frames,
                ticks = self.ticks,
                jitter_p50_ns = stats.jitter.p50,
                jitter_p99_ns = stats.jitter.p99,
                late_ticks = stats.late_ticks,
                "pacer 1s aggregate",
            );
        }
        #[cfg(not(feature = "metrics"))]
        {
            trace_event!(
                target: "medius::pacer",
                tracing::Level::DEBUG,
                frames = self.frames,
                ticks = self.ticks,
                "pacer 1s aggregate",
            );
        }
        self.window_start = std::time::Instant::now();
        self.ticks = 0;
        self.frames = 0;
    }
}

impl Device {
    /// Open a [`MovementSession`] at the default rate ([`DEFAULT_RATE_HZ`], 1 kHz).
    ///
    /// Spawns the pacer thread; [`push`](MovementSession::push) deltas into it and they pace out one
    /// `MOVE` per tick. Drop the session to stop and join the thread.
    pub fn movement(&self) -> MovementSession {
        MovementSession::spawn(self.clone(), DEFAULT_RATE_HZ)
    }

    /// Open a [`MovementSession`] at an explicit rate in Hz (see [`movement`](Self::movement)).
    pub fn movement_at(&self, rate_hz: u32) -> MovementSession {
        MovementSession::spawn(self.clone(), rate_hz)
    }

    /// Open a [`MovementSession`] at the rate configured in `opts`
    /// ([`ConnectOptions::rate_hz`](crate::ConnectOptions::rate_hz)).
    pub fn movement_with(&self, opts: &crate::ConnectOptions) -> MovementSession {
        MovementSession::spawn(self.clone(), opts.rate_hz)
    }
}
