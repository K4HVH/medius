use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use crate::error::{Error, Result};
use crate::link::Link;
use crate::link::catch::{FilterSet, collapse};
use crate::types::{CatchEvent, CatchFilter};

use super::Device;

/// Live stream of [`CatchEvent`]s from the box (`CATCH`, §3.9).
///
/// Unsubscribes when the last clone drops. [`Device::input_events`] decodes held-usage snapshots into
/// press and release edges.
#[derive(Clone, Debug)]
pub struct EventStream {
    rx: flume::Receiver<CatchEvent>,
    dropped: Arc<AtomicU64>,
    // Shared so a clone keeps the subscription.
    _guard: Arc<CatchGuard>,
}

#[derive(Debug)]
struct CatchGuard {
    link: Link,
    id: u64,
}

impl Drop for CatchGuard {
    fn drop(&mut self) {
        self.link.catch_unsubscribe(self.id);
    }
}

impl EventStream {
    pub(crate) fn new(
        rx: flume::Receiver<CatchEvent>,
        dropped: Arc<AtomicU64>,
        link: Link,
        id: u64,
    ) -> EventStream {
        EventStream {
            rx,
            dropped,
            _guard: Arc::new(CatchGuard { link, id }),
        }
    }

    /// Block until the next event.
    pub fn recv(&self) -> Result<CatchEvent> {
        self.rx.recv().map_err(|_| Error::Disconnected)
    }

    /// Next buffered event, or `None` if none is queued. Never blocks.
    pub fn try_recv(&self) -> Option<CatchEvent> {
        self.rx.try_recv().ok()
    }

    /// Block up to `timeout` for the next event; `None` on timeout or a closed channel.
    ///
    /// A closed stream returns at once, so a poll loop that ignores [`Self::is_connected`] spins once
    /// the box goes away.
    pub fn recv_timeout(&self, timeout: Duration) -> Option<CatchEvent> {
        self.rx.recv_timeout(timeout).ok()
    }

    /// Blocking iterator, ending when the box disconnects.
    pub fn iter(&self) -> impl Iterator<Item = CatchEvent> + '_ {
        self.rx.iter()
    }

    /// Drain buffered events without blocking.
    pub fn try_iter(&self) -> impl Iterator<Item = CatchEvent> + '_ {
        self.rx.try_iter()
    }

    /// Await the next event; runs under any executor.
    #[cfg(feature = "async")]
    pub async fn recv_async(&self) -> Result<CatchEvent> {
        self.rx.recv_async().await.map_err(|_| Error::Disconnected)
    }

    /// As a [`Stream`](futures_core::Stream), for `.next().await` and the combinators.
    #[cfg(feature = "async")]
    pub fn stream(&self) -> impl futures_core::Stream<Item = CatchEvent> + '_ {
        self.rx.stream()
    }

    /// Events dropped because the consumer fell behind (host-side back-pressure).
    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    /// Whether the box still delivers to this stream.
    ///
    /// Separates the two meanings of `None` from [`Self::recv_timeout`] and [`Self::try_recv`]:
    /// "nothing yet" (wait longer) and "nothing ever again" (stop).
    pub fn is_connected(&self) -> bool {
        !self.rx.is_disconnected()
    }
}

impl Iterator for EventStream {
    type Item = CatchEvent;

    fn next(&mut self) -> Option<CatchEvent> {
        self.recv().ok()
    }
}

impl<'a> IntoIterator for &'a EventStream {
    type Item = CatchEvent;
    type IntoIter = Box<dyn Iterator<Item = CatchEvent> + 'a>;

    fn into_iter(self) -> Self::IntoIter {
        Box::new(self.iter())
    }
}

// Checks a subscription and collapses it to one entry per box table slot.
pub(crate) fn prepare(filters: impl IntoIterator<Item = CatchFilter>) -> Result<FilterSet> {
    let wanted: Vec<CatchFilter> = filters.into_iter().collect();
    // An empty subscription never yields, which reads as a dead box.
    if wanted.is_empty() {
        return Err(Error::EmptySubscription);
    }
    // WITH/AGAINST resolve against the injection in flight when a report is weighed.
    if let Some(f) = wanted.iter().find(|f| f.direction().is_relative()) {
        return Err(Error::RelativeDirection {
            direction: f.direction(),
            what: "catch subscription",
        });
    }
    if let Some(f) = wanted.iter().find(|f| !f.capture_is_meaningful()) {
        return Err(Error::CaptureNotApplicable {
            class: f.class().expect("a meaningless capture names a class"),
        });
    }
    // 0xFFFF is the every-id sentinel: an exact subscription to it reaches the wire as the class
    // blanket, a much wider stream than asked for, with no error.
    if let Some((class, id)) = wanted.iter().find_map(|f| {
        let (_, id) = f.wire();
        (f.id() == Some(id)).then(|| (f.class(), id))
    }) && id == crate::protocol::opcode::CATCH_ID_ANY
    {
        return Err(Error::ReservedId {
            class: class.expect("an exact id names a class"),
            id,
        });
    }
    Ok(collapse(wanted))
}

impl Device {
    /// Subscribe to the catch stream for `filters` (`CATCH`, §3.9).
    ///
    /// Overlapping subscriptions from different callers collapse into the box's one table; each
    /// consumer still receives only what it asked for.
    ///
    /// ```no_run
    /// # use medius::{Capture, CatchFilter, Device, TrafficClass};
    /// # fn f(dev: &Device) -> medius::Result<()> {
    /// let events = dev.catch_events([
    ///     CatchFilter::everything().with_capture(Capture::First(16)),
    ///     CatchFilter::traffic(TrafficClass::VendorBulk, 3),
    /// ])?;
    /// # Ok(()) }
    /// ```
    ///
    /// # Errors
    ///
    /// [`Error::EmptySubscription`] for no filters, [`Error::CaptureNotApplicable`] for a
    /// [`Capture`](crate::Capture) on an input class, [`Error::RelativeDirection`] for a filter
    /// addressed [`With`](crate::Direction::With) or [`Against`](crate::Direction::Against), and
    /// [`Error::CatchTableFull`] when the union with every other subscription in this process
    /// exceeds the box's table.
    pub fn catch_events(
        &self,
        filters: impl IntoIterator<Item = CatchFilter>,
    ) -> Result<EventStream> {
        let (id, rx, dropped) = self.link.catch_subscribe(prepare(filters)?)?;
        Ok(EventStream::new(rx, dropped, self.link.clone(), id))
    }
}
