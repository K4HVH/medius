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

/// A swappable transport slot (§6 reconnect). The reader and every writer load the *current*
/// transport (a cheap `Arc` clone) for each operation, so [`reconnect`](Device::reconnect) can replace
/// it in place and all parties follow the swap without restarting threads.
#[derive(Debug)]
pub(crate) struct TransportSlot {
    current: Mutex<Arc<dyn Transport>>,
}

impl TransportSlot {
    fn new(transport: Arc<dyn Transport>) -> Self {
        TransportSlot {
            current: Mutex::new(transport),
        }
    }

    /// The current transport (a clone of the `Arc`; the lock is held only for the clone).
    pub(crate) fn current(&self) -> Arc<dyn Transport> {
        Arc::clone(&self.current.lock())
    }

    /// Replace the transport (reconnect). The old one is dropped (closing its fd/HANDLE) once the last
    /// in-flight `current()` clone is released.
    pub(crate) fn swap(&self, transport: Arc<dyn Transport>) {
        *self.current.lock() = transport;
    }
}

/// The shared, reference-counted interior of a [`Device`].
///
/// Each piece of shared state is its own `Arc` so it can be cloned into the reader/keepalive threads
/// independently of `Inner` (see the [module docs](self#threads)). `Inner`'s `Drop` stops and joins
/// both threads.
pub(crate) struct Inner {
    /// The byte pipe to the box — a swappable slot so [`reconnect`](Device::reconnect) can replace the
    /// underlying transport in place while the reader and writers (which load the current transport
    /// each operation) follow the swap. Shared with the reader thread.
    transport: Arc<TransportSlot>,
    /// Held **only** around `transport.write_all` so two senders never interleave a frame's bytes.
    /// Never held while locking `pending`/`desired` (see the lock-ordering note in the module docs).
    /// An `Arc` so the keepalive thread can serialize against the same write lock.
    write_lock: Arc<Mutex<()>>,
    /// Rolling `SEQ` allocator; `fetch_add(1)` wraps at 255 → 0. An `Arc` so the keepalive thread
    /// draws from the same monotonic sequence.
    seq: Arc<AtomicU8>,
    /// In-flight `QUERY`→`RESP` correlation: `SEQ` → a bounded(1) one-shot the reader fulfils.
    pending: Arc<Mutex<HashMap<u8, flume::Sender<Vec<u8>>>>>,
    /// Consumer half of the device LOG fan-out, handed out (cloned) by [`Device::logs`]. The producer
    /// half lives only in the reader thread; reconnect swaps the transport, not the reader, so no
    /// producer-half copy is needed here.
    logs_rx: flume::Receiver<LogLine>,
    /// Intended button overrides; the keepalive + reconnect-reapply act on this (Task 3.6).
    desired: Arc<Mutex<DesiredState>>,
    /// Always-on atomic counters.
    counters: Arc<Counters>,
    /// Set on drop / disconnect; the reader and keepalive observe it and exit.
    stop: Arc<AtomicBool>,
    /// Default timeout [`query`](Device::query) waits for a `RESP` (from [`ConnectOptions`], §10).
    query_timeout: std::time::Duration,
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
    /// Wrap an already-open transport, spawn the reader **and** keepalive threads, and return the
    /// device — **without** any handshake. This is the seam used by tests (with the mock) and by
    /// [`Device::open`] internally (which then performs the handshake). Uses the default
    /// [`ConnectOptions`](crate::ConnectOptions) (default keepalive cadence + query timeout).
    pub(crate) fn from_transport(transport: Arc<dyn Transport>) -> Device {
        Self::from_transport_with(transport, &crate::ConnectOptions::default())
    }

    /// As [`from_transport`](Device::from_transport) but with an explicit keepalive cadence. Tests use
    /// a short cadence so keepalive behaviour is observable without real 500 ms waits. The query
    /// timeout stays at the [`ConnectOptions`](crate::ConnectOptions) default. (Test-only seam.)
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn from_transport_with_cadence(
        transport: Arc<dyn Transport>,
        keepalive_cadence: std::time::Duration,
    ) -> Device {
        let opts = crate::ConnectOptions::default().with_keepalive_cadence(keepalive_cadence);
        Self::from_transport_with(transport, &opts)
    }

    /// As [`from_transport`](Device::from_transport) but driven by a full
    /// [`ConnectOptions`](crate::ConnectOptions): the keepalive cadence and the query timeout both come
    /// from `opts`. This is the single construction seam the public `open_with`/`find_with`
    /// constructors route through.
    pub(crate) fn from_transport_with(
        transport: Arc<dyn Transport>,
        opts: &crate::ConnectOptions,
    ) -> Device {
        let keepalive_cadence = opts.keepalive_cadence;
        let query_timeout = opts.query_timeout;
        // Build every piece of shared state as its own Arc BEFORE spawning, so each thread captures
        // clones of exactly what it needs — never `Arc<Inner>` (which would form a cycle and block
        // Drop's join).
        let pending: Arc<Mutex<HashMap<u8, flume::Sender<Vec<u8>>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let (logs_tx, logs_rx) = flume::bounded(logs::LOGS_CAPACITY);
        let counters = Arc::new(Counters::default());
        let stop = Arc::new(AtomicBool::new(false));
        let desired = Arc::new(Mutex::new(DesiredState::default()));
        let write_lock = Arc::new(Mutex::new(()));
        let seq = Arc::new(AtomicU8::new(0));
        let transport = Arc::new(TransportSlot::new(transport));

        let reader = spawn_reader(
            Arc::clone(&transport),
            Arc::clone(&pending),
            logs_tx.clone(),
            logs_rx.clone(),
            Arc::clone(&counters),
            Arc::clone(&stop),
        );

        // The keepalive shares the *write* state (transport, write_lock, seq, counters) and `desired`,
        // never `Arc<Inner>` — same anti-cycle discipline as the reader.
        let keepalive = reboot::spawn_keepalive(reboot::KeepaliveCtx {
            transport: Arc::clone(&transport),
            write_lock: Arc::clone(&write_lock),
            seq: Arc::clone(&seq),
            counters: Arc::clone(&counters),
            desired: Arc::clone(&desired),
            stop: Arc::clone(&stop),
            cadence: keepalive_cadence,
        });

        Device {
            inner: Arc::new(Inner {
                transport,
                write_lock,
                seq,
                pending,
                logs_rx,
                desired,
                counters,
                stop,
                query_timeout,
                reader: Some(reader),
                keepalive: Some(keepalive),
            }),
        }
    }

    /// The configured default query timeout (from [`ConnectOptions`](crate::ConnectOptions)).
    pub(crate) fn query_timeout_default(&self) -> std::time::Duration {
        self.inner.query_timeout
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
        write_frame(
            &self.inner.transport,
            &self.inner.write_lock,
            &self.inner.counters,
            seq,
            ty,
            payload,
        )
    }

    /// Allocate a fresh `SEQ` and fire one frame (the common fire-and-go path).
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

    /// The number of in-flight query waiters (diagnostic; used by the async timeout test to assert no
    /// leak). Always available — cheap.
    pub fn pending_len(&self) -> usize {
        self.inner.pending.lock().len()
    }

    /// The intended-state map, shared by the command surface and the keepalive/reconnect reconcile.
    pub(crate) fn desired(&self) -> &Mutex<DesiredState> {
        &self.inner.desired
    }

    /// The swappable transport slot (for [`reconnect`](Device::reconnect)).
    pub(crate) fn transport_slot(&self) -> &Arc<TransportSlot> {
        &self.inner.transport
    }

    /// The reconnects counter (bumped by [`reconnect`](Device::reconnect)).
    pub(crate) fn counters_inner(&self) -> &Counters {
        &self.inner.counters
    }
}

/// Encode and write one frame, serialized by `write_lock` (held **only** around `write_all`). Used by
/// [`Device::send_with_seq`] and the keepalive thread — both go through the swappable transport slot
/// so a reconnect redirects them. Bumps `frames_tx` on success.
fn write_frame(
    transport: &TransportSlot,
    write_lock: &Mutex<()>,
    counters: &Counters,
    seq: u8,
    ty: FrameType,
    payload: &[u8],
) -> Result<()> {
    let frame = encode(ty, seq, payload)?; // FrameError → Error::FrameTooLong via `?`
    let current = transport.current();
    {
        let _guard = write_lock.lock();
        current.write_all(&frame)?;
    }
    counters.inc_tx();
    // Per-frame TX at TRACE only (timing-perturbing; never on the pacer's aggregate-only path).
    trace_event!(
        target: "medius::transport",
        tracing::Level::TRACE,
        dir = "tx",
        opcode = u8::from(ty),
        seq,
        len = payload.len(),
    );
    Ok(())
}

/// Spawn the reader thread. It owns clones of exactly the shared state it touches — never the whole
/// `Inner` — so it cannot keep `Inner` alive against its own `Drop`.
#[allow(clippy::too_many_arguments)]
fn spawn_reader(
    transport: Arc<TransportSlot>,
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

/// Back-off after a read error so the reader doesn't busy-spin on a dead port while waiting for a
/// [`reconnect`](Device::reconnect) swap to install a fresh transport.
const READER_ERROR_BACKOFF: std::time::Duration = std::time::Duration::from_millis(20);

/// The reader loop body (factored out so it stays readable and testable in isolation).
///
/// It loads the *current* transport from the slot each iteration, so after a reconnect swaps the slot
/// the very same reader thread follows onto the new transport — no thread restart needed. The loop
/// exits **only** on the `stop` flag (a read error backs off and retries, since the port may be about
/// to be reconnected), so shutdown stays deterministic via Drop.
#[allow(clippy::too_many_arguments)]
fn reader_loop(
    transport: &TransportSlot,
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
        let current = transport.current();
        match current.read(&mut buf) {
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
                // A read error means the current port is gone or hiccuping. Back off and retry rather
                // than exit: `reconnect` may be about to swap in a fresh transport, and the same
                // reader should follow onto it. Drop's `stop` still ends the loop promptly.
                std::thread::sleep(READER_ERROR_BACKOFF);
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
    // Per-frame RX at TRACE only (timing-perturbing; documented in trace.rs).
    trace_event!(
        target: "medius::transport",
        tracing::Level::TRACE,
        dir = "rx",
        opcode = u8::from(frame.ty),
        seq = frame.seq,
        len = frame.payload.len(),
    );
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
            // Re-emit the device LOG as a host tracing event (LOG level → tracing level), under
            // `medius::device` — additional to the logs() channel, which still receives it below.
            #[cfg(feature = "tracing")]
            crate::trace::emit_device_log(&line);
            // Bounded channel: on a full queue we drop the OLDEST line then push, so a non-draining
            // consumer can never OOM the reader while still seeing the most recent logs.
            logs::push(logs_tx, logs_rx, line);
        }
        // MOVE/WHEEL/BUTTON/RESET/QUERY/REBOOT_DL are PC→box; a box that ever echoes one, or any
        // other type, is ignored (forward-compat, §2).
        _ => {}
    }
}
