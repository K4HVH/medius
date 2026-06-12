//! Device `LOG` frame fan-out (§4.3).
//!
//! The reader thread parses each `LOG` frame and pushes a [`LogLine`] onto a **bounded** `flume`
//! channel; [`Device::logs`](crate::Device::logs) hands callers a clone of the receiver. Bounding the
//! channel is the safety property: a consumer that never drains must not let the reader allocate
//! without limit.
//!
//! **Overflow policy — drop the oldest.** When the channel is full, [`push`] removes the oldest
//! buffered line (one non-blocking `try_recv` on a reader-held receiver clone) and then enqueues the
//! new one. A live, recent view of the device log is more useful for diagnostics than a stale prefix,
//! and the reader never blocks or grows unbounded. (The alternative — dropping the *new* line — would
//! freeze the visible log at the moment the consumer stalled; we prefer freshness.)

use crate::protocol::types::LogLine;

use super::Device;

/// Bounded capacity of the device-LOG fan-out channel.
pub(crate) const LOGS_CAPACITY: usize = 1024;

impl Device {
    /// A receiver for the device `LOG` stream (§4.3).
    ///
    /// Returns a clone of the bounded channel's receiver; multiple callers may each hold one (flume
    /// is MPMC). The reader thread pushes every decoded `LOG` line here; on overflow the **oldest**
    /// buffered line is dropped (see the [module docs](self)). Lines arrive in the order the box sent
    /// them.
    pub fn logs(&self) -> flume::Receiver<LogLine> {
        self.inner.logs_rx.clone()
    }
}

/// Push `line` onto the bounded logs channel, evicting the **oldest** buffered line if the channel is
/// full (see the [module docs](self)). Never blocks the reader.
///
/// `evict_rx` is a receiver clone the reader holds purely to make room on a full channel; pulling one
/// item off it is non-blocking. Eviction never starves a real consumer: `flume` is MPMC, so the
/// evicting `try_recv` and a consumer's `recv` race fairly, and at worst one line is consumed by the
/// reader instead of the consumer under sustained overflow.
pub(crate) fn push(
    logs_tx: &flume::Sender<LogLine>,
    evict_rx: &flume::Receiver<LogLine>,
    line: LogLine,
) {
    match logs_tx.try_send(line) {
        Ok(()) => {}
        Err(flume::TrySendError::Full(line)) => {
            // Full: drop the oldest to make room, then enqueue the new line. The reader is the only
            // producer, so a single eviction + retry always succeeds.
            let _ = evict_rx.try_recv();
            let _ = logs_tx.try_send(line);
        }
        Err(flume::TrySendError::Disconnected(_)) => {
            // Unreachable while `Inner` holds a receiver; nothing to do if it ever happens.
        }
    }
}
