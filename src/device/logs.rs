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
    /// buffered line is dropped, so a slow consumer can never stall the reader. Lines arrive in the
    /// order the box sent them.
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
        let _rx2 = device.logs(); // a second clone exists; the first still drains the stream
        mock.push_frame(FrameType::Log, 0, &[2, b'z']);
        let line = rx1.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(line.text, "z");
    }

    /// Direct unit test of the overflow policy: on a full channel, `push` evicts the OLDEST line and
    /// enqueues the new one (the buffer holds the most-recent `cap` lines).
    #[test]
    fn push_drops_oldest_on_overflow() {
        let cap = 4;
        let (tx, rx) = flume::bounded::<LogLine>(cap);
        // Fill to capacity with lines "0".."3".
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
        // Two more overflow → evict oldest ("0","1"), leaving "2".."5".
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
