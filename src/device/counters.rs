//! Always-on atomic counters — cheap, lock-free diagnostics.
//!
//! [`Counters`] tracks four lifetime totals as relaxed [`AtomicU64`]s: frames sent, frames received,
//! CRC-dropped frames, and reconnects. They are **core** (always compiled, never feature-gated) and
//! effectively free on the hot path — the design spec calls them out as always-on telemetry distinct
//! from the optional `metrics` histograms.
//!
//! [`Counters::snapshot`] takes a consistent-enough point-in-time read into a plain
//! [`CountersSnapshot`] (serde-gated) for display / JSON. The reads are independent relaxed loads, so
//! a snapshot is not a transactional barrier across all four fields — that is intentional and
//! sufficient for diagnostics.

use core::sync::atomic::{AtomicU64, Ordering};

/// Lifetime atomic counters for one [`Device`](crate::Device).
///
/// All fields use [`Ordering::Relaxed`] — these are statistics, not synchronization, so no ordering
/// guarantee against other memory is needed or paid for.
#[derive(Debug, Default)]
pub(crate) struct Counters {
    /// Total frames written to the transport.
    pub(crate) frames_tx: AtomicU64,
    /// Total frames decoded from the transport (RESP + LOG + any other known type).
    pub(crate) frames_rx: AtomicU64,
    /// Total frames dropped by the decoder because their CRC failed.
    pub(crate) crc_drops: AtomicU64,
    /// Total successful reconnects.
    pub(crate) reconnects: AtomicU64,
}

impl Counters {
    /// Add one to `frames_tx`.
    pub(crate) fn inc_tx(&self) {
        self.frames_tx.fetch_add(1, Ordering::Relaxed);
    }

    /// Add one to `frames_rx`.
    pub(crate) fn inc_rx(&self) {
        self.frames_rx.fetch_add(1, Ordering::Relaxed);
    }

    /// Add one to `reconnects`.
    #[cfg_attr(not(test), allow(dead_code))] // driven by reconnect (Task 3.6)
    pub(crate) fn inc_reconnects(&self) {
        self.reconnects.fetch_add(1, Ordering::Relaxed);
    }

    /// Set `crc_drops` to `n` (the decoder owns the running total, so we mirror it rather than
    /// increment). Monotonic in practice.
    pub(crate) fn set_crc_drops(&self, n: u64) {
        self.crc_drops.store(n, Ordering::Relaxed);
    }

    /// Take a point-in-time snapshot of all four counters.
    pub(crate) fn snapshot(&self) -> CountersSnapshot {
        CountersSnapshot {
            frames_tx: self.frames_tx.load(Ordering::Relaxed),
            frames_rx: self.frames_rx.load(Ordering::Relaxed),
            crc_drops: self.crc_drops.load(Ordering::Relaxed),
            reconnects: self.reconnects.load(Ordering::Relaxed),
        }
    }
}

/// A plain, copyable snapshot of [`Counters`] for display / JSON (serde-gated).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct CountersSnapshot {
    /// Total frames written to the transport.
    pub frames_tx: u64,
    /// Total frames decoded from the transport.
    pub frames_rx: u64,
    /// Total frames dropped for a failed CRC.
    pub crc_drops: u64,
    /// Total successful reconnects.
    pub reconnects: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_at_zero() {
        let c = Counters::default();
        assert_eq!(c.snapshot(), CountersSnapshot::default());
    }

    #[test]
    fn increments_each_field() {
        let c = Counters::default();
        c.inc_tx();
        c.inc_tx();
        c.inc_rx();
        c.inc_reconnects();
        c.set_crc_drops(5);

        let s = c.snapshot();
        assert_eq!(s.frames_tx, 2);
        assert_eq!(s.frames_rx, 1);
        assert_eq!(s.crc_drops, 5);
        assert_eq!(s.reconnects, 1);
    }

    #[test]
    fn snapshot_is_independent_copy() {
        let c = Counters::default();
        c.inc_tx();
        let s1 = c.snapshot();
        c.inc_tx();
        let s2 = c.snapshot();
        // The earlier snapshot is unaffected by the later increment.
        assert_eq!(s1.frames_tx, 1);
        assert_eq!(s2.frames_tx, 2);
    }

    #[test]
    fn counters_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Counters>();
    }

    #[cfg(feature = "serde")]
    #[test]
    fn snapshot_serde_round_trip() {
        let s = CountersSnapshot {
            frames_tx: 10,
            frames_rx: 7,
            crc_drops: 1,
            reconnects: 2,
        };
        let j = serde_json::to_string(&s).unwrap();
        assert_eq!(serde_json::from_str::<CountersSnapshot>(&j).unwrap(), s);
    }
}
