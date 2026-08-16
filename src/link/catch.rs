//! `CATCH` event stream: subscriber registry plus Link subscribe/unsubscribe plumbing.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use parking_lot::Mutex;

use crate::error::Result;
use crate::protocol::FrameType;
use crate::protocol::command::catch_payload;
use crate::types::{
    CatchClass, CatchEvent, CatchFilter, LockDirection, MotionEvent, TrafficEvent, UsageSnapshot,
};
use std::collections::BTreeSet;

use super::Link;

/// Host-side buffer depth per subscription (~0.25 s at 1 kHz).
pub(crate) const CATCH_CAPACITY: usize = 256;

pub(crate) struct CatchSub {
    id: u64,
    filters: BTreeSet<CatchFilter>,
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
    /// The union every subscriber together asks for. The box holds one table, so overlapping
    /// subscriptions collapse into it and each consumer still sees everything it asked for.
    fn effective(&self) -> BTreeSet<CatchFilter> {
        self.subs
            .iter()
            .flat_map(|s| s.filters.iter().copied())
            .collect()
    }
}

fn decode_event(ty: FrameType, payload: &[u8]) -> Option<CatchEvent> {
    match ty {
        FrameType::MotionEvent => MotionEvent::from_payload(payload).map(CatchEvent::Motion),
        FrameType::UsageEvent => UsageSnapshot::from_payload(payload).map(CatchEvent::Usages),
        FrameType::TrafficEvent => TrafficEvent::from_payload(payload).map(CatchEvent::Traffic),
        _ => None,
    }
}

/// The address an event carries, for matching against a subscriber's own filters.
///
/// The input frames do not carry a class byte: a `MOTION_EVENT` is always an axis, and a
/// `USAGE_EVENT` is whatever class its usages are. That is enough to route them.
fn event_address(event: &CatchEvent) -> Option<(CatchClass, u16, LockDirection)> {
    Some(match event {
        CatchEvent::Motion(_) => (CatchClass::Axis, u16::MAX, LockDirection::Both),
        CatchEvent::Usages(u) => {
            let class = match u.class()? {
                crate::types::Class::Button => CatchClass::Button,
                crate::types::Class::Key => CatchClass::Key,
                crate::types::Class::Media => CatchClass::Media,
            };
            (class, u16::MAX, LockDirection::Both)
        }
        CatchEvent::Traffic(t) => (t.class, t.id, t.direction),
    })
}

/// Deliver one decoded catch frame to the subscribers that asked for it, dropping the oldest on a
/// full buffer.
///
/// Matched against each subscriber's OWN filters, not just delivered to all of them. The box holds
/// one table -- the union of every subscription -- so without this check a caller watching one
/// endpoint would also receive everything every other caller in the process had subscribed to, and
/// its stream would change shape depending on unrelated code elsewhere.
pub(crate) fn deliver_event(reg: &Mutex<CatchReg>, ty: FrameType, payload: &[u8]) {
    let Some(event) = decode_event(ty, payload) else {
        return;
    };
    let addr = event_address(&event);
    let reg = reg.lock();
    for sub in &reg.subs {
        // An event whose address cannot be determined (an empty usage snapshot) goes to everyone
        // subscribed to anything rather than being dropped: losing it silently would be worse.
        if let Some((class, id, dir)) = addr {
            if !sub.filters.iter().any(|f| f.matches(class, id, dir)) {
                continue;
            }
        }
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
    /// Send every entry in `next` as a subscribe, and an unsubscribe for anything `prev` held that
    /// `next` does not. Re-sending an existing entry is a harmless overwrite box-side, so only the
    /// removals have to be diffed; blanket-clearing and re-adding instead would leave a gap in the
    /// stream every time any subscription changed.
    pub(crate) fn catch_sync(
        &self,
        prev: &BTreeSet<CatchFilter>,
        next: &BTreeSet<CatchFilter>,
    ) -> Result<()> {
        for f in prev.difference(next) {
            let (class, id) = f.wire();
            self.send(
                FrameType::Catch,
                &catch_payload(class, id, f.direction.as_u8(), 0, f.snaplen),
            )?;
        }
        for f in next {
            let (class, id) = f.wire();
            self.send(
                FrameType::Catch,
                &catch_payload(class, id, f.direction.as_u8(), 1, f.snaplen),
            )?;
        }
        Ok(())
    }

    /// Register a subscription, widen the box's table to the new union, and return the receiver plus
    /// drop counter.
    pub(crate) fn catch_subscribe(
        &self,
        filters: BTreeSet<CatchFilter>,
    ) -> Result<(u64, flume::Receiver<CatchEvent>, Arc<AtomicU64>)> {
        // Serialize subscribe/unsubscribe so the registry mutate, union recompute and CATCH sends
        // commit atomically; interleaving could leave the box streaming a table the registry dropped.
        let _serial = self.inner.catch_lock.lock();
        let (tx, rx) = flume::bounded::<CatchEvent>(CATCH_CAPACITY);
        let evict_rx = rx.clone();
        let dropped = Arc::new(AtomicU64::new(0));
        let id = self.inner.catch_gen.fetch_add(1, Ordering::Relaxed);
        let prev = self.inner.events.lock().effective();
        let effective = {
            let mut reg = self.inner.events.lock();
            reg.subs.push(CatchSub {
                id,
                filters,
                tx,
                evict_rx,
                dropped: Arc::clone(&dropped),
            });
            reg.effective()
        };
        self.inner.desired.lock().set_catch(effective.clone());
        if let Err(e) = self.catch_sync(&prev, &effective) {
            self.detach_sub(id);
            return Err(e);
        }
        Ok((id, rx, dropped))
    }

    /// Drop a subscription and narrow the box's table to what remains.
    pub(crate) fn catch_unsubscribe(&self, id: u64) {
        let _serial = self.inner.catch_lock.lock();
        let prev = self.inner.events.lock().effective();
        let effective = self.detach_sub(id);
        let _ = self.catch_sync(&prev, &effective);
    }

    /// Tear down every catch subscription (used by `reset()`). One blanket clear rather than a
    /// per-entry diff: nothing is left to keep streaming.
    pub(crate) fn catch_disconnect_all(&self) {
        let _serial = self.inner.catch_lock.lock();
        self.inner.events.lock().subs.clear();
        self.inner.desired.lock().set_catch(BTreeSet::new());
        let _ = self.send(
            FrameType::Catch,
            &catch_payload(
                crate::protocol::opcode::CATCH_CLS_ANY,
                crate::protocol::opcode::CATCH_ID_ANY,
                0,
                0,
                0,
            ),
        );
    }

    fn detach_sub(&self, id: u64) -> BTreeSet<CatchFilter> {
        let effective = {
            let mut reg = self.inner.events.lock();
            reg.subs.retain(|s| s.id != id);
            reg.effective()
        };
        self.inner.desired.lock().set_catch(effective.clone());
        effective
    }
}
