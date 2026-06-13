//! Always-on atomic counters — cheap, lock-free diagnostics.
//!
//! [`Counters`] tracks four lifetime totals (frames sent/received, CRC-dropped frames, reconnects) as
//! relaxed [`AtomicU64`]s. They are core (always compiled, effectively free on the hot path).
//! [`Counters::snapshot`] reads them into a plain [`CountersSnapshot`] (the public value type, in
//! [`crate::types`]); the four reads are independent, so a snapshot is not transactional across fields
//! — intentional and sufficient for diagnostics.

use core::sync::atomic::{AtomicU64, Ordering};

use crate::types::CountersSnapshot;

/// Lifetime atomic counters for one [`Device`](crate::Device). All [`Ordering::Relaxed`] — statistics,
/// not synchronization.
#[derive(Debug, Default)]
pub(crate) struct Counters {
    pub(crate) frames_tx: AtomicU64,
    pub(crate) frames_rx: AtomicU64,
    pub(crate) crc_drops: AtomicU64,
    pub(crate) reconnects: AtomicU64,
}

impl Counters {
    pub(crate) fn inc_tx(&self) {
        self.frames_tx.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn inc_rx(&self) {
        self.frames_rx.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn inc_reconnects(&self) {
        self.reconnects.fetch_add(1, Ordering::Relaxed);
    }

    /// Mirror the decoder's running CRC-drop total (it owns the count; we store, not increment).
    pub(crate) fn set_crc_drops(&self, n: u64) {
        self.crc_drops.store(n, Ordering::Relaxed);
    }

    pub(crate) fn snapshot(&self) -> CountersSnapshot {
        CountersSnapshot {
            frames_tx: self.frames_tx.load(Ordering::Relaxed),
            frames_rx: self.frames_rx.load(Ordering::Relaxed),
            crc_drops: self.crc_drops.load(Ordering::Relaxed),
            reconnects: self.reconnects.load(Ordering::Relaxed),
        }
    }
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
        assert_eq!(s1.frames_tx, 1);
        assert_eq!(s2.frames_tx, 2);
    }

    #[test]
    fn counters_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Counters>();
    }
}
