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
    last_raw: Option<u32>,
    epoch: u64,
}

impl CatchReg {
    fn effective(&self) -> CatchMask {
        self.subs.iter().fold(CatchMask::empty(), |m, s| m | s.mask)
    }

    /// Widen the wire's `u32` microseconds into the `u64` consumers see.
    ///
    /// A stamp lower than the last one is a rollover only if it looks like one, meaning the previous
    /// value sat in the top quarter of the range and the new one in the bottom quarter. Any other
    /// decrease is the box's clock restarting rather than wrapping, which a reconnect hook would miss:
    /// the chip that stamps these is the mouse-facing one, and it can reboot on its own without the
    /// control link ever dropping. There the epoch restarts too, so the value visibly steps backwards
    /// instead of jumping 71.6 minutes into the future.
    fn widen(&mut self, raw: u32) -> u64 {
        const QUARTER: u32 = u32::MAX / 4;
        if let Some(prev) = self.last_raw
            && raw < prev
        {
            if prev > QUARTER * 3 && raw < QUARTER {
                self.epoch += 1 << 32;
            } else {
                self.epoch = 0;
            }
        }
        self.last_raw = Some(raw);
        self.epoch + raw as u64
    }

    fn reset_clock(&mut self) {
        self.last_raw = None;
        self.epoch = 0;
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
    let Some(mut event) = decode_event(ty, payload) else {
        return;
    };
    let mut reg = reg.lock();
    // from_payload leaves the raw wire u32 in ts_us; widen it here, where the epoch state lives.
    let raw = match &event {
        CatchEvent::Motion(m) => m.ts_us as u32,
        CatchEvent::Usages(u) => u.ts_us as u32,
    };
    let wide = reg.widen(raw);
    match &mut event {
        CatchEvent::Motion(m) => m.ts_us = wide,
        CatchEvent::Usages(u) => u.ts_us = wide,
    }
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
            if reg.subs.is_empty() {
                reg.reset_clock(); // first subscriber: no stale epoch from an earlier stream
            }
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
        let mut reg = self.inner.events.lock();
        reg.subs.clear();
        reg.reset_clock();
        drop(reg);
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
