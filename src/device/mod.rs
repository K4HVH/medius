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

const READER_IDLE_POLL: std::time::Duration = std::time::Duration::from_millis(2);

/// Default `RESP` wait before [`Error::QueryTimeout`](crate::Error::QueryTimeout).
pub const DEFAULT_QUERY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(1);

/// Default keepalive cadence for refreshing a held override.
pub const DEFAULT_KEEPALIVE_CADENCE: std::time::Duration = std::time::Duration::from_millis(500);

struct PendingEntry {
    gen_id: u64,
    expected_what: u8,
    tx: flume::Sender<Vec<u8>>,
}

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

    pub(crate) fn current(&self) -> Arc<dyn Transport> {
        Arc::clone(&self.current.lock())
    }

    pub(crate) fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    pub(crate) fn swap(&self, transport: Arc<dyn Transport>) {
        *self.current.lock() = transport;
        self.generation.fetch_add(1, Ordering::Release);
    }
}

pub(crate) struct Inner {
    transport: Arc<TransportSlot>,
    write_lock: Arc<Mutex<()>>,
    seq: Arc<AtomicU8>,
    query_gen: Arc<AtomicU64>,
    pending: Arc<Mutex<HashMap<u8, PendingEntry>>>,
    logs_rx: flume::Receiver<LogLine>,
    desired: Arc<Mutex<DesiredState>>,
    counters: Arc<Counters>,
    stop: Arc<AtomicBool>,
    reconnect_lock: Arc<Mutex<()>>,
    query_timeout: std::time::Duration,
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
    pub(crate) fn cancel_query(&self, seq: u8, gen_id: u64) {
        let mut pending = self.pending.lock();
        if pending.get(&seq).is_some_and(|e| e.gen_id == gen_id) {
            pending.remove(&seq);
        }
    }
}

impl Drop for Inner {
    fn drop(&mut self) {
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
#[derive(Clone, Debug)]
pub struct Device {
    inner: Arc<Inner>,
}

impl Device {
    pub(crate) fn from_transport(transport: Arc<dyn Transport>) -> Device {
        Self::from_transport_with_cadence(transport, DEFAULT_KEEPALIVE_CADENCE)
    }

    pub(crate) fn from_transport_with_cadence(
        transport: Arc<dyn Transport>,
        keepalive_cadence: std::time::Duration,
    ) -> Device {
        let query_timeout = DEFAULT_QUERY_TIMEOUT;
        let pending: Arc<Mutex<HashMap<u8, PendingEntry>>> = Arc::new(Mutex::new(HashMap::new()));
        let (logs_tx, logs_rx) = flume::bounded(logs::LOGS_CAPACITY);
        let counters = Arc::new(Counters::default());
        let stop = Arc::new(AtomicBool::new(false));
        let desired = Arc::new(Mutex::new(DesiredState::default()));
        let write_lock = Arc::new(Mutex::new(()));
        let seq = Arc::new(AtomicU8::new(0));
        let query_gen = Arc::new(AtomicU64::new(0));
        let transport = Arc::new(TransportSlot::new(transport));
        let reconnect_lock = Arc::new(Mutex::new(()));

        let reader = spawn_reader(
            Arc::clone(&transport),
            Arc::clone(&pending),
            logs_tx.clone(),
            logs_rx.clone(),
            Arc::clone(&counters),
            Arc::clone(&stop),
            reboot::ReconnectCtx {
                transport: Arc::clone(&transport),
                write_lock: Arc::clone(&write_lock),
                seq: Arc::clone(&seq),
                counters: Arc::clone(&counters),
                desired: Arc::clone(&desired),
                reconnect_lock: Arc::clone(&reconnect_lock),
            },
        );

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
                reconnect_lock,
                query_timeout,
                reader: Some(reader),
                keepalive: Some(keepalive),
            }),
        }
    }

    pub(crate) fn query_timeout_default(&self) -> std::time::Duration {
        self.inner.query_timeout
    }

    pub(crate) fn next_seq(&self) -> u8 {
        self.inner.seq.fetch_add(1, Ordering::Relaxed)
    }

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

    pub(crate) fn send(&self, ty: FrameType, payload: &[u8]) -> Result<()> {
        let seq = self.next_seq();
        self.send_with_seq(seq, ty, payload)
    }

    /// A snapshot of the always-on counters.
    pub fn counters(&self) -> CountersSnapshot {
        self.inner.counters.snapshot()
    }

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

    pub(crate) fn cancel_query(&self, seq: u8, gen_id: u64) {
        self.inner.cancel_query(seq, gen_id);
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn pending_len(&self) -> usize {
        self.inner.pending.lock().len()
    }

    #[cfg(feature = "async")]
    pub(crate) fn weak_inner(&self) -> std::sync::Weak<Inner> {
        Arc::downgrade(&self.inner)
    }

    pub(crate) fn desired(&self) -> &Mutex<DesiredState> {
        &self.inner.desired
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn transport_slot(&self) -> &Arc<TransportSlot> {
        &self.inner.transport
    }
}

fn write_frame(
    transport: &TransportSlot,
    write_lock: &Mutex<()>,
    counters: &Counters,
    seq: u8,
    ty: FrameType,
    payload: &[u8],
) -> Result<()> {
    let frame = encode(ty, seq, payload)?;
    let current = transport.current();
    {
        let _guard = write_lock.lock();
        current.write_all(&frame)?;
    }
    counters.inc_tx();
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

#[allow(clippy::too_many_arguments)]
fn spawn_reader(
    transport: Arc<TransportSlot>,
    pending: Arc<Mutex<HashMap<u8, PendingEntry>>>,
    logs_tx: flume::Sender<LogLine>,
    logs_rx: flume::Receiver<LogLine>,
    counters: Arc<Counters>,
    stop: Arc<AtomicBool>,
    reconnect_ctx: reboot::ReconnectCtx,
) -> JoinHandle<()> {
    std::thread::Builder::new()
        .name("medius-reader".into())
        .spawn(move || {
            reader_loop(
                &transport,
                &pending,
                &logs_tx,
                &logs_rx,
                &counters,
                &stop,
                &reconnect_ctx,
            )
        })
        .expect("spawn medius-reader thread")
}

#[allow(clippy::too_many_arguments)]
fn reader_loop(
    transport: &TransportSlot,
    pending: &Mutex<HashMap<u8, PendingEntry>>,
    logs_tx: &flume::Sender<LogLine>,
    logs_rx: &flume::Receiver<LogLine>,
    counters: &Counters,
    stop: &AtomicBool,
    reconnect_ctx: &reboot::ReconnectCtx,
) {
    let mut decoder = FrameDecoder::new();
    let mut buf = [0u8; 1024];
    let mut seen_generation = transport.generation();

    loop {
        if stop.load(Ordering::SeqCst) {
            return;
        }
        let generation = transport.generation();
        if generation != seen_generation {
            decoder = FrameDecoder::new();
            seen_generation = generation;
        }
        let current = transport.current();
        match current.read(&mut buf) {
            Ok(0) => {
                std::thread::sleep(READER_IDLE_POLL);
            }
            Ok(n) => {
                decoder.feed(&buf[..n], |frame| {
                    route_frame(frame, pending, logs_tx, logs_rx, counters);
                });
                counters.set_crc_drops(decoder.crc_error_count());
            }
            Err(_) => {
                drop(current);
                reboot::auto_reconnect(reconnect_ctx, stop);
            }
        }
    }
}

fn route_frame(
    frame: crate::protocol::DecodedFrame,
    pending: &Mutex<HashMap<u8, PendingEntry>>,
    logs_tx: &flume::Sender<LogLine>,
    logs_rx: &flume::Receiver<LogLine>,
    counters: &Counters,
) {
    counters.inc_rx();
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
            #[cfg(feature = "tracing")]
            crate::trace::emit_device_log(&line);
            logs::push(logs_tx, logs_rx, line);
        }
        _ => {}
    }
}
