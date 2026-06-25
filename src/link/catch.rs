//! `CATCH` event stream — the subscriber registry the reader broadcasts decoded events to, plus the
//! Link subscribe/unsubscribe plumbing. One device-class-generic stream: the box sends mouse `EVENT`,
//! keyboard `KB_EVENT`, and media `CONS_EVENT` frames under one subscription, and each is decoded into
//! a [`CatchEvent`] variant. The box streams the UNION of every open subscription's mask, so a new
//! subscriber only ever widens the stream and each open stream receives every event in that union.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use parking_lot::Mutex;

use crate::error::Result;
use crate::protocol::FrameType;
use crate::protocol::command::catch_payload;
use crate::types::{CatchEvent, CatchMask, KeyboardEvent, MediaEvent, MouseEvent};

use super::Link;

/// Host-side buffer depth per subscription (~0.25 s at 1 kHz). A consumer that falls behind drops the
/// newest events — counted in the subscription's `dropped` gauge — rather than blocking the reader.
pub(crate) const CATCH_CAPACITY: usize = 256;

pub(crate) struct CatchSub {
    id: u64,
    mask: CatchMask,
    tx: flume::Sender<CatchEvent>,
    // A receiver clone the reader evicts from when the buffer is full (drop-oldest, like logs::push).
    // The consumer's own receiver lives in the EventStream; both share the one MPMC channel.
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

/// Decode a box→PC catch frame into a [`CatchEvent`] by its frame type.
fn decode_event(ty: FrameType, payload: &[u8]) -> Option<CatchEvent> {
    match ty {
        FrameType::MouseEvent => MouseEvent::from_payload(payload).map(CatchEvent::Mouse),
        FrameType::KbEvent => KeyboardEvent::from_payload(payload).map(CatchEvent::Keyboard),
        FrameType::ConsEvent => MediaEvent::from_payload(payload).map(CatchEvent::Media),
        _ => None,
    }
}

/// Broadcast one decoded catch frame to every subscriber. A full buffer drops the OLDEST event (evict
/// then resend, like [`logs::push`](super::logs)) so a slow consumer keeps the freshest input, not the
/// stalest; the drop is counted. A disconnected sub is skipped (its guard deregisters it).
pub(crate) fn deliver_event(reg: &Mutex<CatchReg>, ty: FrameType, payload: &[u8]) {
    let Some(event) = decode_event(ty, payload) else {
        return;
    };
    let reg = reg.lock();
    for sub in &reg.subs {
        match sub.tx.try_send(event.clone()) {
            Ok(()) => {}
            Err(flume::TrySendError::Full(e)) => {
                let _ = sub.evict_rx.try_recv(); // drop the oldest queued event
                let _ = sub.tx.try_send(e); // room freed; re-send the newest
                sub.dropped.fetch_add(1, Ordering::Relaxed);
            }
            Err(flume::TrySendError::Disconnected(_)) => {}
        }
    }
}

impl Link {
    /// Register a new subscription, push the widened union mask to the box, and hand back the
    /// receiver + a shared host-side drop counter. Rolls the subscription back if the send fails.
    pub(crate) fn catch_subscribe(
        &self,
        mask: CatchMask,
    ) -> Result<(u64, flume::Receiver<CatchEvent>, Arc<AtomicU64>)> {
        // Serialize against other subscribe/unsubscribe so the registry mutate, union recompute,
        // desired update, and CATCH send all commit in one order — never interleaved with another
        // caller's, which could leave the box streaming a mask the registry no longer matches.
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
            self.detach_sub(id); // failed subscribe leaves no trace
            return Err(e);
        }
        Ok((id, rx, dropped))
    }

    /// Drop a subscription and re-assert the (possibly narrowed) union to the box; an empty union
    /// sends `CATCH(0)` = unsubscribe. Best-effort send so a guard drop never panics on a dead link.
    pub(crate) fn catch_unsubscribe(&self, id: u64) {
        let _serial = self.inner.catch_lock.lock();
        let effective = self.detach_sub(id);
        let _ = self.send(FrameType::Catch, &catch_payload(effective.bits()));
    }

    /// Tear down EVERY catch subscription: drop all subscribers so each open
    /// [`EventStream`](crate::EventStream) sees its channel disconnect (`recv()` returns `Err`, never a
    /// silent hang), and clear the desired mask. Used by `reset()` — catch clears like injection, and
    /// the firmware drops `g_catch_mask` on the same `RESET`, so the host doesn't re-assert it.
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
