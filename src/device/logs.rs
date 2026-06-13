use std::time::Duration;

use crate::error::{Error, Result};
use crate::types::LogLine;

use super::Device;

/// A receiver for the device `LOG` stream.
#[derive(Clone, Debug)]
pub struct LogStream(flume::Receiver<LogLine>);

impl LogStream {
    /// Block until the next `LOG` line arrives.
    pub fn recv(&self) -> Result<LogLine> {
        self.0.recv().map_err(|_| Error::Disconnected)
    }

    /// The next buffered `LOG` line, or `None` if none is queued (never blocks).
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

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl Device {
    /// A [`LogStream`] over the device `LOG` stream.
    pub fn logs(&self) -> LogStream {
        LogStream(self.link.logs_rx())
    }
}
