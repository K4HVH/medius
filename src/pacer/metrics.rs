//! Pacer timing metrics (`pacer/metrics.rs`, feature = `metrics`).
//!
//! When the `metrics` feature is on, the pacer thread records, per tick, the **jitter** (realized
//! inter-tick interval minus the ideal period) and the **write latency** of the one `MOVE` it emits,
//! and counts **late ticks** (a tick whose interval overran the period). [`MovementSession::stats`]
//! snapshots them into a [`PacerStats`] — the detailed timing layer that self-validates the 1 kHz /
//! no-jitter claim (§10 of the design spec).
//!
//! **Zero-cost when off.** This whole module is `#[cfg(feature = "metrics")]`, and every call site in
//! the pacer hot loop is cfg-gated too, so with the feature off there is *no* recording — no atomics,
//! no branches, nothing compiled into the tick loop.
//!
//! Histograms use [`hdrhistogram`]: high dynamic range, fixed memory, lock-free reads via a snapshot
//! clone. Values are recorded in **nanoseconds**.
//!
//! [`MovementSession::stats`]: super::MovementSession::stats

use std::time::Duration;

use hdrhistogram::Histogram;
use parking_lot::Mutex;

/// The maximum value (ns) the histograms track before saturating: 1 second. A tick or write that
/// takes longer than this is an extreme outlier; it is recorded at the ceiling rather than lost.
const MAX_TRACKED_NS: u64 = 1_000_000_000;

/// Live, mutable metrics the pacer thread writes each tick. Held behind an [`Arc`](std::sync::Arc) so
/// the session handle can [`snapshot`](Self::snapshot) it concurrently.
#[derive(Debug)]
pub(crate) struct MetricsState {
    inner: Mutex<Inner>,
}

#[derive(Debug)]
struct Inner {
    /// Total ticks the pacer has run (whether or not a `MOVE` was emitted).
    ticks: u64,
    /// Ticks whose realized interval overran the ideal period (`interval > period`).
    late_ticks: u64,
    /// `|realized interval − ideal period|` per tick, in nanoseconds.
    jitter: Histogram<u64>,
    /// `Device::move_rel` write latency per emitting tick, in nanoseconds.
    write_latency: Histogram<u64>,
    /// The realized instant of the previous tick, to compute the next interval. `None` until the
    /// first recorded tick (which establishes the baseline and counts but contributes no jitter).
    last_tick: Option<std::time::Instant>,
}

impl MetricsState {
    /// Create an empty metrics state with auto-resizing histograms (1 ns–1 s range, 3 sig figs).
    pub(crate) fn new() -> Self {
        // new_with_bounds(low, high, sigfig): low=1ns avoids a zero-low panic; high=1s ceiling. These
        // are non-auto so a pathological value saturates at the ceiling instead of growing unbounded.
        let jitter =
            Histogram::new_with_bounds(1, MAX_TRACKED_NS, 3).expect("valid histogram bounds");
        let write_latency =
            Histogram::new_with_bounds(1, MAX_TRACKED_NS, 3).expect("valid histogram bounds");
        MetricsState {
            inner: Mutex::new(Inner {
                ticks: 0,
                late_ticks: 0,
                jitter,
                write_latency,
                last_tick: None,
            }),
        }
    }

    /// Record one tick: bump the tick count, and (from the second tick on) the jitter vs `period`
    /// and the late-tick count. Called once per tick by the pacer loop.
    pub(crate) fn record_tick(&self, period: Duration) {
        let now = std::time::Instant::now();
        let mut inner = self.inner.lock();
        inner.ticks += 1;
        if let Some(prev) = inner.last_tick {
            let interval = now.duration_since(prev);
            // Jitter = |interval − period|.
            let jitter_ns = abs_diff_nanos(interval, period);
            let _ = inner.jitter.record(jitter_ns.max(1));
            if interval > period {
                inner.late_ticks += 1;
            }
        }
        inner.last_tick = Some(now);
    }

    /// Record the write latency of one emitted `MOVE`.
    pub(crate) fn record_write_latency(&self, latency: Duration) {
        let ns = duration_nanos_clamped(latency);
        self.inner.lock().write_latency.record(ns.max(1)).ok();
    }

    /// Take a point-in-time snapshot for [`MovementSession::stats`](super::MovementSession::stats).
    pub(crate) fn snapshot(&self) -> PacerStats {
        let inner = self.inner.lock();
        PacerStats {
            ticks: inner.ticks,
            late_ticks: inner.late_ticks,
            jitter: HistogramSnapshot::from(&inner.jitter),
            write_latency: HistogramSnapshot::from(&inner.write_latency),
        }
    }
}

/// A snapshot of the pacer's timing metrics (returned by
/// [`MovementSession::stats`](super::MovementSession::stats)).
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PacerStats {
    /// Total ticks the pacer has run.
    pub ticks: u64,
    /// Ticks whose realized interval overran the ideal period.
    pub late_ticks: u64,
    /// Per-tick jitter (`|interval − period|`) distribution, nanoseconds.
    pub jitter: HistogramSnapshot,
    /// Per-emitting-tick `MOVE` write-latency distribution, nanoseconds.
    pub write_latency: HistogramSnapshot,
}

/// A serializable summary of an [`hdrhistogram::Histogram`] — count, min/max/mean, and key
/// percentiles in nanoseconds. (The live histogram is not itself serde-able, and a full snapshot is
/// rarely what a consumer wants; these summary stats are.)
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct HistogramSnapshot {
    /// Number of samples recorded.
    pub count: u64,
    /// Minimum recorded value (ns); 0 if empty.
    pub min: u64,
    /// Maximum recorded value (ns); 0 if empty.
    pub max: u64,
    /// Arithmetic mean (ns); 0.0 if empty.
    pub mean: f64,
    /// 50th percentile (median, ns).
    pub p50: u64,
    /// 90th percentile (ns).
    pub p90: u64,
    /// 99th percentile (ns).
    pub p99: u64,
}

impl HistogramSnapshot {
    /// Whether any samples were recorded.
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }
}

impl From<&Histogram<u64>> for HistogramSnapshot {
    fn from(h: &Histogram<u64>) -> Self {
        HistogramSnapshot {
            count: h.len(),
            min: if h.is_empty() { 0 } else { h.min() },
            max: if h.is_empty() { 0 } else { h.max() },
            mean: h.mean(),
            p50: h.value_at_quantile(0.50),
            p90: h.value_at_quantile(0.90),
            p99: h.value_at_quantile(0.99),
        }
    }
}

/// `|a − b|` in nanoseconds, saturating into `u64`.
fn abs_diff_nanos(a: Duration, b: Duration) -> u64 {
    duration_nanos_clamped(a.abs_diff(b))
}

/// A `Duration` as `u64` nanoseconds, clamped to [`MAX_TRACKED_NS`] so a huge stall saturates the
/// histogram ceiling instead of erroring on `record`.
fn duration_nanos_clamped(d: Duration) -> u64 {
    d.as_nanos().min(MAX_TRACKED_NS as u128) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_ticks_and_jitter() {
        let m = MetricsState::new();
        let period = Duration::from_millis(1);
        // First tick: baseline (no jitter sample yet).
        m.record_tick(period);
        // A few more ticks with a tiny real gap so an interval exists.
        for _ in 0..5 {
            std::thread::sleep(Duration::from_micros(200));
            m.record_tick(period);
        }
        m.record_write_latency(Duration::from_micros(30));

        let s = m.snapshot();
        assert_eq!(s.ticks, 6);
        // 5 jitter samples (ticks 2..=6), at least one write-latency sample.
        assert_eq!(s.jitter.count, 5);
        assert!(!s.jitter.is_empty());
        assert_eq!(s.write_latency.count, 1);
        assert!(!s.write_latency.is_empty());
        assert!(s.write_latency.max >= 1);
    }

    #[test]
    fn counts_late_ticks() {
        let m = MetricsState::new();
        // Tiny ideal period → real sleeps overrun it → every interval counts as late.
        let period = Duration::from_nanos(1);
        m.record_tick(period); // baseline
        for _ in 0..3 {
            std::thread::sleep(Duration::from_micros(100));
            m.record_tick(period);
        }
        let s = m.snapshot();
        assert_eq!(s.ticks, 4);
        assert_eq!(s.late_ticks, 3, "every real interval overruns a 1ns period");
    }

    #[test]
    fn empty_snapshot_is_clean() {
        let m = MetricsState::new();
        let s = m.snapshot();
        assert_eq!(s.ticks, 0);
        assert_eq!(s.late_ticks, 0);
        assert!(s.jitter.is_empty());
        assert!(s.write_latency.is_empty());
        assert_eq!(s.jitter.min, 0);
        assert_eq!(s.jitter.max, 0);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn stats_serialize_round_trips() {
        let m = MetricsState::new();
        m.record_tick(Duration::from_millis(1));
        std::thread::sleep(Duration::from_micros(200));
        m.record_tick(Duration::from_millis(1));
        let s = m.snapshot();
        let j = serde_json::to_string(&s).unwrap();
        let back: PacerStats = serde_json::from_str(&j).unwrap();
        assert_eq!(back.ticks, s.ticks);
        assert_eq!(back.jitter.count, s.jitter.count);
    }
}
