use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use crate::error::{Error, Result};
use crate::link::Link;
use crate::types::{CatchMask, InputReport};

use super::Device;

/// A live stream of physical-input [`InputReport`]s from the box (the `CATCH` feature, §3.9).
///
/// Created by [`Device::catch_events`]. Cloning shares the queue (like [`LogStream`](crate::LogStream));
/// for an independent stream, call [`catch_events`](Device::catch_events) again. The subscription ends
/// when the stream and all its clones drop, and the box returns to pure passthrough. The buffer is
/// bounded and lossy: if the consumer falls behind, the newest events are dropped and counted in
/// [`dropped`](Self::dropped).
#[derive(Clone, Debug)]
pub struct EventStream {
    rx: flume::Receiver<InputReport>,
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
        rx: flume::Receiver<InputReport>,
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

    /// Block until the next physical-input report arrives.
    pub fn recv(&self) -> Result<InputReport> {
        self.rx.recv().map_err(|_| Error::Disconnected)
    }

    /// The next buffered report, or `None` if none is queued (never blocks).
    pub fn try_recv(&self) -> Option<InputReport> {
        self.rx.try_recv().ok()
    }

    /// Block up to `timeout` for the next report; `None` on timeout (or a closed channel).
    pub fn recv_timeout(&self, timeout: Duration) -> Option<InputReport> {
        self.rx.recv_timeout(timeout).ok()
    }

    /// Drain every currently-buffered report without blocking.
    pub fn try_iter(&self) -> impl Iterator<Item = InputReport> + '_ {
        self.rx.try_iter()
    }

    /// Await the next report. Runtime-agnostic (the same `flume` channel as the sync methods), so it
    /// runs under any executor. Available with the `async` feature.
    #[cfg(feature = "async")]
    pub async fn recv_async(&self) -> Result<InputReport> {
        self.rx.recv_async().await.map_err(|_| Error::Disconnected)
    }

    /// Events this stream dropped because the consumer fell behind (host-side back-pressure). The
    /// box-side drop count is [`CatchState::dropped`](crate::CatchState), from `query_catch`.
    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }
}

impl Device {
    /// Subscribe to the physical-input event stream for the given classes (the `CATCH` feature, §3.9).
    ///
    /// The box streams the user's real mouse input — buttons, wheel, and X/Y — as it happens, even on
    /// targets you've locked or are injecting on (the report is captured before suppression). The
    /// returned [`EventStream`] receives every report; dropping it unsubscribes. Combine classes with
    /// `|`, or pass [`CatchMask::all`] for the full mirror. The subscription is held alive by the
    /// library's keepalive and re-asserted across a reconnect; it clears on its own if the host goes
    /// silent (§5.4).
    ///
    /// ```no_run
    /// # use medius::{Device, CatchMask, Button};
    /// # fn main() -> medius::Result<()> {
    /// let device = Device::find()?;
    /// let events = device.catch_events(CatchMask::all())?;
    /// while let Ok(report) = events.recv() {
    ///     if report.is_pressed(Button::Side1) {
    ///         // rebind the side button...
    ///     }
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub fn catch_events(&self, mask: CatchMask) -> Result<EventStream> {
        let (id, rx, dropped) = self.link.catch_subscribe(mask)?;
        Ok(EventStream::new(rx, dropped, self.link.clone(), id))
    }
}
