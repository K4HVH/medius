//! An in-memory [`Transport`] for hardware-free tests.
//!
//! [`MockTransport`] is interior-mutable (a [`parking_lot::Mutex`]) to satisfy the `&self` trait.
//! Inbound: a test pushes bytes/frames and [`Transport::read`] drains them; an empty queue reads
//! `Ok(0)`, simulating the real read timeout. Outbound: each [`Transport::write_all`] is captured for
//! a test to drain via [`MockTransport::written`]. [`MockTransport::with_responder`] is the seam the
//! future public "programmable box" mode builds on, but that mode is not implemented here.

// Test double until the `mock` feature re-exports it publicly; its seams are only driven by tests, so
// the lib build sees them unused. Scoped to this module so real unused items elsewhere still fail.
#![cfg_attr(not(test), allow(dead_code))]

use std::collections::VecDeque;
use std::io;

use parking_lot::Mutex;

use super::Transport;
use crate::protocol::{FrameDecoder, FrameType, encode};

/// Callback invoked on each decoded outbound frame; its return bytes are queued as an inbound reply.
type Responder = Box<dyn Fn(FrameType, u8, &[u8]) -> Vec<u8> + Send + Sync>;

struct Inner {
    /// Bytes the box "sends" to the host; drained by [`Transport::read`].
    inbound: VecDeque<u8>,
    /// Bytes the host wrote; captured for assertion via [`MockTransport::written`].
    outbound: Vec<u8>,
    /// Decoder for the outbound stream, used only when a responder is installed.
    out_decoder: FrameDecoder,
    responder: Option<Responder>,
}

/// An in-memory transport for tests and the future public scriptable mock box.
pub(crate) struct MockTransport {
    inner: Mutex<Inner>,
}

impl std::fmt::Debug for MockTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let inner = self.inner.lock();
        f.debug_struct("MockTransport")
            .field("inbound_len", &inner.inbound.len())
            .field("outbound_len", &inner.outbound.len())
            .field("has_responder", &inner.responder.is_some())
            .finish()
    }
}

impl Default for MockTransport {
    fn default() -> Self {
        Self::new()
    }
}

impl MockTransport {
    /// Create an empty mock with no auto-responder.
    pub(crate) fn new() -> Self {
        MockTransport {
            inner: Mutex::new(Inner {
                inbound: VecDeque::new(),
                outbound: Vec::new(),
                out_decoder: FrameDecoder::new(),
                responder: None,
            }),
        }
    }

    /// Create a mock whose `responder` runs on every decoded outbound frame; its return bytes are
    /// queued as inbound, as if the box replied. The seam for the programmable box.
    pub(crate) fn with_responder<F>(responder: F) -> Self
    where
        F: Fn(FrameType, u8, &[u8]) -> Vec<u8> + Send + Sync + 'static,
    {
        let mut mock = Self::new();
        mock.inner.get_mut().responder = Some(Box::new(responder));
        mock
    }

    /// Queue raw `bytes` to be returned by subsequent [`Transport::read`] calls.
    pub(crate) fn push_bytes(&self, bytes: &[u8]) {
        self.inner.lock().inbound.extend(bytes.iter().copied());
    }

    /// Encode a frame and queue it inbound.
    ///
    /// # Panics
    /// If `payload` exceeds [`crate::protocol::MAX_PAYLOAD`] — a test bug, since tests control inputs.
    pub(crate) fn push_frame(&self, ty: FrameType, seq: u8, payload: &[u8]) {
        let frame = encode(ty, seq, payload).expect("mock push_frame: payload too long");
        self.push_bytes(&frame);
    }
}

impl Transport for MockTransport {
    fn write_all(&self, buf: &[u8]) -> io::Result<()> {
        let mut inner = self.inner.lock();
        inner.outbound.extend_from_slice(buf);

        // Feed outbound bytes through the decoder and queue any reply. Replies are collected first to
        // avoid borrowing `inner` twice.
        if inner.responder.is_some() {
            let Inner {
                out_decoder,
                responder,
                ..
            } = &mut *inner;
            let responder = responder.as_ref().expect("checked is_some above");
            let mut replies: Vec<u8> = Vec::new();
            out_decoder.feed(buf, |frame| {
                replies.extend(responder(frame.ty, frame.seq, &frame.payload));
            });
            inner.inbound.extend(replies);
        }
        Ok(())
    }

    fn read(&self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let mut inner = self.inner.lock();
        if inner.inbound.is_empty() {
            // Empty queue == read timeout.
            return Ok(0);
        }
        let n = buf.len().min(inner.inbound.len());
        for slot in buf.iter_mut().take(n) {
            *slot = inner.inbound.pop_front().expect("len checked above");
        }
        Ok(n)
    }
}
