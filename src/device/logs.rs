//! Device `LOG` frame fan-out (§4.3).
//!
//! The reader pushes each decoded [`LogLine`] onto a **bounded** `flume` channel;
//! [`Device::logs`](crate::Device::logs) hands callers a [`LogStream`] over a clone of the receiver. The
//! bound is the safety property: a consumer that never drains must not let the reader allocate without
//! limit.
//!
//! Overflow policy is drop-the-oldest: a live, recent view is more useful for diagnostics than a stale
//! prefix, and the reader never blocks or grows unbounded.

use std::time::Duration;

use crate::error::{Error, Result};
use crate::protocol::types::LogLine;

use super::Device;

/// Bounded capacity of the device-LOG fan-out channel.
pub(crate) const LOGS_CAPACITY: usize = 1024;

/// A receiver for the device `LOG` stream (§4.3).
///
/// Wraps a clone of the bounded fan-out channel's receiver; callers may each hold one (the channel is
/// MPMC). On overflow the **oldest** buffered line is dropped, so a slow consumer can never stall the
/// reader. Lines arrive in the order the box sent them. The wrapper keeps the underlying channel crate
/// out of the public signatures, so a consumer never has to name it.
#[derive(Clone, Debug)]
pub struct LogStream(flume::Receiver<LogLine>);

impl LogStream {
    /// Block until the next `LOG` line arrives.
    ///
    /// # Errors
    /// [`Error::Disconnected`] once the device is dropped and the channel closes.
    pub fn recv(&self) -> Result<LogLine> {
        self.0.recv().map_err(|_| Error::Disconnected)
    }

    /// The next buffered `LOG` line, or `None` if none is queued (never blocks). `None` also once the
    /// channel has closed.
    pub fn try_recv(&self) -> Option<LogLine> {
        self.0.try_recv().ok()
    }

    /// Block up to `timeout` for the next `LOG` line; `None` on timeout (or a closed channel).
    pub fn recv_timeout(&self, timeout: Duration) -> Option<LogLine> {
        self.0.recv_timeout(timeout).ok()
    }

    /// Drain every currently-buffered `LOG` line without blocking.
    pub fn try_iter(&self) -> impl Iterator<Item = LogLine> + '_ {
        self.0.try_iter()
    }
}

impl IntoIterator for LogStream {
    type Item = LogLine;
    type IntoIter = flume::IntoIter<LogLine>;

    /// A blocking iterator that yields each `LOG` line until the device is dropped and the channel closes.
    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl Device {
    /// A [`LogStream`] over the device `LOG` stream (§4.3).
    ///
    /// Each call hands out an independent stream over a clone of the bounded channel's receiver (the
    /// channel is MPMC, so every clone sees the stream). On overflow the **oldest** buffered line is
    /// dropped, so a slow consumer can never stall the reader. Lines arrive in the order the box sent them.
    pub fn logs(&self) -> LogStream {
        LogStream(self.inner.logs_rx.clone())
    }
}

/// Push `line`, evicting the oldest buffered line if the channel is full. Never blocks the reader.
///
/// `evict_rx` is a reader-held receiver clone used only to make room. Eviction never starves a real
/// consumer: flume is MPMC, so at worst one line is consumed by the reader under sustained overflow.
pub(crate) fn push(
    logs_tx: &flume::Sender<LogLine>,
    evict_rx: &flume::Receiver<LogLine>,
    line: LogLine,
) {
    match logs_tx.try_send(line) {
        Ok(()) => {}
        Err(flume::TrySendError::Full(line)) => {
            // Drop the oldest, then enqueue. The reader is the only producer, so this retry always succeeds.
            let _ = evict_rx.try_recv();
            let _ = logs_tx.try_send(line);
        }
        // Unreachable while `Inner` holds a receiver.
        Err(flume::TrySendError::Disconnected(_)) => {}
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use crate::protocol::FrameType;
    use crate::protocol::types::{LogLevel, LogLine};
    use crate::transport::mock::MockTransport;

    use super::*;

    /// Pushed LOG frames surface on `logs()` in the order the box sent them.
    #[test]
    fn logs_arrive_in_order() {
        let mock = Arc::new(MockTransport::new());
        let device = Device::from_transport(mock.clone());
        let rx = device.logs();

        let levels = [
            (0u8, LogLevel::Error),
            (1, LogLevel::Warn),
            (2, LogLevel::Info),
            (3, LogLevel::Debug),
            (4, LogLevel::Verbose),
        ];
        for (i, (lvl, _)) in levels.iter().enumerate() {
            mock.push_frame(FrameType::Log, i as u8, &[*lvl, b'a' + i as u8]);
        }

        for (i, (_, expect_level)) in levels.iter().enumerate() {
            let line = rx.recv_timeout(Duration::from_secs(1)).unwrap();
            assert_eq!(line.level, *expect_level);
            assert_eq!(line.text, ((b'a' + i as u8) as char).to_string());
        }
    }

    /// `logs()` hands out independent receiver clones (flume MPMC) — each sees the stream.
    #[test]
    fn logs_returns_independent_receiver_clones() {
        let mock = Arc::new(MockTransport::new());
        let device = Device::from_transport(mock.clone());
        let rx1 = device.logs();
        let _rx2 = device.logs(); // second clone exists; the first still drains
        mock.push_frame(FrameType::Log, 0, &[2, b'z']);
        let line = rx1.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(line.text, "z");
    }

    /// On a full channel, `push` evicts the oldest line so the buffer holds the most-recent `cap` lines.
    #[test]
    fn push_drops_oldest_on_overflow() {
        let cap = 4;
        let (tx, rx) = flume::bounded::<LogLine>(cap);
        // Fill to capacity with "0".."3".
        for i in 0..cap {
            push(
                &tx,
                &rx,
                LogLine {
                    level: LogLevel::Info,
                    text: i.to_string(),
                },
            );
        }
        // Two more overflow → evict "0","1", leaving "2".."5".
        for i in cap..cap + 2 {
            push(
                &tx,
                &rx,
                LogLine {
                    level: LogLevel::Info,
                    text: i.to_string(),
                },
            );
        }

        let drained: Vec<String> = std::iter::from_fn(|| rx.try_recv().ok())
            .map(|l| l.text)
            .collect();
        assert_eq!(drained, vec!["2", "3", "4", "5"]);
    }
}
