use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use parking_lot::Mutex;

use crate::transport::Transport;

#[derive(Debug)]
pub(crate) struct TransportSlot {
    current: Mutex<Arc<dyn Transport>>,
    generation: AtomicU64,
}

impl TransportSlot {
    pub(crate) fn new(transport: Arc<dyn Transport>) -> Self {
        TransportSlot {
            current: Mutex::new(transport),
            generation: AtomicU64::new(0),
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
    }
}
