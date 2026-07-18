//! `CATCH` event stream: subscriber registry plus Link subscribe/unsubscribe plumbing.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use parking_lot::Mutex;

use crate::error::Result;
use crate::protocol::FrameType;
use crate::protocol::command::catch_payload;
use crate::types::{CatchEvent, CatchMask, MotionEvent, UsageSnapshot};

use super::Link;

/// Host-side buffer depth per subscription (~0.25 s at 1 kHz).
pub(crate) const CATCH_CAPACITY: usize = 256;

pub(crate) struct CatchSub {
    id: u64,
    mask: CatchMask,
    tx: flume::Sender<CatchEvent>,
    // Reader-side clone for drop-oldest eviction; the consumer's own receiver lives in the
    // EventStream, both sharing the one MPMC channel.
    evict_rx: flume::Receiver<CatchEvent>,
    dropped: Arc<AtomicU64>,
}

#[derive(Default)]
pub(crate) struct CatchReg {
    subs: Vec<CatchSub>,
}

impl CatchReg {
    fn effective(&self) -> CatchMask {
        self.subs.iter().fold(CatchMask::empty(), |m, s| m | s.mask)
    }
}

fn decode_event(ty: FrameType, payload: &[u8]) -> Option<CatchEvent> {
    match ty {
        FrameType::MotionEvent => MotionEvent::from_payload(payload).map(CatchEvent::Motion),
        FrameType::UsageEvent => UsageSnapshot::from_payload(payload).map(CatchEvent::Usages),
        _ => None,
    }
}

/// Broadcast one decoded catch frame to every subscriber, dropping the oldest on a full buffer.
pub(crate) fn deliver_event(reg: &Mutex<CatchReg>, ty: FrameType, payload: &[u8]) {
    let Some(event) = decode_event(ty, payload) else {
        return;
    };
    let reg = reg.lock();
    for sub in &reg.subs {
        match sub.tx.try_send(event.clone()) {
            Ok(()) => {}
            Err(flume::TrySendError::Full(e)) => {
                let _ = sub.evict_rx.try_recv();
                let _ = sub.tx.try_send(e);
                sub.dropped.fetch_add(1, Ordering::Relaxed);
            }
            Err(flume::TrySendError::Disconnected(_)) => {}
        }
    }
}

impl Link {
    /// Register a subscription, push the widened union mask, and return the receiver plus drop counter.
    pub(crate) fn catch_subscribe(
        &self,
        mask: CatchMask,
    ) -> Result<(u64, flume::Receiver<CatchEvent>, Arc<AtomicU64>)> {
        // Serialize subscribe/unsubscribe so the registry mutate, union recompute, and CATCH send
        // commit atomically; interleaving could leave the box streaming a mask the registry dropped.
        let _serial = self.inner.catch_lock.lock();
        let (tx, rx) = flume::bounded::<CatchEvent>(CATCH_CAPACITY);
        let evict_rx = rx.clone();
        let dropped = Arc::new(AtomicU64::new(0));
        let id = self.inner.catch_gen.fetch_add(1, Ordering::Relaxed);
        let effective = {
            let mut reg = self.inner.events.lock();
            reg.subs.push(CatchSub {
                id,
                mask,
                tx,
                evict_rx,
                dropped: Arc::clone(&dropped),
            });
            reg.effective()
        };
        self.inner.desired.lock().set_catch(effective);
        if let Err(e) = self.send(FrameType::Catch, &catch_payload(effective.bits())) {
            self.detach_sub(id);
            return Err(e);
        }
        Ok((id, rx, dropped))
    }

    /// Drop a subscription and re-assert the narrowed union; an empty union sends `CATCH(0)` to unsubscribe.
    pub(crate) fn catch_unsubscribe(&self, id: u64) {
        let _serial = self.inner.catch_lock.lock();
        let effective = self.detach_sub(id);
        let _ = self.send(FrameType::Catch, &catch_payload(effective.bits()));
    }

    /// Tear down every catch subscription and clear the desired mask (used by `reset()`).
    pub(crate) fn catch_disconnect_all(&self) {
        let _serial = self.inner.catch_lock.lock();
        self.inner.events.lock().subs.clear();
        self.inner.desired.lock().set_catch(CatchMask::empty());
    }

    fn detach_sub(&self, id: u64) -> CatchMask {
        let effective = {
            let mut reg = self.inner.events.lock();
            reg.subs.retain(|s| s.id != id);
            reg.effective()
        };
        self.inner.desired.lock().set_catch(effective);
        effective
    }
}
