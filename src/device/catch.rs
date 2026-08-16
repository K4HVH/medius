use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use crate::error::{Error, Result};
use crate::link::Link;
use crate::types::{CatchEvent, CatchFilter};

use super::Device;

/// A live stream of physical-input [`CatchEvent`]s from the box (the `CATCH` feature, §3.9).
#[derive(Clone, Debug)]
pub struct EventStream {
    rx: flume::Receiver<CatchEvent>,
    dropped: Arc<AtomicU64>,
    // Unsubscribes when the last clone drops; the Arc keeps it alive across clones.
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

    /// Block until the next physical-input event arrives.
    pub fn recv(&self) -> Result<CatchEvent> {
        self.rx.recv().map_err(|_| Error::Disconnected)
    }

    /// The next buffered event, or `None` if none is queued (never blocks).
    pub fn try_recv(&self) -> Option<CatchEvent> {
        self.rx.try_recv().ok()
    }

    /// Block up to `timeout` for the next event; `None` on timeout (or a closed channel).
    pub fn recv_timeout(&self, timeout: Duration) -> Option<CatchEvent> {
        self.rx.recv_timeout(timeout).ok()
    }

    /// Drain every currently-buffered event without blocking.
    pub fn try_iter(&self) -> impl Iterator<Item = CatchEvent> + '_ {
        self.rx.try_iter()
    }

    /// Await the next event; runtime-agnostic, runs under any executor.
    #[cfg(feature = "async")]
    pub async fn recv_async(&self) -> Result<CatchEvent> {
        self.rx.recv_async().await.map_err(|_| Error::Disconnected)
    }

    /// Events this stream dropped because the consumer fell behind (host-side back-pressure).
    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }
}

impl Device {
    /// Subscribe to the catch stream for the given filters (the `CATCH` feature, §3.9).
    ///
    /// Each [`CatchFilter`] addresses a class, an id within it, or everything. Overlapping
    /// subscriptions from different callers collapse into the one table the box holds, and each
    /// consumer still receives everything it asked for.
    ///
    /// ```no_run
    /// # use medius::{Device, CatchClass, CatchFilter};
    /// # fn f(dev: &Device) -> medius::Result<()> {
    /// // Everything, cut to 16 bytes per event, except one endpoint kept whole.
    /// let events = dev.catch_events([
    ///     CatchFilter::all().snaplen(16),
    ///     CatchFilter::addr(CatchClass::VendorBulk, 0x83),
    /// ])?;
    /// # Ok(()) }
    /// ```
    pub fn catch_events(
        &self,
        filters: impl IntoIterator<Item = CatchFilter>,
    ) -> Result<EventStream> {
        // Widest-wins within this one call too. Collecting straight into the set silently kept
        // whichever duplicate address came last, so `[addr(x).snaplen(0), addr(x).snaplen(16)]` cut
        // the caller's own captures to 16 while the reverse order gave whole packets -- a difference
        // in the order two filters were listed, with nothing to say so.
        let set = CatchFilter::widest(filters);
        let (id, rx, dropped) = self.link.catch_subscribe(set)?;
        Ok(EventStream::new(rx, dropped, self.link.clone(), id))
    }
}
