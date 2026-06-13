//! The public [`Device`] surface — the concurrency heart of the crate (§5).
//!
//! [`Device`] is `&self`-only, `Send + Sync`, and a cheap `Arc<Inner>` newtype. All shared state lives
//! in [`Inner`] behind its own `Arc`s, built before the background threads are spawned and cloned into
//! them; the threads never hold `Arc<Inner>` itself, so there is no cycle and [`Inner`]'s `Drop` can
//! deterministically stop and join them.
//!
//! Two threads: the **reader** (sole transport reader; loops read → [`FrameDecoder`] → route by `TYPE`
//! and observes `stop` within one read timeout) and the **keepalive** (sends a cheap frame only while
//! desired-state is non-idle, defeating the firmware's 1000 ms silence auto-clear of held state).
//!
//! Lock-ordering: [`Inner::write_lock`] is held **only** around `transport.write_all`, never while
//! holding another lock; `pending` and `desired` are short-lived and never nested. No two locks are
//! ever held at once, so the layer is deadlock-free by construction.

pub(crate) mod commands;
pub(crate) mod connect;
pub(crate) mod counters;
pub(crate) mod logs;
pub(crate) mod query;
pub(crate) mod reboot;
pub(crate) mod reconcile;

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::thread::JoinHandle;

use parking_lot::Mutex;

use crate::error::Result;
use crate::protocol::{FrameDecoder, FrameType, encode, parse_log};
use crate::transport::Transport;
use crate::types::{CountersSnapshot, LogLine};

use counters::Counters;
use reconcile::DesiredState;

/// Reader sleep between drains when a read returns `Ok(0)` and the transport has no native blocking
/// timeout (the mock). Real serial reads block ≈100 ms themselves, so this is a no-op there.
const READER_IDLE_POLL: std::time::Duration = std::time::Duration::from_millis(2);

/// How long [`query_version`](Device::query_version) / [`query_health`](Device::query_health) wait for
/// the correlated `RESP` before returning [`Error::QueryTimeout`](crate::Error::QueryTimeout). Fixed:
/// on the 4 Mbaud link a query round-trips in well under a millisecond, so a 1 s ceiling is generous
/// for any real reply.
pub const DEFAULT_QUERY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(1);

/// How often the keepalive refreshes a held override to defeat the firmware's 1000 ms silence
/// auto-clear. Fixed sub-1 s so a held button outlives the auto-clear; idle periods stay silent so a
/// real host crash still clears (the no-stuck safety).
pub const DEFAULT_KEEPALIVE_CADENCE: std::time::Duration = std::time::Duration::from_millis(500);

/// One in-flight `QUERY`→`RESP` waiter, tagged with a monotonic `gen` so a stale canceller evicts only
/// its own entry — never a newer query that reused the same wire `SEQ` (see [`Inner::cancel_query`]).
struct PendingEntry {
    gen_id: u64,
    /// The `QUERY` selector this waiter expects echoed back in `RESP[0]`. The reader only fulfils a
    /// waiter whose selector matches, so an unsolicited `RESP` (e.g. the firmware boot/first-contact
    /// VERSION hello, SEQ=0) can never satisfy — and corrupt — a query awaiting a different selector.
    expected_what: u8,
    /// Bounded(1) one-shot the reader fulfils with the correlated `RESP` payload.
    tx: flume::Sender<Vec<u8>>,
}

/// A swappable transport slot (§6 reconnect). The reader and writers load the current transport (a
/// cheap `Arc` clone) per operation, so [`reconnect`](Device::reconnect) can replace it in place and
/// all parties follow without restarting threads. The generation is bumped on each `swap` so the
/// reader resets its [`FrameDecoder`]: a frame interrupted mid-parse on the old port must not mis-frame
/// the first bytes of the new one.
#[derive(Debug)]
pub(crate) struct TransportSlot {
    current: Mutex<Arc<dyn Transport>>,
    generation: AtomicU64,
}

impl TransportSlot {
    fn new(transport: Arc<dyn Transport>) -> Self {
        TransportSlot {
            current: Mutex::new(transport),
            generation: AtomicU64::new(0),
        }
    }

    /// The current transport (a clone of the `Arc`; the lock is held only for the clone).
    pub(crate) fn current(&self) -> Arc<dyn Transport> {
        Arc::clone(&self.current.lock())
    }

    pub(crate) fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    /// Replace the transport and bump the generation so the reader resets its decoder. The old
    /// transport's fd/HANDLE closes once the last in-flight `current()` clone is released.
    pub(crate) fn swap(&self, transport: Arc<dyn Transport>) {
        *self.current.lock() = transport;
        self.generation.fetch_add(1, Ordering::Release);
    }
}

/// The shared, reference-counted interior of a [`Device`].
///
/// Each piece of shared state is its own `Arc` so it can be cloned into the reader/keepalive threads
/// independently of `Inner` (avoiding a cycle). `Inner`'s `Drop` stops and joins both threads.
pub(crate) struct Inner {
    /// Swappable byte pipe to the box; [`reconnect`](Device::reconnect) replaces the transport in place
    /// while the reader and writers follow the swap.
    transport: Arc<TransportSlot>,
    /// Held **only** around `transport.write_all` so two senders never interleave a frame. Never held
    /// while locking `pending`/`desired`.
    write_lock: Arc<Mutex<()>>,
    /// Rolling `SEQ` allocator; `fetch_add(1)` wraps 255 → 0.
    seq: Arc<AtomicU8>,
    /// Monotonic per-query generation tagging each [`PendingEntry`], so a stale canceller can only
    /// evict its own waiter, never a newer query that reused the same wire `SEQ`.
    query_gen: Arc<AtomicU64>,
    /// In-flight `QUERY`→`RESP` correlation: `SEQ` → generation-tagged one-shot. A new query's `SEQ` is
    /// chosen free of any pending entry, so two in-flight queries never share a wire `SEQ`.
    pending: Arc<Mutex<HashMap<u8, PendingEntry>>>,
    /// Consumer half of the LOG fan-out, cloned out by [`Device::logs`]; the producer half lives only
    /// in the reader thread.
    logs_rx: flume::Receiver<LogLine>,
    /// Intended button overrides; the keepalive and reconnect-reapply act on this.
    desired: Arc<Mutex<DesiredState>>,
    counters: Arc<Counters>,
    /// Set on drop/disconnect; the reader and keepalive observe it and exit.
    stop: Arc<AtomicBool>,
    /// Default `RESP` wait for [`query`](Device::query) — the fixed [`DEFAULT_QUERY_TIMEOUT`].
    query_timeout: std::time::Duration,
    /// Thread handles, joined on drop.
    reader: Option<JoinHandle<()>>,
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

impl Inner {
    /// Remove the pending entry under `seq` only if its generation matches `gen_id`, so a stale
    /// canceller never evicts a newer query that reused the same wire `SEQ`. Lives on `Inner` so the
    /// async timer can call it through a `Weak<Inner>` without pinning the device alive.
    pub(crate) fn cancel_query(&self, seq: u8, gen_id: u64) {
        let mut pending = self.pending.lock();
        if pending.get(&seq).is_some_and(|e| e.gen_id == gen_id) {
            pending.remove(&seq);
        }
    }
}

impl Drop for Inner {
    fn drop(&mut self) {
        // Signal then join; the reader's read timeout (≈100 ms real / a few ms mock) bounds how long it
        // takes to notice, so this never hangs.
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
/// `Device` is `&self`-only, `Send + Sync`, and a cheap `Arc<Inner>` to [`Clone`]. Cloning yields
/// another handle to the *same* box and threads; the threads stop and join on the last drop. Construct
/// it with [`Device::open`] (a path), [`Device::find`] (VID/PID scan), or the crate-internal
/// `from_transport` (no handshake).
#[derive(Clone, Debug)]
pub struct Device {
    inner: Arc<Inner>,
}

impl Device {
    /// Wrap an already-open transport, spawn the reader and keepalive threads, and return the device —
    /// **without** a handshake. The no-handshake seam used by [`Device::open`]'s handshaking path, the
    /// device tests, and the `mock` feature. Uses the fixed [`DEFAULT_KEEPALIVE_CADENCE`].
    pub(crate) fn from_transport(transport: Arc<dyn Transport>) -> Device {
        Self::from_transport_with_cadence(transport, DEFAULT_KEEPALIVE_CADENCE)
    }

    /// As [`from_transport`](Device::from_transport) but with an explicit keepalive cadence, so tests
    /// can observe keepalive behaviour without real 500 ms waits. The single construction seam.
    pub(crate) fn from_transport_with_cadence(
        transport: Arc<dyn Transport>,
        keepalive_cadence: std::time::Duration,
    ) -> Device {
        let query_timeout = DEFAULT_QUERY_TIMEOUT;
        // Build each shared piece as its own Arc BEFORE spawning, so threads capture only what they
        // need — never `Arc<Inner>` (a cycle that would block Drop's join).
        let pending: Arc<Mutex<HashMap<u8, PendingEntry>>> = Arc::new(Mutex::new(HashMap::new()));
        let (logs_tx, logs_rx) = flume::bounded(logs::LOGS_CAPACITY);
        let counters = Arc::new(Counters::default());
        let stop = Arc::new(AtomicBool::new(false));
        let desired = Arc::new(Mutex::new(DesiredState::default()));
        let write_lock = Arc::new(Mutex::new(()));
        let seq = Arc::new(AtomicU8::new(0));
        let query_gen = Arc::new(AtomicU64::new(0));
        let transport = Arc::new(TransportSlot::new(transport));

        let reader = spawn_reader(
            Arc::clone(&transport),
            Arc::clone(&pending),
            logs_tx.clone(),
            logs_rx.clone(),
            Arc::clone(&counters),
            Arc::clone(&stop),
        );

        // Keepalive shares the write state and `desired`, never `Arc<Inner>` (anti-cycle, like the reader).
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
                query_gen,
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

    /// The configured default query timeout.
    pub(crate) fn query_timeout_default(&self) -> std::time::Duration {
        self.inner.query_timeout
    }

    /// Allocate the next rolling `SEQ` (wraps 255 → 0).
    pub(crate) fn next_seq(&self) -> u8 {
        self.inner.seq.fetch_add(1, Ordering::Relaxed)
    }

    /// Encode and fire one frame with an explicit `SEQ`, returning once the bytes are flushed.
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

    /// Register a fresh query waiter and return `(seq, gen, rx)`: take a unique generation, pick a wire
    /// `SEQ` not currently pending, and insert the generation-tagged one-shot under it.
    ///
    /// Picking a free `SEQ` guarantees no two in-flight queries share one, so a `RESP` can never be
    /// cross-delivered (the two would be indistinguishable on the wire). The 256-draw sweep always finds
    /// a free slot unless all 256 are in flight (unreachable — the box answers in microseconds), in
    /// which case the last draw is reused.
    pub(crate) fn register_pending(
        &self,
        expected_what: u8,
    ) -> (u8, u64, flume::Receiver<Vec<u8>>) {
        let gen_id = self.inner.query_gen.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = flume::bounded::<Vec<u8>>(1);
        let mut pending = self.inner.pending.lock();
        let mut seq = self.next_seq();
        for _ in 0..256 {
            if !pending.contains_key(&seq) {
                break;
            }
            seq = self.next_seq();
        }
        pending.insert(
            seq,
            PendingEntry {
                gen_id,
                expected_what,
                tx,
            },
        );
        (seq, gen_id, rx)
    }

    /// Delegates to [`Inner::cancel_query`].
    pub(crate) fn cancel_query(&self, seq: u8, gen_id: u64) {
        self.inner.cancel_query(seq, gen_id);
    }

    /// The number of in-flight query waiters (a diagnostic seam the FIX-1 correlation tests assert on).
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn pending_len(&self) -> usize {
        self.inner.pending.lock().len()
    }

    /// A `Weak` handle to the interior, so the async query timer can cancel a pending entry without
    /// pinning `Inner` alive (a held `Arc<Inner>` would defer shutdown).
    #[cfg(feature = "async")]
    pub(crate) fn weak_inner(&self) -> std::sync::Weak<Inner> {
        Arc::downgrade(&self.inner)
    }

    /// The intended-state map, shared by the command surface and the keepalive/reconnect reconcile.
    pub(crate) fn desired(&self) -> &Mutex<DesiredState> {
        &self.inner.desired
    }

    /// The swappable transport slot (for [`reconnect`](Device::reconnect)).
    pub(crate) fn transport_slot(&self) -> &Arc<TransportSlot> {
        &self.inner.transport
    }

    pub(crate) fn counters_inner(&self) -> &Counters {
        &self.inner.counters
    }
}

/// Encode and fire one frame, serialized by `write_lock` (held **only** around `write_all`). Goes
/// through the swappable slot so a reconnect redirects it; bumps `frames_tx` on success.
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
    // Per-frame TX at TRACE only (timing-perturbing; kept off any tight caller timing loop).
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

/// Spawn the reader thread. It clones only the shared state it touches, never `Inner`, so it cannot
/// keep `Inner` alive against its own `Drop`.
#[allow(clippy::too_many_arguments)]
fn spawn_reader(
    transport: Arc<TransportSlot>,
    pending: Arc<Mutex<HashMap<u8, PendingEntry>>>,
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

/// Back-off after a read error so the reader doesn't busy-spin on a dead port while a
/// [`reconnect`](Device::reconnect) swap installs a fresh transport.
const READER_ERROR_BACKOFF: std::time::Duration = std::time::Duration::from_millis(20);

/// The reader loop. It loads the current transport each iteration, so after a reconnect swap the same
/// thread follows onto the new transport with no restart. Exits **only** on `stop` (a read error backs
/// off and retries, since the port may be about to be reconnected).
#[allow(clippy::too_many_arguments)]
fn reader_loop(
    transport: &TransportSlot,
    pending: &Mutex<HashMap<u8, PendingEntry>>,
    logs_tx: &flume::Sender<LogLine>,
    logs_rx: &flume::Receiver<LogLine>,
    counters: &Counters,
    stop: &AtomicBool,
) {
    let mut decoder = FrameDecoder::new();
    let mut buf = [0u8; 1024];
    let mut seen_generation = transport.generation();

    loop {
        if stop.load(Ordering::SeqCst) {
            return;
        }
        // On a reconnect swap (generation bumped), reset the decoder so a frame interrupted mid-parse on
        // the old port can't mis-frame the first bytes of the new one.
        let generation = transport.generation();
        if generation != seen_generation {
            decoder = FrameDecoder::new();
            seen_generation = generation;
        }
        let current = transport.current();
        match current.read(&mut buf) {
            Ok(0) => {
                // Read timeout / empty mock queue: re-check `stop`. The tiny sleep avoids busy-spinning
                // a mock that returns instantly; real serial reads already block ≈100 ms.
                std::thread::sleep(READER_IDLE_POLL);
            }
            Ok(n) => {
                decoder.feed(&buf[..n], |frame| {
                    route_frame(frame, pending, logs_tx, logs_rx, counters);
                });
                counters.set_crc_drops(decoder.crc_error_count());
            }
            Err(_) => {
                // Port gone or hiccuping: back off and retry rather than exit, since `reconnect` may be
                // about to swap in a fresh transport. Drop's `stop` still ends the loop promptly.
                std::thread::sleep(READER_ERROR_BACKOFF);
            }
        }
    }
}

/// Route one decoded frame by `TYPE` (§5: RESP→pending, LOG→fan-out, others ignored).
fn route_frame(
    frame: crate::protocol::DecodedFrame,
    pending: &Mutex<HashMap<u8, PendingEntry>>,
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
            // Correlate by SEQ *and* selector: only fulfil a waiter whose expected `what` matches
            // `RESP[0]`. A SEQ with no waiter (timed out / duplicate) is dropped; a SEQ-matched but
            // selector-mismatched RESP — e.g. the unsolicited VERSION hello landing on a HEALTH
            // query that reused SEQ=0 — is also dropped, leaving the real reply to fulfil the waiter.
            let mut pending = pending.lock();
            let deliver = pending
                .get(&frame.seq)
                .is_some_and(|e| frame.payload.first() == Some(&e.expected_what));
            if deliver && let Some(entry) = pending.remove(&frame.seq) {
                let _ = entry.tx.send(frame.payload);
            }
        }
        FrameType::Log => {
            let line = parse_log(&frame.payload);
            // Mirror the LOG as a host tracing event, in addition to the logs() channel below.
            #[cfg(feature = "tracing")]
            crate::trace::emit_device_log(&line);
            logs::push(logs_tx, logs_rx, line);
        }
        // MOVE/WHEEL/BUTTON/RESET/QUERY/REBOOT_DL are PC→box; an echo or any other type is ignored
        // (forward-compat, §2).
        _ => {}
    }
}
