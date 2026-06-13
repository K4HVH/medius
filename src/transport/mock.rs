//! An in-memory [`Transport`] for hardware-free tests.

#![cfg_attr(not(test), allow(dead_code))]

use std::collections::VecDeque;
use std::io;

use parking_lot::Mutex;

use super::Transport;
use crate::protocol::{FrameDecoder, FrameType, encode};

type Responder = Box<dyn Fn(FrameType, u8, &[u8]) -> Vec<u8> + Send + Sync>;

struct Inner {
    inbound: VecDeque<u8>,
    outbound: Vec<u8>,
    out_decoder: FrameDecoder,
    responder: Option<Responder>,
}

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

    pub(crate) fn with_responder<F>(responder: F) -> Self
    where
        F: Fn(FrameType, u8, &[u8]) -> Vec<u8> + Send + Sync + 'static,
    {
        let mut mock = Self::new();
        mock.inner.get_mut().responder = Some(Box::new(responder));
        mock
    }

    pub(crate) fn push_bytes(&self, bytes: &[u8]) {
        self.inner.lock().inbound.extend(bytes.iter().copied());
    }

    pub(crate) fn push_frame(&self, ty: FrameType, seq: u8, payload: &[u8]) {
        let frame = encode(ty, seq, payload).expect("mock push_frame: payload too long");
        self.push_bytes(&frame);
    }
}

impl Transport for MockTransport {
    fn write_all(&self, buf: &[u8]) -> io::Result<()> {
        let mut inner = self.inner.lock();
        inner.outbound.extend_from_slice(buf);

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
            return Ok(0);
        }
        let n = buf.len().min(inner.inbound.len());
        for slot in buf.iter_mut().take(n) {
            *slot = inner.inbound.pop_front().expect("len checked above");
        }
        Ok(n)
    }
}
