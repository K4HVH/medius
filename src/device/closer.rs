use std::fmt::Debug;
use std::sync::Weak;

/// What a [`StreamCloser`] ends.
pub(crate) trait Close: Send + Sync + Debug {
    fn close(&self);
}

/// Closes a stream from a thread that holds none of its handles, such as one whose stream was
/// moved into the thread receiving on it.
///
/// [`close`](Self::close) ends the stream as the stream's `close` method does: every receive
/// blocked on it returns at once, and every receive after it too. It holds the stream weakly, so it
/// keeps no subscription open, and closing one whose every handle has dropped does nothing.
#[derive(Clone, Debug)]
pub struct StreamCloser(pub(crate) Weak<dyn Close>);

impl StreamCloser {
    /// Close the stream, if any handle to it is left.
    pub fn close(&self) {
        if let Some(target) = self.0.upgrade() {
            target.close();
        }
    }
}
