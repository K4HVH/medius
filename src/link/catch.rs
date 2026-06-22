//! `CATCH` event stream — the subscriber registry the reader broadcasts decoded `EVENT` frames to,
//! plus the Link subscribe/unsubscribe plumbing. The box streams the UNION of every open
//! subscription's mask, so a new subscriber only ever widens the stream and each open stream receives
//! every event in that union.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use parking_lot::Mutex;

use crate::error::Result;
use crate::protocol::FrameType;
use crate::protocol::command::catch_payload;
use crate::types::{CatchMask, InputReport};

use super::Link;

/// Host-side buffer depth per subscription (~0.25 s at 1 kHz). A consumer that falls behind drops the
/// newest events — counted in the subscription's `dropped` gauge — rather than blocking the reader.
pub(crate) const CATCH_CAPACITY: usize = 256;

pub(crate) struct CatchSub {
    id: u64,
    mask: CatchMask,
    tx: flume::Sender<InputReport>,
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

/// Broadcast one decoded `EVENT` to every subscriber. A full channel drops the newest event (counted);
/// a disconnected one is skipped — its [`EventStream`](crate::EventStream) guard deregisters it.
pub(crate) fn deliver_event(reg: &Mutex<CatchReg>, payload: &[u8]) {
    let Some(report) = InputReport::from_payload(payload) else {
        return;
    };
    let reg = reg.lock();
    for sub in &reg.subs {
        match sub.tx.try_send(report) {
            Ok(()) => {}
            Err(flume::TrySendError::Full(_)) => {
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
    ) -> Result<(u64, flume::Receiver<InputReport>, Arc<AtomicU64>)> {
        // Serialize against other subscribe/unsubscribe so the registry mutate, union recompute,
        // desired update, and CATCH send all commit in one order — never interleaved with another
        // caller's, which could leave the box streaming a mask the registry no longer matches.
        let _serial = self.inner.catch_lock.lock();
        let (tx, rx) = flume::bounded::<InputReport>(CATCH_CAPACITY);
        let dropped = Arc::new(AtomicU64::new(0));
        let id = self.inner.catch_gen.fetch_add(1, Ordering::Relaxed);
        let effective = {
            let mut reg = self.inner.events.lock();
            reg.subs.push(CatchSub {
                id,
                mask,
                tx,
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
