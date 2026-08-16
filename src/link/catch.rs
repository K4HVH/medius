//! `CATCH` event stream: subscriber registry plus Link subscribe/unsubscribe plumbing.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use parking_lot::Mutex;

use crate::error::Result;
use crate::protocol::FrameType;
use crate::protocol::command::catch_payload;
use crate::protocol::opcode::CATCH_MAX_ENTRIES;
use crate::types::{
    Axis, CatchClass, CatchEvent, CatchFilter, LockDirection, MotionEvent, TrafficEvent,
    UsageSnapshot,
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
    ///
    /// Where two subscribers name the same entry with different capture lengths, the WIDEST wins --
    /// 0 means the whole packet, so it beats every finite length. Snaplen is a property of an entry
    /// rather than part of its address, so collapsing on address alone let whichever filter the set
    /// happened to keep decide it: a caller that asked for whole packets started receiving cut ones
    /// the moment unrelated code in the same process subscribed with a shorter snaplen, with no
    /// error and nothing to say why.
    fn effective(&self) -> BTreeSet<CatchFilter> {
        let widest = CatchFilter::widest(self.subs.iter().flat_map(|s| s.filters.iter().copied()));
        // Widest-wins across ADDRESSES too, not only across identical ones. The box resolves an event
        // to its most SPECIFIC matching entry and captures at that entry's snaplen, so a narrow entry
        // from one subscriber silently cuts a broad subscriber's packets: `all()` at whole-packet plus
        // `addr(VendorBulk, 0x83).snaplen(8)` gives the blanket subscriber 8 bytes on that endpoint.
        // Every filter that would also have matched an entry therefore folds its snaplen into it.
        let all: Vec<CatchFilter> = self
            .subs
            .iter()
            .flat_map(|s| s.filters.iter().copied())
            .collect();
        widest
            .into_iter()
            .map(|mut f| {
                f.snaplen = all
                    .iter()
                    .filter(|o| {
                        o.matches(
                            f.class.unwrap_or(CatchClass::Button),
                            f.id.unwrap_or(u16::MAX),
                            f.direction,
                        )
                    })
                    .fold(f.snaplen, |acc, o| {
                        if acc == 0 || o.snaplen == 0 {
                            0
                        } else {
                            acc.max(o.snaplen)
                        }
                    });
                f
            })
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

/// Whether this subscriber asked for this event.
///
/// A traffic event carries its own `(class, id, direction)` and matches directly. The two input
/// frames do not: they carry CONTENT, and the addresses they represent have to be read out of it.
///
/// Getting that wrong is silent in the worst way. Passing `id = u16::MAX` for an input event -- which
/// is the wildcard value on the wire but not on this side -- made every exact-id input subscription
/// match nothing at all: the box accepted the entry, `RESP(CATCH)` listed it, its drop count stayed
/// zero, and the stream was simply empty forever.
fn wanted(sub: &CatchSub, event: &CatchEvent) -> bool {
    let any = |class, id, dir| sub.filters.iter().any(|f| f.matches(class, id, dir));
    match event {
        // One report can move several axes. It is delivered if ANY axis it moved was subscribed, with
        // that axis's own direction -- the sign of its delta, which is what an axis direction means
        // here and what the box resolves on. A report that moved nothing names no axis, so it falls
        // back to the class, exactly as the empty usage snapshot does: the box does not emit one, and
        // an event this side cannot address is one to deliver, never one to discard.
        CatchEvent::Motion(m) => {
            let moved = [(Axis::X, m.dx), (Axis::Y, m.dy), (Axis::Wheel, m.dz)];
            if moved.iter().all(|(_, d)| *d == 0) {
                return sub
                    .filters
                    .iter()
                    .any(|f| f.matches_class_only(CatchClass::Axis));
            }
            moved.into_iter().any(|(ax, d)| {
                d != 0
                    && any(
                        CatchClass::Axis,
                        ax.as_u16(),
                        if d > 0 {
                            LockDirection::Positive
                        } else {
                            LockDirection::Negative
                        },
                    )
            })
        }
        // A snapshot is the CLASS's state, not one usage's: it lists what is HELD, so the release of
        // usage U is the snapshot that does NOT contain U. Matching per-usage therefore threw away
        // exactly the edge a caller was waiting for -- and only when some OTHER subscriber's usage
        // happened to still be held, which is the shape that hides it from a single-subscriber test.
        // Routed on class and edge instead; the subscriber diffs successive snapshots for its own
        // usages, which is the only thing a snapshot can support.
        CatchEvent::Usages(u) => sub.filters.iter().any(|f| {
            f.matches_class_only(CatchClass::from_usage_class(u.class))
                && f.admits_direction(u.direction)
        }),
        CatchEvent::Traffic(t) => any(t.class, t.id, t.direction),
    }
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
    let reg = reg.lock();
    for sub in &reg.subs {
        if !wanted(sub, &event) {
            continue;
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
    /// Send an unsubscribe for anything `prev` held that `next` does not, and a subscribe for every
    /// entry that is new or whose capture length changed. Blanket-clearing and re-adding instead
    /// would leave a gap in the stream every time any subscription changed.
    ///
    /// Only what CHANGED, not all of `next`. This runs holding `catch_lock`, and every frame is a
    /// blocking write on a serial port — so re-sending the whole table made dropping one stream cost
    /// up to a write per entry, all of it in front of any other subscribe and of the keepalive. The
    /// ordinary case is now zero or one frame. Snaplen has to be compared explicitly because it is
    /// deliberately not part of a filter's identity, so the set difference alone cannot see it move.
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
            if prev.get(f).is_some_and(|had| had.snaplen == f.snaplen) {
                continue; // the box already holds this entry, at this capture length
            }
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
        // Refused BEFORE anything is sent, and before the registry keeps the subscription: the box
        // silently drops entries past its table and reports it only in a flag nothing was obliged to
        // read, so the caller's stream would just be missing the addresses that did not fit.
        if effective.len() > CATCH_MAX_ENTRIES {
            let needed = effective.len();
            self.inner.events.lock().subs.retain(|s| s.id != id);
            return Err(crate::error::Error::CatchTableFull { needed });
        }
        self.inner.desired.lock().set_catch(effective.clone());
        if let Err(e) = self.catch_sync(&prev, &effective) {
            // The send failed PART WAY: entries before the failure are live in the box, and undoing
            // only the registry would leave the box streaming a table nothing on this side records --
            // so no later diff could ever narrow it, and on a vendor-bulk entry that is a quarter of a
            // megabyte a second for the life of the connection. Narrow the box back to `prev` too.
            // Best-effort: if the link is down that write fails as well, and a reconnect re-syncs
            // from `desired`, which detach_sub has already corrected.
            let restored = self.detach_sub(id);
            let _ = self.catch_sync(&effective, &restored);
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
