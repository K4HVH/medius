pub(crate) mod catch;
pub(crate) mod correlation;
pub(crate) mod counters;
pub(crate) mod keepalive;
pub(crate) mod logs;
pub(crate) mod reader;
pub(crate) mod reconcile;
pub(crate) mod reconnect;
pub(crate) mod slot;

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

use parking_lot::Mutex;

use crate::error::Result;
use crate::protocol::{FrameType, encode};
use crate::transport::Transport;
use crate::types::{CountersSnapshot, LogLine};

use catch::CatchReg;
use correlation::PendingEntry;
use counters::Counters;
use reconcile::DesiredState;
use reconnect::BoxIdentity;
use slot::TransportSlot;

/// Default `RESP` wait before [`Error::QueryTimeout`](crate::Error::QueryTimeout).
pub const DEFAULT_QUERY_TIMEOUT: Duration = Duration::from_secs(1);

/// Default keepalive cadence for refreshing a held override.
pub const DEFAULT_KEEPALIVE_CADENCE: Duration = Duration::from_millis(500);

pub(crate) struct LinkInner {
    transport: Arc<TransportSlot>,
    write_lock: Arc<Mutex<()>>,
    seq: Arc<AtomicU8>,
    query_gen: Arc<AtomicU64>,
    pending: Arc<Mutex<HashMap<u8, PendingEntry>>>,
    logs_rx: flume::Receiver<LogLine>,
    desired: Arc<Mutex<DesiredState>>,
    events: Arc<Mutex<CatchReg>>,
    catch_gen: Arc<AtomicU64>,
    // Serializes a whole subscribe/unsubscribe sequence (registry mutate -> union recompute ->
    // desired update -> CATCH send) so concurrent callers can't commit their masks out of order and
    // leave the box streaming a mask that disagrees with the registry. Not taken on the reader path.
    catch_lock: Arc<Mutex<()>>,
    counters: Arc<Counters>,
    stop: Arc<AtomicBool>,
    reconnect_lock: Arc<Mutex<()>>,
    // The opened box's stable identity (CH343 serial + device MAC), set once the handshake succeeds.
    // Reconnect anchors to it so a rescan never adopts a different box that happens to be present.
    identity: Arc<Mutex<Option<BoxIdentity>>>,
    query_timeout: Duration,
    reader: Option<JoinHandle<()>>,
    keepalive: Option<JoinHandle<()>>,
}

impl std::fmt::Debug for LinkInner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LinkInner")
            .field("transport", &self.transport)
            .field("seq", &self.seq.load(Ordering::Relaxed))
            .field("pending", &self.pending.lock().len())
            .field("counters", &self.counters.snapshot())
            .field("stopped", &self.stop.load(Ordering::Relaxed))
            .finish_non_exhaustive()
    }
}

impl Drop for LinkInner {
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

#[derive(Clone, Debug)]
pub(crate) struct Link {
    inner: Arc<LinkInner>,
}

impl Link {
    pub(crate) fn from_transport(transport: Arc<dyn Transport>) -> Link {
        Self::from_transport_with_cadence(transport, DEFAULT_KEEPALIVE_CADENCE)
    }

    pub(crate) fn from_transport_with_cadence(
        transport: Arc<dyn Transport>,
        keepalive_cadence: Duration,
    ) -> Link {
        let query_timeout = DEFAULT_QUERY_TIMEOUT;
        let pending: Arc<Mutex<HashMap<u8, PendingEntry>>> = Arc::new(Mutex::new(HashMap::new()));
        let (logs_tx, logs_rx) = flume::bounded(logs::LOGS_CAPACITY);
        let counters = Arc::new(Counters::default());
        let stop = Arc::new(AtomicBool::new(false));
        let desired = Arc::new(Mutex::new(DesiredState::default()));
        let events = Arc::new(Mutex::new(CatchReg::default()));
        let catch_gen = Arc::new(AtomicU64::new(0));
        let catch_lock = Arc::new(Mutex::new(()));
        let write_lock = Arc::new(Mutex::new(()));
        let seq = Arc::new(AtomicU8::new(0));
        let query_gen = Arc::new(AtomicU64::new(0));
        let transport = Arc::new(TransportSlot::new(transport));
        let reconnect_lock = Arc::new(Mutex::new(()));
        let identity: Arc<Mutex<Option<BoxIdentity>>> = Arc::new(Mutex::new(None));

        let reader = reader::spawn_reader(
            Arc::clone(&transport),
            Arc::clone(&pending),
            logs_tx.clone(),
            logs_rx.clone(),
            Arc::clone(&events),
            Arc::clone(&counters),
            Arc::clone(&stop),
            reconnect::ReconnectCtx {
                transport: Arc::clone(&transport),
                write_lock: Arc::clone(&write_lock),
                seq: Arc::clone(&seq),
                counters: Arc::clone(&counters),
                desired: Arc::clone(&desired),
                reconnect_lock: Arc::clone(&reconnect_lock),
                identity: Arc::clone(&identity),
            },
        );

        let keepalive = keepalive::spawn_keepalive(keepalive::KeepaliveCtx {
            transport: Arc::clone(&transport),
            write_lock: Arc::clone(&write_lock),
            seq: Arc::clone(&seq),
            counters: Arc::clone(&counters),
            desired: Arc::clone(&desired),
            stop: Arc::clone(&stop),
            cadence: keepalive_cadence,
        });

        Link {
            inner: Arc::new(LinkInner {
                transport,
                write_lock,
                seq,
                query_gen,
                pending,
                logs_rx,
                desired,
                events,
                catch_gen,
                catch_lock,
                counters,
                stop,
                reconnect_lock,
                identity,
                query_timeout,
                reader: Some(reader),
                keepalive: Some(keepalive),
            }),
        }
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

    pub(crate) fn counters(&self) -> CountersSnapshot {
        self.inner.counters.snapshot()
    }

    pub(crate) fn query_timeout_default(&self) -> Duration {
        self.inner.query_timeout
    }

    pub(crate) fn desired(&self) -> &Mutex<DesiredState> {
        &self.inner.desired
    }

    pub(crate) fn logs_rx(&self) -> flume::Receiver<LogLine> {
        self.inner.logs_rx.clone()
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn transport_slot(&self) -> &Arc<TransportSlot> {
        &self.inner.transport
    }

    #[cfg(feature = "async")]
    pub(crate) fn weak(&self) -> std::sync::Weak<LinkInner> {
        Arc::downgrade(&self.inner)
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
