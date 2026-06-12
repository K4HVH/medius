//! An in-memory [`Transport`] for hardware-free tests (the test/seam transport).
//!
//! [`MockTransport`] is interior-mutable (a [`parking_lot::Mutex`] inside) so it satisfies the
//! `&self` trait while being driven from a test. It does two things:
//!
//! - **Inbound:** a test pushes raw bytes ([`MockTransport::push_bytes`]) or whole encoded frames
//!   ([`MockTransport::push_frame`]); [`Transport::read`] then drains them. An empty queue reads as
//!   `Ok(0)` — simulating the real transport's read timeout (so a reader thread keeps polling).
//! - **Outbound:** every [`Transport::write_all`] is appended to a capture buffer that a test drains
//!   with [`MockTransport::written`] and decodes with [`crate::protocol::FrameDecoder`].
//!
//! ## Responder seam (for Milestone 5's `mock` feature)
//!
//! [`MockTransport::with_responder`] installs an optional callback invoked on every fully decoded
//! outbound frame; whatever bytes it returns are queued as if the box had replied. This is the clean
//! seam the public "programmable box" mode (auto-answering `QUERY(VERSION)`/`QUERY(HEALTH)`) will
//! build on — but the full programmable box is not implemented here.

// The mock is purely a test double until the `mock` feature (Milestone 5) re-exports it publicly;
// its scripting seams (`push_*`/`written`/`with_responder`) are only driven by tests, so the lib
// build legitimately sees them as unused. Scoped to this module so a real unused item elsewhere is
// still caught.
#![cfg_attr(not(test), allow(dead_code))]

use std::collections::VecDeque;
use std::io;

use parking_lot::Mutex;

use super::Transport;
use crate::protocol::{FrameDecoder, FrameType, encode};

/// A callback invoked on each decoded outbound frame; its return bytes are queued as an inbound
/// reply. This is the seam the public programmable-box mode (Milestone 5) builds on.
type Responder = Box<dyn Fn(FrameType, u8, &[u8]) -> Vec<u8> + Send + Sync>;

/// Mutable interior of [`MockTransport`], guarded by a single mutex.
struct Inner {
    /// Bytes the box "sends" to the host; drained by [`Transport::read`].
    inbound: VecDeque<u8>,
    /// Bytes the host wrote; captured for assertion via [`MockTransport::written`].
    outbound: Vec<u8>,
    /// Decoder for the outbound stream, used only when a responder is installed.
    out_decoder: FrameDecoder,
    /// Optional auto-responder (the programmable-box seam).
    responder: Option<Responder>,
}

/// An in-memory transport for tests and the (future) public scriptable mock box.
///
/// See the [module docs](self) for the inbound/outbound model and the responder seam.
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

    /// Create a mock whose `responder` is invoked on every decoded outbound frame; the bytes it
    /// returns are queued as inbound (as if the box replied). The seam for the programmable box.
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

    /// Encode a frame (via [`crate::protocol::encode`]) and queue it inbound.
    ///
    /// # Panics
    /// Panics if `payload` exceeds [`crate::protocol::MAX_PAYLOAD`] — tests control their inputs, so
    /// an over-length payload is a test bug, not a runtime condition.
    pub(crate) fn push_frame(&self, ty: FrameType, seq: u8, payload: &[u8]) {
        let frame = encode(ty, seq, payload).expect("mock push_frame: payload too long");
        self.push_bytes(&frame);
    }

    /// Drain and return all bytes the host has written so far (decodable with a
    /// [`crate::protocol::FrameDecoder`]). Leaves the capture buffer empty.
    pub(crate) fn written(&self) -> Vec<u8> {
        std::mem::take(&mut self.inner.lock().outbound)
    }
}

impl Transport for MockTransport {
    fn write_all(&self, buf: &[u8]) -> io::Result<()> {
        let mut inner = self.inner.lock();
        inner.outbound.extend_from_slice(buf);

        // If a responder is installed, feed the outbound bytes through a decoder and queue any
        // reply it produces. We avoid borrowing `inner` twice by collecting replies first.
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
            // Empty queue == read timeout (no data within the window).
            return Ok(0);
        }
        let n = buf.len().min(inner.inbound.len());
        for slot in buf.iter_mut().take(n) {
            *slot = inner.inbound.pop_front().expect("len checked above");
        }
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{DecodedFrame, command::move_payload};

    /// Decode every frame captured by `written()`.
    fn decode_all(bytes: &[u8]) -> Vec<DecodedFrame> {
        FrameDecoder::new().feed_collect(bytes)
    }

    /// A written frame round-trips: capture the bytes, decode them, assert the fields.
    #[test]
    fn write_frame_then_decode_captured() {
        let mock = MockTransport::new();
        let payload = move_payload(40, -2);
        let frame = encode(FrameType::Move, 0x11, &payload).unwrap();
        mock.write_all(&frame).unwrap();

        let captured = mock.written();
        let frames = decode_all(&captured);
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].ty, FrameType::Move);
        assert_eq!(frames[0].seq, 0x11);
        assert_eq!(frames[0].payload, payload);

        // written() drains: a second call yields nothing.
        assert!(mock.written().is_empty());
    }

    /// Multiple writes accumulate; `written()` returns them in order.
    #[test]
    fn writes_accumulate_in_order() {
        let mock = MockTransport::new();
        mock.write_all(&encode(FrameType::Move, 1, &move_payload(1, 0)).unwrap())
            .unwrap();
        mock.write_all(&encode(FrameType::Wheel, 2, &[3, 0]).unwrap())
            .unwrap();
        let frames = decode_all(&mock.written());
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0].seq, 1);
        assert_eq!(frames[1].seq, 2);
    }

    /// A pushed RESP frame is returned by `read`, and decodes back to the same frame.
    #[test]
    fn push_frame_then_read_yields_it() {
        let mock = MockTransport::new();
        // RESP(VERSION): [what=0][proto_ver][fw_major][fw_minor][fw_patch]
        mock.push_frame(FrameType::Resp, 0x05, &[0, 1, 2, 3, 4]);

        let mut buf = [0u8; 64];
        let n = mock.read(&mut buf).unwrap();
        assert!(n > 0);
        let frames = decode_all(&buf[..n]);
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].ty, FrameType::Resp);
        assert_eq!(frames[0].seq, 0x05);
        assert_eq!(frames[0].payload, vec![0, 1, 2, 3, 4]);
    }

    /// `read` on an empty inbound queue returns `Ok(0)` (timeout simulation), not an error.
    #[test]
    fn read_empty_is_timeout_zero() {
        let mock = MockTransport::new();
        let mut buf = [0u8; 16];
        assert_eq!(mock.read(&mut buf).unwrap(), 0);
    }

    /// A read into a zero-length buffer is `Ok(0)` and consumes nothing.
    #[test]
    fn read_into_empty_buffer_consumes_nothing() {
        let mock = MockTransport::new();
        mock.push_bytes(&[1, 2, 3]);
        let mut empty: [u8; 0] = [];
        assert_eq!(mock.read(&mut empty).unwrap(), 0);
        // The pushed bytes are still there.
        let mut buf = [0u8; 8];
        assert_eq!(mock.read(&mut buf).unwrap(), 3);
        assert_eq!(&buf[..3], &[1, 2, 3]);
    }

    /// `read` honors a small buffer, draining only as many bytes as fit and leaving the rest.
    #[test]
    fn read_respects_small_buffer() {
        let mock = MockTransport::new();
        mock.push_bytes(&[1, 2, 3, 4, 5]);
        let mut small = [0u8; 2];
        assert_eq!(mock.read(&mut small).unwrap(), 2);
        assert_eq!(&small, &[1, 2]);
        let mut rest = [0u8; 8];
        assert_eq!(mock.read(&mut rest).unwrap(), 3);
        assert_eq!(&rest[..3], &[3, 4, 5]);
    }

    /// The responder seam: a decoded outbound QUERY frame triggers a queued inbound reply that a
    /// subsequent `read` returns. (A miniature of the Milestone 5 programmable box.)
    #[test]
    fn responder_auto_answers_query() {
        let mock = MockTransport::with_responder(|ty, seq, payload| {
            // Answer QUERY(VERSION) with a RESP echoing the SEQ.
            if ty == FrameType::Query && payload.first() == Some(&0) {
                encode(FrameType::Resp, seq, &[0, 1, 9, 9, 9]).unwrap()
            } else {
                Vec::new()
            }
        });

        // Host sends QUERY(VERSION) with seq 0x20.
        mock.write_all(&encode(FrameType::Query, 0x20, &[0]).unwrap())
            .unwrap();

        // The responder queued a RESP; read it back.
        let mut buf = [0u8; 64];
        let n = mock.read(&mut buf).unwrap();
        let frames = decode_all(&buf[..n]);
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].ty, FrameType::Resp);
        assert_eq!(frames[0].seq, 0x20);
        assert_eq!(frames[0].payload, vec![0, 1, 9, 9, 9]);
    }

    /// The responder also works when the outbound frame is delivered split across two writes.
    #[test]
    fn responder_handles_split_outbound_writes() {
        let mock = MockTransport::with_responder(|ty, seq, _| {
            if ty == FrameType::Query {
                encode(FrameType::Resp, seq, &[1, 0x0F]).unwrap()
            } else {
                Vec::new()
            }
        });
        let frame = encode(FrameType::Query, 7, &[1]).unwrap();
        let (a, b) = frame.split_at(3);
        mock.write_all(a).unwrap();
        mock.write_all(b).unwrap();

        let mut buf = [0u8; 64];
        let n = mock.read(&mut buf).unwrap();
        let frames = decode_all(&buf[..n]);
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].ty, FrameType::Resp);
        assert_eq!(frames[0].seq, 7);
    }

    /// The mock is `Send + Sync` (required: shared as `Arc<dyn Transport>` across threads).
    #[test]
    fn mock_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<MockTransport>();
    }
}
