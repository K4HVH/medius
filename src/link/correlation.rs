use std::collections::HashMap;
use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};
use std::time::Duration;

use parking_lot::Mutex;

use crate::error::{Error, Result};
use crate::protocol::FrameType;
use crate::protocol::command::{query_payload, transfer_payload};
use crate::protocol::opcode::{Q_OPTIONS, XFER_LAST, XFER_RESP_HDR};
use crate::types::{Setup, TransferOutcome, TransferStage, TransferStatus};

use super::{Link, LinkInner};

pub(crate) struct PendingEntry {
    gen_id: u64,
    waiter: Waiter,
}

// The first byte must match too: a stale reply on a reused `SEQ` is never taken as the awaited one.
enum Waiter {
    Resp {
        what: u8,
        tx: flume::Sender<Vec<u8>>,
    },
    Transfer {
        ep: u8,
        reply: TransferReply,
        tx: flume::Sender<TransferReply>,
    },
}

#[derive(Default)]
pub(crate) struct TransferReply {
    status: u8,
    stage: u8,
    data: Vec<u8>,
    got: usize,
}

impl TransferReply {
    fn take(&mut self, chunk: &[u8]) -> bool {
        let Some((&[_, status, stage, flags, lo, hi], data)) =
            chunk.split_first_chunk::<XFER_RESP_HDR>()
        else {
            return false;
        };
        let off = u16::from_le_bytes([lo, hi]) as usize;
        let end = off + data.len();
        if self.data.len() < end {
            self.data.resize(end, 0);
        }
        self.data[off..end].copy_from_slice(data);
        self.got += data.len();
        if flags & XFER_LAST == 0 {
            return false;
        }
        self.status = status;
        self.stage = stage;
        true
    }

    pub(crate) fn outcome(self) -> Result<TransferOutcome> {
        if self.got != self.data.len() {
            return Err(Error::TransferReplyIncomplete {
                got: self.got,
                len: self.data.len(),
            });
        }
        Ok(TransferOutcome {
            status: TransferStatus::from_u8(self.status),
            stage: TransferStage::from_u8(self.stage),
            data: self.data,
        })
    }
}

/// `SEQ` of the box's unsolicited `RESP(VERSION)` hello; no awaited reply uses it.
pub(crate) const HELLO_SEQ: u8 = 0;

const TRANSFER_REPLY_MARGIN: Duration = Duration::from_secs(2);

// Takes a free `SEQ` for a reply to wait on, never the hello's.
fn insert(
    pending: &Mutex<HashMap<u8, PendingEntry>>,
    query_gen: &AtomicU64,
    seq_counter: &AtomicU8,
    waiter: Waiter,
) -> (u8, u64) {
    let gen_id = query_gen.fetch_add(1, Ordering::Relaxed);
    let mut pending = pending.lock();
    let next = || seq_counter.fetch_add(1, Ordering::Relaxed);
    let mut seq = next();
    for _ in 0..256 {
        if seq != HELLO_SEQ && !pending.contains_key(&seq) {
            break;
        }
        seq = next();
    }
    if seq == HELLO_SEQ {
        seq = next();
    }
    pending.insert(seq, PendingEntry { gen_id, waiter });
    (seq, gen_id)
}

pub(crate) fn register(
    pending: &Mutex<HashMap<u8, PendingEntry>>,
    query_gen: &AtomicU64,
    seq_counter: &AtomicU8,
    what: u8,
) -> (u8, u64, flume::Receiver<Vec<u8>>) {
    let (tx, rx) = flume::bounded(1);
    let (seq, gen_id) = insert(pending, query_gen, seq_counter, Waiter::Resp { what, tx });
    (seq, gen_id, rx)
}

pub(crate) fn cancel(pending: &Mutex<HashMap<u8, PendingEntry>>, seq: u8, gen_id: u64) {
    let mut pending = pending.lock();
    if pending.get(&seq).is_some_and(|e| e.gen_id == gen_id) {
        pending.remove(&seq);
    }
}

pub(crate) fn deliver(
    pending: &Mutex<HashMap<u8, PendingEntry>>,
    ty: FrameType,
    seq: u8,
    payload: Vec<u8>,
) {
    let mut pending = pending.lock();
    let Some(entry) = pending.get_mut(&seq) else {
        return;
    };
    let first = payload.first().copied();
    let done = match &mut entry.waiter {
        Waiter::Resp { what, .. } => ty == FrameType::Resp && first == Some(*what),
        Waiter::Transfer { ep, reply, .. } => {
            ty == FrameType::TransferResp && first == Some(*ep) && reply.take(&payload)
        }
    };
    if !done {
        return;
    }
    match pending.remove(&seq).map(|e| e.waiter) {
        Some(Waiter::Resp { tx, .. }) => {
            let _ = tx.send(payload);
        }
        Some(Waiter::Transfer { reply, tx, .. }) => {
            let _ = tx.send(reply);
        }
        None => {}
    }
}

impl LinkInner {
    pub(crate) fn cancel_query(&self, seq: u8, gen_id: u64) {
        cancel(&self.pending, seq, gen_id);
    }
}

impl Link {
    pub(crate) fn register_pending(&self, what: u8) -> (u8, u64, flume::Receiver<Vec<u8>>) {
        register(
            &self.inner.pending,
            &self.inner.query_gen,
            &self.inner.seq,
            what,
        )
    }

    fn register_transfer(&self, ep: u8) -> (u8, u64, flume::Receiver<TransferReply>) {
        let (tx, rx) = flume::bounded(1);
        let waiter = Waiter::Transfer {
            ep,
            reply: TransferReply::default(),
            tx,
        };
        let (seq, gen_id) = insert(
            &self.inner.pending,
            &self.inner.query_gen,
            &self.inner.seq,
            waiter,
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

    pub(crate) fn register_query(&self, what: u8) -> Result<(u8, u64, flume::Receiver<Vec<u8>>)> {
        self.register_query_with(what, &query_payload(what))
    }

    // An option reply leads with the Q_OPTIONS selector, not the id, so correlation matches on
    // `expected_what` and `SEQ` separates concurrent reads.
    pub(crate) fn register_query_with(
        &self,
        expected_what: u8,
        request: &[u8],
    ) -> Result<(u8, u64, flume::Receiver<Vec<u8>>)> {
        let (seq, gen_id, rx) = self.register_pending(expected_what);
        if let Err(e) = self.send_with_seq(seq, FrameType::Query, request) {
            self.cancel_query(seq, gen_id);
            return Err(e);
        }
        Ok((seq, gen_id, rx))
    }

    /// `QUERY [what][index]`: one indexed entry (a rewrite rule or descriptor patch), correlated on
    /// the reply's leading `what` selector, like [`query_option`](Self::query_option).
    pub(crate) fn query_indexed(&self, what: u8, index: u8) -> Result<Vec<u8>> {
        let timeout = self.query_timeout_default();
        let (seq, gen_id, rx) = self.register_query_with(what, &[what, index])?;
        self.recv_query(seq, gen_id, &rx, what, timeout)
    }

    /// One `TRANSFER` the box waits `timeout_ms` on, awaiting the last chunk of its reply.
    pub(crate) fn transfer(
        &self,
        ep: u8,
        timeout_ms: u16,
        setup: Setup,
        out: &[u8],
    ) -> Result<TransferOutcome> {
        let (seq, gen_id, rx) = self.send_transfer(ep, timeout_ms, setup, out)?;
        match rx.recv_timeout(transfer_wait(timeout_ms)) {
            Ok(reply) => {
                trace_event!(
                    target: "medius::device",
                    tracing::Level::DEBUG,
                    ep,
                    seq,
                    "transfer resolved",
                );
                reply.outcome()
            }
            Err(_) => {
                self.cancel_query(seq, gen_id);
                trace_event!(
                    target: "medius::device",
                    tracing::Level::WARN,
                    ep,
                    seq,
                    "transfer timed out",
                );
                Err(Error::QueryTimeout)
            }
        }
    }

    fn send_transfer(
        &self,
        ep: u8,
        timeout_ms: u16,
        setup: Setup,
        out: &[u8],
    ) -> Result<(u8, u64, flume::Receiver<TransferReply>)> {
        let payload = transfer_payload(ep, timeout_ms, setup, out);
        let (seq, gen_id, rx) = self.register_transfer(ep);
        if let Err(e) = self.send_with_seq(seq, FrameType::Transfer, &payload) {
            self.cancel_query(seq, gen_id);
            return Err(e);
        }
        Ok((seq, gen_id, rx))
    }

    pub(crate) fn query(&self, what: u8) -> Result<Vec<u8>> {
        self.query_timeout(what, self.query_timeout_default())
    }

    pub(crate) fn query_timeout(&self, what: u8, timeout: Duration) -> Result<Vec<u8>> {
        let (seq, gen_id, rx) = self.register_query(what)?;
        self.recv_query(seq, gen_id, &rx, what, timeout)
    }

    /// `QUERY(OPTIONS, id)`: one persistent box option, correlated on the `Q_OPTIONS` selector.
    pub(crate) fn query_option(&self, id: u8) -> Result<Vec<u8>> {
        let timeout = self.query_timeout_default();
        let (seq, gen_id, rx) = self.register_query_with(Q_OPTIONS, &[Q_OPTIONS, id])?;
        self.recv_query(seq, gen_id, &rx, Q_OPTIONS, timeout)
    }

    #[cfg_attr(not(feature = "tracing"), allow(unused_variables))] // `what` is only read by trace_event!
    fn recv_query(
        &self,
        seq: u8,
        gen_id: u64,
        rx: &flume::Receiver<Vec<u8>>,
        what: u8,
        timeout: Duration,
    ) -> Result<Vec<u8>> {
        match rx.recv_timeout(timeout) {
            Ok(payload) => {
                trace_event!(
                    target: "medius::device",
                    tracing::Level::DEBUG,
                    selector = what,
                    seq,
                    resp_len = payload.len(),
                    "query resolved",
                );
                Ok(payload)
            }
            Err(_) => {
                self.cancel_query(seq, gen_id);
                trace_event!(
                    target: "medius::device",
                    tracing::Level::WARN,
                    selector = what,
                    seq,
                    "query timed out",
                );
                Err(Error::QueryTimeout)
            }
        }
    }

    #[cfg(feature = "async")]
    pub(crate) async fn query_async(&self, what: u8, timeout: Duration) -> Result<Vec<u8>> {
        let (seq, gen_id, rx) = self.register_query(what)?;
        self.recv_query_async(seq, gen_id, rx, timeout).await
    }

    #[cfg(feature = "async")]
    pub(crate) async fn query_option_async(&self, id: u8, timeout: Duration) -> Result<Vec<u8>> {
        let (seq, gen_id, rx) = self.register_query_with(Q_OPTIONS, &[Q_OPTIONS, id])?;
        self.recv_query_async(seq, gen_id, rx, timeout).await
    }

    #[cfg(feature = "async")]
    pub(crate) async fn query_indexed_async(
        &self,
        what: u8,
        index: u8,
        timeout: Duration,
    ) -> Result<Vec<u8>> {
        let (seq, gen_id, rx) = self.register_query_with(what, &[what, index])?;
        self.recv_query_async(seq, gen_id, rx, timeout).await
    }

    #[cfg(feature = "async")]
    pub(crate) async fn transfer_async(
        &self,
        ep: u8,
        timeout_ms: u16,
        setup: Setup,
        out: &[u8],
    ) -> Result<TransferOutcome> {
        let (seq, gen_id, rx) = self.send_transfer(ep, timeout_ms, setup, out)?;
        self.recv_query_async(seq, gen_id, rx, transfer_wait(timeout_ms))
            .await?
            .outcome()
    }

    #[cfg(feature = "async")]
    async fn recv_query_async<T>(
        &self,
        seq: u8,
        gen_id: u64,
        rx: flume::Receiver<T>,
        timeout: Duration,
    ) -> Result<T> {
        let (cancel_tx, cancel_rx) = flume::bounded::<()>(1);
        let weak = self.weak();
        std::thread::Builder::new()
            .name("medius-query-timeout".into())
            .spawn(move || {
                if let Err(flume::RecvTimeoutError::Timeout) = cancel_rx.recv_timeout(timeout)
                    && let Some(inner) = weak.upgrade()
                {
                    inner.cancel_query(seq, gen_id);
                }
            })
            .expect("spawn medius-query-timeout thread");
        let res = rx.recv_async().await;
        drop(cancel_tx);
        match res {
            Ok(payload) => Ok(payload),
            Err(_) => Err(Error::QueryTimeout),
        }
    }
}

fn transfer_wait(timeout_ms: u16) -> Duration {
    Duration::from_millis(timeout_ms.into()) + TRANSFER_REPLY_MARGIN
}
