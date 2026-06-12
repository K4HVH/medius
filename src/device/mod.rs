//! The public [`Device`] surface — the concurrency heart of the crate (§5 of the design spec).
//!
//! [`Device`] is `&self`-only and `Send + Sync`, cloned freely (it is an `Arc<Inner>` newtype). All
//! shared state lives in [`Inner`] behind its own `Arc`s, built **before** the background threads are
//! spawned and cloned into them; the threads never hold `Arc<Inner>` itself, so there is no reference
//! cycle and [`Inner`]'s `Drop` can deterministically stop and join them.
//!
//! ## Threads
//!
//! - **Reader thread** — the sole reader of the transport. It loops `transport.read()` (≈100 ms
//!   timeout) → feeds a [`FrameDecoder`] → routes each frame by `TYPE`: `RESP` fulfils the pending
//!   query keyed by `SEQ`; `LOG` is parsed and fanned out on the logs channel; other types are
//!   ignored. It observes the stop flag within one read timeout, so shutdown is bounded (fixing
//!   makcu's lingering reader).
//! - **Keepalive thread** — added in Task 3.6; it sends a cheap frame only while desired-state is
//!   non-idle, to defeat the firmware's 1000 ms silence auto-clear of *intentionally* held state.
//!
//! ## Lock-ordering discipline (deadlock avoidance)
//!
//! The write mutex ([`Inner::write_lock`]) is held **only** around `transport.write_all` and is never
//! held while taking any other lock. The `pending` and `desired` mutexes are short-lived and never
//! nested with each other. A query inserts its sender into `pending`, then sends (taking `write_lock`)
//! — but it releases the `pending` lock before sending, so the two are never held together. This
//! ordering means no two locks are ever held at once, so the layer is deadlock-free by construction.

pub(crate) mod commands;
pub(crate) mod connect;
pub(crate) mod counters;
pub(crate) mod logs;
pub(crate) mod query;
pub(crate) mod reboot;
pub(crate) mod reconcile;

#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::thread::JoinHandle;

use parking_lot::Mutex;

use crate::error::Result;
use crate::protocol::types::LogLine;
use crate::protocol::{FrameDecoder, FrameType, encode, parse_log};
use crate::transport::Transport;

use counters::Counters;
use reconcile::DesiredState;

pub use counters::CountersSnapshot;

/// How long the reader sleeps between drain attempts when a read returns `Ok(0)` *and* the transport
/// has no native blocking timeout (the mock). Real serial reads block up to ≈100 ms themselves.
const READER_IDLE_POLL: std::time::Duration = std::time::Duration::from_millis(2);

/// The shared, reference-counted interior of a [`Device`].
///
/// Each piece of shared state is its own `Arc` so it can be cloned into the reader/keepalive threads
/// independently of `Inner` (see the [module docs](self#threads)). `Inner`'s `Drop` stops and joins
/// both threads.
pub(crate) struct Inner {
    /// The byte pipe to the box, shared with the reader thread.
    transport: Arc<dyn Transport>,
    /// Held **only** around `transport.write_all` so two senders never interleave a frame's bytes.
    /// Never held while locking `pending`/`desired` (see the lock-ordering note in the module docs).
    write_lock: Mutex<()>,
    /// Rolling `SEQ` allocator; `fetch_add(1)` wraps at 255 → 0.
    seq: AtomicU8,
    /// In-flight `QUERY`→`RESP` correlation: `SEQ` → a bounded(1) one-shot the reader fulfils.
    pending: Arc<Mutex<HashMap<u8, flume::Sender<Vec<u8>>>>>,
    /// Producer half of the device LOG fan-out (bounded; oldest dropped on a full, non-draining
    /// consumer — see [`logs`]). Kept for the reconnect path (Task 3.6) to re-spawn the reader.
    #[allow(dead_code)] // consumed by reconnect (Task 3.6)
    logs_tx: flume::Sender<LogLine>,
    /// Consumer half handed out by [`Device::logs`].
    logs_rx: flume::Receiver<LogLine>,
    /// Intended button overrides; the keepalive + reconnect-reapply act on this (Task 3.6).
    #[allow(dead_code)] // read by the keepalive thread (Task 3.6)
    desired: Arc<Mutex<DesiredState>>,
    /// Always-on atomic counters.
    counters: Arc<Counters>,
    /// Set on drop / disconnect; the reader and keepalive observe it and exit.
    stop: Arc<AtomicBool>,
    /// The reader thread handle, joined on drop.
    reader: Option<JoinHandle<()>>,
    /// The keepalive thread handle, joined on drop (Task 3.6).
    keepalive: Option<JoinHandle<()>>,
}

impl std::fmt::Debug for Inner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Inner")
            .field("transport", &self.transport)
            .field("seq", &self.seq.load(Ordering::Relaxed))
            .field("pending", &self.pending.lock().len())
            .field("counters", &self.counters.snapshot())
            .field("stopped", &self.stop.load(Ordering::Relaxed))
            .finish_non_exhaustive()
    }
}

impl Drop for Inner {
    fn drop(&mut self) {
        // Signal both threads, then join. The reader's read timeout bounds how long it can take to
        // notice (≈100 ms real / a few ms mock), so this never hangs.
        self.stop.store(true, Ordering::SeqCst);
        if let Some(h) = self.reader.take() {
            let _ = h.join();
        }
        if let Some(h) = self.keepalive.take() {
            let _ = h.join();
        }
    }
}

/// The host control handle for one medius box.
///
/// `Device` is `&self`-only, `Send + Sync`, and cheap to [`Clone`] (it is an `Arc<Inner>`). Cloning
/// yields another handle to the *same* box and background threads; the threads stop and join when the
/// last clone is dropped.
///
/// Construct it with [`Device::open`] (a path), [`Device::find`] (VID/PID scan), or — for tests and
/// internal use — [`Device::from_transport`] (no handshake).
#[derive(Clone, Debug)]
pub struct Device {
    inner: Arc<Inner>,
}

impl Device {
    /// Wrap an already-open transport, spawn the reader thread, and return the device — **without**
    /// any handshake. This is the seam used by tests (with the mock) and by [`Device::open`]
    /// internally (which then performs the handshake).
    pub(crate) fn from_transport(transport: Arc<dyn Transport>) -> Device {
        // Build every piece of shared state as its own Arc BEFORE spawning, so the thread captures
        // clones of exactly what it needs — never `Arc<Inner>` (which would form a cycle and block
        // Drop's join).
        let pending: Arc<Mutex<HashMap<u8, flume::Sender<Vec<u8>>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let (logs_tx, logs_rx) = flume::bounded(logs::LOGS_CAPACITY);
        let counters = Arc::new(Counters::default());
        let stop = Arc::new(AtomicBool::new(false));
        let desired = Arc::new(Mutex::new(DesiredState::default()));

        let reader = spawn_reader(
            Arc::clone(&transport),
            Arc::clone(&pending),
            logs_tx.clone(),
            logs_rx.clone(),
            Arc::clone(&counters),
            Arc::clone(&stop),
        );

        Device {
            inner: Arc::new(Inner {
                transport,
                write_lock: Mutex::new(()),
                seq: AtomicU8::new(0),
                pending,
                logs_tx,
                logs_rx,
                desired,
                counters,
                stop,
                reader: Some(reader),
                keepalive: None,
            }),
        }
    }

    /// Allocate the next rolling `SEQ` (wraps 255 → 0).
    pub(crate) fn next_seq(&self) -> u8 {
        self.inner.seq.fetch_add(1, Ordering::Relaxed)
    }

    /// Encode and write one frame with an explicit `SEQ`. Fire-and-go: returns once the bytes are
    /// flushed to the transport.
    ///
    /// Holds [`Inner::write_lock`] **only** around `transport.write_all` (never while holding another
    /// lock) so concurrent senders cannot interleave a frame.
    pub(crate) fn send_with_seq(&self, seq: u8, ty: FrameType, payload: &[u8]) -> Result<()> {
        let frame = encode(ty, seq, payload)?; // FrameError → Error::FrameTooLong via `?`
        {
            let _guard = self.inner.write_lock.lock();
            self.inner.transport.write_all(&frame)?;
        }
        self.inner.counters.inc_tx();
        Ok(())
    }

    /// Allocate a fresh `SEQ` and fire one frame (the common fire-and-go path).
    #[cfg_attr(not(test), allow(dead_code))] // the command surface (Task 3.3) is the lib-side user
    pub(crate) fn send(&self, ty: FrameType, payload: &[u8]) -> Result<()> {
        let seq = self.next_seq();
        self.send_with_seq(seq, ty, payload)
    }

    /// A snapshot of the always-on counters.
    pub fn counters(&self) -> CountersSnapshot {
        self.inner.counters.snapshot()
    }

    // ---- internal accessors used by the sibling command/query/reconcile modules ----

    pub(crate) fn pending(&self) -> &Mutex<HashMap<u8, flume::Sender<Vec<u8>>>> {
        &self.inner.pending
    }

    /// The intended-state map, shared by the command surface (Task 3.3) and the keepalive/reconnect
    /// reconcile (Task 3.6).
    #[allow(dead_code)] // first user is the command surface (Task 3.3)
    pub(crate) fn desired(&self) -> &Mutex<DesiredState> {
        &self.inner.desired
    }
}

/// Spawn the reader thread. It owns clones of exactly the shared state it touches — never the whole
/// `Inner` — so it cannot keep `Inner` alive against its own `Drop`.
#[allow(clippy::too_many_arguments)]
fn spawn_reader(
    transport: Arc<dyn Transport>,
    pending: Arc<Mutex<HashMap<u8, flume::Sender<Vec<u8>>>>>,
    logs_tx: flume::Sender<LogLine>,
    logs_rx: flume::Receiver<LogLine>,
    counters: Arc<Counters>,
    stop: Arc<AtomicBool>,
) -> JoinHandle<()> {
    std::thread::Builder::new()
        .name("medius-reader".into())
        .spawn(move || reader_loop(&transport, &pending, &logs_tx, &logs_rx, &counters, &stop))
        .expect("spawn medius-reader thread")
}

/// The reader loop body (factored out so it stays readable and testable in isolation).
#[allow(clippy::too_many_arguments)]
fn reader_loop(
    transport: &Arc<dyn Transport>,
    pending: &Mutex<HashMap<u8, flume::Sender<Vec<u8>>>>,
    logs_tx: &flume::Sender<LogLine>,
    logs_rx: &flume::Receiver<LogLine>,
    counters: &Counters,
    stop: &AtomicBool,
) {
    let mut decoder = FrameDecoder::new();
    let mut buf = [0u8; 1024];

    loop {
        if stop.load(Ordering::SeqCst) {
            return;
        }
        match transport.read(&mut buf) {
            Ok(0) => {
                // Read timeout (or empty mock queue): nothing to do but re-check `stop`. A tiny
                // sleep avoids a busy-spin against a mock whose read returns instantly; a real
                // serial read already blocks ≈100 ms, making this a no-op there.
                std::thread::sleep(READER_IDLE_POLL);
            }
            Ok(n) => {
                decoder.feed(&buf[..n], |frame| {
                    route_frame(frame, pending, logs_tx, logs_rx, counters);
                });
                // Mirror the decoder's running CRC-drop total into the counters.
                counters.set_crc_drops(decoder.crc_error_count());
            }
            Err(_) => {
                // A real I/O error / disconnect: the port is gone. Stop reading; `reconnect`
                // (Task 3.6) can rebuild the reader. Mark stop so a racing Drop doesn't re-join a
                // dead thread oddly (idempotent).
                stop.store(true, Ordering::SeqCst);
                return;
            }
        }
    }
}

/// Route one decoded frame by `TYPE` (§5: RESP→pending, LOG→fan-out, others ignored).
fn route_frame(
    frame: crate::protocol::DecodedFrame,
    pending: &Mutex<HashMap<u8, flume::Sender<Vec<u8>>>>,
    logs_tx: &flume::Sender<LogLine>,
    logs_rx: &flume::Receiver<LogLine>,
    counters: &Counters,
) {
    counters.inc_rx();
    match frame.ty {
        FrameType::Resp => {
            // Correlate by SEQ: remove the one-shot and deliver the payload. An absent SEQ (timed
            // out / duplicate) is simply dropped.
            let tx = pending.lock().remove(&frame.seq);
            if let Some(tx) = tx {
                let _ = tx.send(frame.payload);
            }
        }
        FrameType::Log => {
            let line = parse_log(&frame.payload);
            // Bounded channel: on a full queue we drop the OLDEST line then push, so a non-draining
            // consumer can never OOM the reader while still seeing the most recent logs.
            logs::push(logs_tx, logs_rx, line);
        }
        // MOVE/WHEEL/BUTTON/RESET/QUERY/REBOOT_DL are PC→box; a box that ever echoes one, or any
        // other type, is ignored (forward-compat, §2).
        _ => {}
    }
}
