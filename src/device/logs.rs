use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use parking_lot::Mutex;

use crate::error::{Error, Result};
use crate::types::LogLine;

use super::Device;
use super::closer::{Close, StreamCloser};

/// Receiver for the device `LOG` stream.
///
/// Every `LogStream` from one [`Device`] reads one shared queue, so each line goes to one of them.
/// A clone shares its stream's [`close`](Self::close).
#[derive(Clone, Debug)]
pub struct LogStream {
    rx: flume::Receiver<LogLine>,
    close: Arc<LogClose>,
}

// The queue is shared with every other LogStream, so closing one cannot end the channel: a receive
// waits on the queue and on this signal, which closing disconnects.
#[derive(Debug)]
struct LogClose {
    closed: AtomicBool,
    signal_tx: Mutex<Option<flume::Sender<()>>>,
    signal_rx: flume::Receiver<()>,
}

impl Close for LogClose {
    fn close(&self) {
        self.closed.store(true, Ordering::Release);
        self.signal_tx.lock().take();
    }
}

impl LogStream {
    fn new(rx: flume::Receiver<LogLine>) -> LogStream {
        let (tx, signal_rx) = flume::bounded(0);
        LogStream {
            rx,
            close: Arc::new(LogClose {
                closed: AtomicBool::new(false),
                signal_tx: Mutex::new(Some(tx)),
                signal_rx,
            }),
        }
    }

    fn closed(&self) -> bool {
        self.close.closed.load(Ordering::Acquire)
    }

    /// Block until the next `LOG` line; `Disconnected` once the box disconnects or the stream is
    /// closed.
    pub fn recv(&self) -> Result<LogLine> {
        if self.closed() {
            return Err(Error::Disconnected);
        }
        flume::Selector::new()
            .recv(&self.rx, |r| r.map_err(|_| Error::Disconnected))
            .recv(&self.close.signal_rx, |_| Err(Error::Disconnected))
            .wait()
    }

    /// Next buffered `LOG` line, or `None` if none is queued. Never blocks.
    pub fn try_recv(&self) -> Option<LogLine> {
        if self.closed() {
            return None;
        }
        self.rx.try_recv().ok()
    }

    /// Block up to `timeout` for the next `LOG` line; `None` on timeout or a closed channel.
    pub fn recv_timeout(&self, timeout: Duration) -> Option<LogLine> {
        if self.closed() {
            return None;
        }
        flume::Selector::new()
            .recv(&self.rx, |r| r.ok())
            .recv(&self.close.signal_rx, |_| None)
            .wait_timeout(timeout)
            .ok()
            .flatten()
    }

    /// Whether the box still delivers to this stream. Separates the two meanings of `None` from
    /// [`Self::recv_timeout`] and [`Self::try_recv`]: "nothing yet" and "nothing ever again".
    pub fn is_connected(&self) -> bool {
        !self.closed() && !self.rx.is_disconnected()
    }

    /// Drain buffered `LOG` lines without blocking.
    pub fn try_iter(&self) -> impl Iterator<Item = LogLine> + '_ {
        std::iter::from_fn(|| self.try_recv())
    }

    /// Await the next `LOG` line; runtime-agnostic (same `flume` channel as the sync methods).
    #[cfg(feature = "async")]
    pub async fn recv_async(&self) -> Result<LogLine> {
        if self.closed() {
            return Err(Error::Disconnected);
        }
        FirstOf {
            line: self.rx.recv_async(),
            closed: self.close.signal_rx.recv_async(),
        }
        .await
    }

    /// End this stream and every clone of it. Every receive blocked on it returns at once, and
    /// every receive after it too. Other `LogStream`s go on reading the queue.
    ///
    /// For a thread that holds no handle to the stream, see [`Self::closer`].
    pub fn close(&self) {
        self.close.close();
    }

    /// A handle that closes this stream from any thread.
    pub fn closer(&self) -> StreamCloser {
        let close: Arc<dyn Close> = self.close.clone();
        StreamCloser(Arc::downgrade(&close))
    }
}

// A line, or the close signal ending the wait, whichever comes first.
#[cfg(feature = "async")]
struct FirstOf<'a> {
    line: flume::r#async::RecvFut<'a, LogLine>,
    closed: flume::r#async::RecvFut<'a, ()>,
}

#[cfg(feature = "async")]
impl std::future::Future for FirstOf<'_> {
    type Output = Result<LogLine>;

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<LogLine>> {
        use std::task::Poll;
        if std::pin::Pin::new(&mut self.closed).poll(cx).is_ready() {
            return Poll::Ready(Err(Error::Disconnected));
        }
        std::pin::Pin::new(&mut self.line)
            .poll(cx)
            .map(|r| r.map_err(|_| Error::Disconnected))
    }
}

impl IntoIterator for LogStream {
    type Item = LogLine;
    type IntoIter = Box<dyn Iterator<Item = LogLine> + Send>;

    fn into_iter(self) -> Self::IntoIter {
        Box::new(std::iter::from_fn(move || self.recv().ok()))
    }
}

impl Device {
    /// [`LogStream`] over the device `LOG` stream.
    pub fn logs(&self) -> LogStream {
        LogStream::new(self.link.logs_rx())
    }
}
