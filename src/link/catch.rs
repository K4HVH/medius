//! `CATCH` event stream: subscriber registry plus Link subscribe/unsubscribe plumbing.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use parking_lot::Mutex;

use crate::error::Result;
use crate::protocol::FrameType;
use crate::protocol::command::catch_payload;
use crate::protocol::opcode::CATCH_MAX_ENTRIES;
use crate::types::catch::FilterKey;
use crate::types::{
    CatchClass, CatchEvent, CatchFilter, Direction, MotionEvent, TrafficEvent, UsageSnapshot,
};

use super::Link;

/// Host-side buffer depth per subscription (~0.25 s at 1 kHz).
pub(crate) const CATCH_CAPACITY: usize = 256;

/// One entry per box table slot. The box holds a single entry per `(class, id, direction)`, so the
/// host has to collapse onto the same key or two subscribers silently overwrite each other.
pub(crate) type FilterSet = BTreeMap<FilterKey, CatchFilter>;

/// Collapse filters onto one entry per address, keeping the widest capture.
///
/// Widest-wins rather than last-wins: a pair naming one address at two captures has to mean the same
/// thing in either order, and only one of the two orders can be the one the caller meant.
pub(crate) fn collapse(filters: impl IntoIterator<Item = CatchFilter>) -> FilterSet {
    let mut out = FilterSet::new();
    for f in filters {
        out.entry(f.key())
            .and_modify(|e| *e = e.with_capture(e.capture().widest(f.capture())))
            .or_insert(f);
    }
    out
}

// Whether `o` is no more specific than `f` and addresses the same thing, so an event resolving to `f`
// is one `o` asked for. The box captures at the MOST SPECIFIC matching entry, so without folding
// `o`'s capture into `f`, a narrow entry from one subscriber cuts a broad subscriber's packets.
fn covers(o: CatchFilter, f: CatchFilter) -> bool {
    let class_ok = match (o.class(), f.class()) {
        (None, _) => true,
        (Some(oc), Some(fc)) => oc == fc,
        (Some(_), None) => false,
    };
    let id_ok = match (o.id(), f.id()) {
        (None, _) => true,
        (Some(oi), Some(fi)) => oi == fi,
        (Some(_), None) => false,
    };
    if !(class_ok && id_ok) {
        return false;
    }
    // Direction ranks in specificity ONLY between two entries at the same address. Once `o` is
    // broader in (class, id) its own entry always ranks below `f`, so `f` serves every direction `o`
    // admits and `f`'s capture is what the box applies. Requiring `o` to be Both there cut a broad
    // subscriber that had merely named a direction: `everything().inbound()` at whole packets got 8
    // bytes because an unrelated caller capped one endpoint.
    if o.class() == f.class() && o.id() == f.id() {
        o.direction() == Direction::Both
    } else {
        o.direction().admits(f.direction())
    }
}

// The box raises BUS events with direction BOTH, and its matcher lets a BOTH event match an entry of
// any direction -- so two siblings at one address with opposite named directions tie on rank, and the
// firmware breaks the tie by registration order. Nothing on this side models that. Collapsing the
// pair into the one BOTH entry the box can represent exactly removes the tie; it costs no extra
// traffic, because two named entries already had the box sending both directions.
fn collapse_opposite_siblings(set: &mut FilterSet) {
    let addresses: Vec<(Option<CatchClass>, Option<u16>)> = set
        .values()
        .filter(|f| f.direction() != Direction::Both)
        .map(|f| (f.class(), f.id()))
        .collect();
    for (class, id) in addresses {
        let at = |dir| {
            set.values()
                .find(|f| f.class() == class && f.id() == id && f.direction() == dir)
                .copied()
        };
        let (Some(pos), Some(neg)) = (at(Direction::Positive), at(Direction::Negative)) else {
            continue;
        };
        let mut merged = pos
            .with_direction(Direction::Both)
            .with_capture(pos.capture().widest(neg.capture()));
        if let Some(both) = at(Direction::Both) {
            merged = merged.with_capture(merged.capture().widest(both.capture()));
        }
        set.remove(&pos.key());
        set.remove(&neg.key());
        set.insert(merged.key(), merged);
    }
}

pub(crate) struct CatchSub {
    id: u64,
    filters: FilterSet,
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
    /// The union every subscriber together asks for, each entry's capture widened to satisfy every
    /// subscription that covers it.
    fn effective(&self) -> FilterSet {
        let all: Vec<CatchFilter> = self
            .subs
            .iter()
            .flat_map(|s| s.filters.values().copied())
            .collect();
        let mut out = collapse(all.iter().copied());
        for f in out.values_mut() {
            let widened = all
                .iter()
                .filter(|o| covers(**o, *f))
                .fold(f.capture(), |acc, o| acc.widest(o.capture()));
            *f = f.with_capture(widened);
        }
        collapse_opposite_siblings(&mut out);
        out
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
/// frames do not: they carry content, and the addresses they represent have to be read out of it.
/// Passing `u16::MAX` for an input event, the wildcard on the wire but not on this side, made every
/// exact-id input subscription match nothing, silently: the box accepted the entry, `RESP(CATCH)`
/// listed it, its drop count stayed zero, and the stream was empty forever.
fn wanted(sub: &CatchSub, event: &CatchEvent) -> bool {
    let any = |class, id, dir| sub.filters.values().any(|f| f.matches(class, id, dir));
    match event {
        // One report can move several axes. It is delivered if ANY axis it moved was subscribed, with
        // that axis's own sign. A report that moved nothing names no axis, so it falls back to the
        // class: an event this side cannot address is one to deliver, never one to discard.
        CatchEvent::Motion(m) => {
            let mut moved = m.axes().peekable();
            if moved.peek().is_none() {
                return sub
                    .filters
                    .values()
                    .any(|f| f.matches_class_only(CatchClass::Axis));
            }
            moved.any(|(ax, d)| any(CatchClass::Axis, ax.as_u16(), Direction::of_delta(d)))
        }
        // A snapshot is the CLASS's state, not one usage's, so it routes on class and edge. Matching
        // per-usage threw away exactly the edge a caller was waiting for -- and only when some OTHER
        // subscriber's usage happened to still be held, which is the shape that hides it from a
        // single-subscriber test.
        CatchEvent::Usages(u) => sub.filters.values().any(|f| {
            f.matches_class_only(CatchClass::from(u.class)) && f.direction().admits(u.direction)
        }),
        CatchEvent::Traffic(t) => any(t.class, t.id, t.direction),
    }
}

/// Deliver one decoded catch frame to the subscribers that asked for it, dropping the oldest on a
/// full buffer.
///
/// Matched against each subscriber's own filters, not broadcast: the box holds one table, the union
/// of every subscription, so without this check a caller watching one endpoint would also receive
/// everything every other caller in the process had subscribed to.
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
    /// Send an unsubscribe for anything `prev` holds that `next` does not, and a subscribe for every
    /// entry that is new or whose capture changed.
    ///
    /// Only what changed, not all of `next`. This runs holding `catch_lock` and every frame is a
    /// blocking serial write, so re-sending the whole table made dropping one stream cost up to a
    /// write per entry, ahead of any other subscribe and of the keepalive.
    pub(crate) fn catch_sync(&self, prev: &FilterSet, next: &FilterSet) -> Result<()> {
        // An unsubscribe of the wildcard entry is byte-for-byte the frame the box treats as "clear
        // the whole table" -- it does not look at the direction. So dropping an `everything()`
        // subscriber took every OTHER subscriber's entry with it, and the diff below then skipped
        // re-sending them because their captures had not changed: a silent hole in their streams
        // until the keepalive re-asserted, with no drop counted and no flag set.
        let wildcard_removed = prev
            .iter()
            .any(|(key, f)| f.class().is_none() && !next.contains_key(key));
        for (key, f) in prev {
            if next.contains_key(key) || (f.class().is_none() && wildcard_removed) {
                continue;
            }
            let (class, id) = f.wire();
            self.send(
                FrameType::Catch,
                &catch_payload(class, id, f.direction().as_u8(), 0, f.capture().as_u8()),
            )?;
        }
        if wildcard_removed {
            self.send(
                FrameType::Catch,
                &catch_payload(
                    crate::protocol::opcode::CATCH_CLS_ANY,
                    crate::protocol::opcode::CATCH_ID_ANY,
                    0,
                    0,
                    0,
                ),
            )?;
        }
        for (key, f) in next {
            // After a whole-table clear the box holds nothing, so every entry is new again.
            if !wildcard_removed
                && prev
                    .get(key)
                    .is_some_and(|had| had.capture() == f.capture())
            {
                continue;
            }
            let (class, id) = f.wire();
            self.send(
                FrameType::Catch,
                &catch_payload(class, id, f.direction().as_u8(), 1, f.capture().as_u8()),
            )?;
        }
        Ok(())
    }

    /// Register a subscription, widen the box's table to the new union, and return the receiver plus
    /// drop counter.
    pub(crate) fn catch_subscribe(
        &self,
        filters: FilterSet,
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
            return Err(crate::error::Error::CatchTableFull {
                needed,
                limit: CATCH_MAX_ENTRIES,
            });
        }
        self.inner.desired.lock().set_catch(effective.clone());
        if let Err(e) = self.catch_sync(&prev, &effective) {
            // The send failed PART WAY: entries before the failure are live in the box, and undoing
            // only the registry would leave the box streaming a table nothing on this side records --
            // so no later diff could narrow it, and on a vendor-bulk entry that is a quarter of a
            // megabyte a second for the life of the connection. Narrow the box back to `prev` too.
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
        self.inner.desired.lock().set_catch(FilterSet::new());
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

    fn detach_sub(&self, id: u64) -> FilterSet {
        let effective = {
            let mut reg = self.inner.events.lock();
            reg.subs.retain(|s| s.id != id);
            reg.effective()
        };
        self.inner.desired.lock().set_catch(effective.clone());
        effective
    }
}
