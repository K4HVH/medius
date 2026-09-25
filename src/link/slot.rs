use std::sync::Arc;
use std::sync::atomic::{AtomicU16, AtomicU64, Ordering};

use parking_lot::Mutex;

use crate::transport::Transport;

#[derive(Debug)]
pub(crate) struct TransportSlot {
    current: Mutex<Arc<dyn Transport>>,
    generation: AtomicU64,
    // `REFUSED | proto` once the box said hello on a protocol this build does not speak.
    refused: AtomicU16,
}

const REFUSED: u16 = 0x100;

impl TransportSlot {
    pub(crate) fn new(transport: Arc<dyn Transport>) -> Self {
        TransportSlot {
            current: Mutex::new(transport),
            generation: AtomicU64::new(0),
            refused: AtomicU16::new(0),
        }
    }

    pub(crate) fn current(&self) -> Arc<dyn Transport> {
        Arc::clone(&self.current.lock())
    }

    pub(crate) fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    pub(crate) fn swap(&self, transport: Arc<dyn Transport>) {
        *self.current.lock() = transport;
        self.generation.fetch_add(1, Ordering::Release);
        self.accept();
    }

    pub(crate) fn refuse(&self, proto: u8) {
        self.refused
            .store(REFUSED | u16::from(proto), Ordering::Release);
    }

    pub(crate) fn accept(&self) {
        self.refused.store(0, Ordering::Release);
    }

    // The protocol the box came back on, while this build refuses to speak to it.
    pub(crate) fn refused(&self) -> Option<u8> {
        let r = self.refused.load(Ordering::Acquire);
        (r & REFUSED != 0).then_some(r as u8)
    }
}
