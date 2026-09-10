use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::time::Duration;

use parking_lot::Mutex;

use crate::error::{Error, Result};
use crate::protocol::FrameType;
use crate::protocol::command::{query_payload, transfer_payload};
use crate::protocol::opcode::Q_OPTIONS;
use crate::types::Setup;

use super::{Link, LinkInner};

pub(crate) struct PendingEntry {
    gen_id: u64,
    // Both the frame type and its first byte have to match, so a stale `RESP` reusing a `SEQ` a
    // `TRANSFER` now waits on cannot be delivered as that transfer's answer: the two frames differ in
    // type (`Resp` vs `TransferResp`) even when their first byte (a selector vs an endpoint) collides.
    expected_ty: FrameType,
    expected_what: u8,
    tx: flume::Sender<Vec<u8>>,
}

pub(crate) fn deliver(
    pending: &Mutex<HashMap<u8, PendingEntry>>,
    ty: FrameType,
    seq: u8,
    payload: Vec<u8>,
) {
    let mut pending = pending.lock();
    let matches = pending
        .get(&seq)
        .is_some_and(|e| e.expected_ty == ty && payload.first() == Some(&e.expected_what));
    if matches && let Some(entry) = pending.remove(&seq) {
        let _ = entry.tx.send(payload);
    }
}

impl LinkInner {
    pub(crate) fn cancel_query(&self, seq: u8, gen_id: u64) {
        let mut pending = self.pending.lock();
        if pending.get(&seq).is_some_and(|e| e.gen_id == gen_id) {
            pending.remove(&seq);
        }
    }
}

impl Link {
    pub(crate) fn register_pending(
        &self,
        expected_ty: FrameType,
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
                expected_ty,
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

    pub(crate) fn register_query(&self, what: u8) -> Result<(u8, u64, flume::Receiver<Vec<u8>>)> {
        self.register_query_with(what, &query_payload(what))
    }

    // The option query's request is `[Q_OPTIONS][id]` but its reply still leads with the Q_OPTIONS
    // selector, so correlation matches on `expected_what` while SEQ disambiguates concurrent reads.
    pub(crate) fn register_query_with(
        &self,
        expected_what: u8,
        request: &[u8],
    ) -> Result<(u8, u64, flume::Receiver<Vec<u8>>)> {
        let (seq, gen_id, rx) = self.register_pending(FrameType::Resp, expected_what);
        if let Err(e) = self.send_with_seq(seq, FrameType::Query, request) {
            self.cancel_query(seq, gen_id);
            return Err(e);
        }
        Ok((seq, gen_id, rx))
    }

    /// `QUERY [what][index]`: read one indexed entry (a rewrite rule or descriptor patch), correlated
    /// on the `what` selector the reply leads with, exactly like [`query_option`](Self::query_option).
    pub(crate) fn query_indexed(&self, what: u8, index: u8) -> Result<Vec<u8>> {
        let timeout = self.query_timeout_default();
        let (seq, gen_id, rx) = self.register_query_with(what, &[what, index])?;
        self.recv_query(seq, gen_id, &rx, what, timeout)
    }

    /// Run one `TRANSFER` and wait for its `TRANSFER_RESP`, correlated by `SEQ` on the answer's own
    /// opcode. Returns `(status, in_data)`; the surrounding `Ok` means the box answered at all.
    pub(crate) fn transfer(
        &self,
        ep: u8,
        setup: Setup,
        out: &[u8],
        timeout: Duration,
    ) -> Result<(u8, Vec<u8>)> {
        let (seq, gen_id, rx) = self.register_pending(FrameType::TransferResp, ep);
        if let Err(e) =
            self.send_with_seq(seq, FrameType::Transfer, &transfer_payload(ep, setup, out))
        {
            self.cancel_query(seq, gen_id);
            return Err(e);
        }
        let resp = self.recv_query(seq, gen_id, &rx, ep, timeout)?;
        Ok(split_transfer_resp(&resp))
    }

    pub(crate) fn query(&self, what: u8) -> Result<Vec<u8>> {
        self.query_timeout(what, self.query_timeout_default())
    }

    pub(crate) fn query_timeout(&self, what: u8, timeout: Duration) -> Result<Vec<u8>> {
        let (seq, gen_id, rx) = self.register_query(what)?;
        self.recv_query(seq, gen_id, &rx, what, timeout)
    }

    /// `QUERY(OPTIONS, id)`: read one persistent box option, correlated on the `Q_OPTIONS` selector.
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
        setup: Setup,
        out: &[u8],
        timeout: Duration,
    ) -> Result<(u8, Vec<u8>)> {
        let (seq, gen_id, rx) = self.register_pending(FrameType::TransferResp, ep);
        if let Err(e) =
            self.send_with_seq(seq, FrameType::Transfer, &transfer_payload(ep, setup, out))
        {
            self.cancel_query(seq, gen_id);
            return Err(e);
        }
        let resp = self.recv_query_async(seq, gen_id, rx, timeout).await?;
        Ok(split_transfer_resp(&resp))
    }

    #[cfg(feature = "async")]
    async fn recv_query_async(
        &self,
        seq: u8,
        gen_id: u64,
        rx: flume::Receiver<Vec<u8>>,
        timeout: Duration,
    ) -> Result<Vec<u8>> {
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

// Split a `TRANSFER_RESP` payload `[ep][status][in-data…]` into `(status, in-data)`. A reply too
// short to carry a status reads as `Refused` (0xFC), the same byte the box sends when it declines.
fn split_transfer_resp(payload: &[u8]) -> (u8, Vec<u8>) {
    let status = payload.get(1).copied().unwrap_or(0xFC);
    let data = payload.get(2..).unwrap_or(&[]).to_vec();
    (status, data)
}
