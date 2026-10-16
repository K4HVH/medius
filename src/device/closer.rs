use std::fmt::Debug;
use std::sync::Weak;

/// What a [`StreamCloser`] ends.
pub(crate) trait Close: Send + Sync + Debug {
    fn close(&self);
}

/// Closes a stream from a thread holding none of its handles (such as after the stream moved into
/// the thread receiving on it).
///
/// [`close`](Self::close) ends the stream like the stream's `close` method: every blocked receive
/// returns at once, as does every later one. The closer holds the stream weakly, so it keeps no
/// subscription open. Closing a stream whose handles have all dropped does nothing.
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
