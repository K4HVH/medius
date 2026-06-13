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
