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

// The box holds one entry per `(class, id, direction)`, so the host collapses onto that key or two
// subscribers overwrite each other with no error.
pub(crate) type FilterSet = BTreeMap<FilterKey, CatchFilter>;

// Collapse filters onto one entry per address, keeping the widest capture.
pub(crate) fn collapse(filters: impl IntoIterator<Item = CatchFilter>) -> FilterSet {
    let mut out = FilterSet::new();
    for f in filters {
        out.entry(f.key())
            .and_modify(|e| *e = e.with_capture(e.capture().widest(f.capture())))
            .or_insert(f);
    }
    out
}

// Whether `o` addresses what `f` does and is no more specific, so an event resolving to `f` is one
// `o` asked for.
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
    // Direction ranks in specificity only between entries at the same address.
    if o.class() == f.class() && o.id() == f.id() {
        o.direction() == Direction::Both
    } else {
        o.direction().admits(f.direction())
    }
}

// The box raises BUS events with direction BOTH, which match an entry of any direction, so opposite
// named siblings at one address tie on rank and the firmware breaks the tie by registration order,
// which this side does not model. Collapsing the pair to one BOTH entry removes the tie at no extra
// traffic: the named pair already had the box sending both directions.
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
    // Reader-side clone for drop-oldest eviction; the consumer's receiver is in the EventStream, on
    // the same MPMC channel.
    evict_rx: flume::Receiver<CatchEvent>,
    dropped: Arc<AtomicU64>,
}

#[derive(Default)]
pub(crate) struct CatchReg {
    subs: Vec<CatchSub>,
}

impl CatchReg {
    // Union of all subscriptions, each entry's capture widened to satisfy every one covering it.
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

// A traffic event carries its `(class, id, direction)` and matches directly.
fn wanted(sub: &CatchSub, event: &CatchEvent) -> bool {
    let any = |class, id, dir| sub.filters.values().any(|f| f.matches(class, id, dir));
    match event {
        // Delivered if any axis the report moved was subscribed, with that axis's sign.
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
        // A snapshot is the class's state, so it routes on class and edge.
        CatchEvent::Usages(u) => sub.filters.values().any(|f| {
            f.matches_class_only(CatchClass::from(u.class)) && f.direction().admits(u.direction)
        }),
        CatchEvent::Traffic(t) => any(t.class, t.id, t.direction),
    }
}

// Drops the oldest on a full buffer.
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
    // Sends only the changes: this holds `catch_lock` and each frame is a blocking serial write, so
    // re-sending the whole table cost a write per entry ahead of other subscribes and the keepalive.
    pub(crate) fn catch_sync(&self, prev: &FilterSet, next: &FilterSet) -> Result<()> {
        // Unsubscribing the wildcard entry is byte for byte the box's "clear the whole table"
        // frame, whatever the direction.
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

    // Registers, widens the box's table to the new union, returns the receiver and drop counter.
    pub(crate) fn catch_subscribe(
        &self,
        filters: FilterSet,
    ) -> Result<(u64, flume::Receiver<CatchEvent>, Arc<AtomicU64>)> {
        // Registry mutate, union recompute and CATCH sends commit atomically, or the box can stream
        // a table the registry dropped.
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
        // Refused before any send or registration: the box drops entries past its table, flagged
        // only in `table_full`, so the stream would miss the addresses that did not fit.
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
            // A partial send leaves earlier entries live on the box; undoing only the registry
            // leaves a table no later diff narrows (a vendor-bulk entry streams ~250 KB/s for the
            // connection's life). Narrow the box back to `prev` too.
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

    // For `reset()`, which holds the subscribe/unsubscribe lock. One blanket clear, since nothing
    // is left.
    pub(crate) fn catch_disconnect_all_locked(&self) {
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
