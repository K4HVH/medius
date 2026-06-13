use core::sync::atomic::{AtomicU64, Ordering};

use crate::types::CountersSnapshot;

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
