use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::time::Duration;

use parking_lot::Mutex;

use crate::error::{Error, Result};
use crate::protocol::FrameType;
use crate::protocol::command::query_payload;
use crate::protocol::opcode::Q_OPTIONS;

use super::{Link, LinkInner};

pub(crate) struct PendingEntry {
    gen_id: u64,
    expected_what: u8,
    tx: flume::Sender<Vec<u8>>,
}

pub(crate) fn deliver(pending: &Mutex<HashMap<u8, PendingEntry>>, seq: u8, payload: Vec<u8>) {
    let mut pending = pending.lock();
    let matches = pending
        .get(&seq)
        .is_some_and(|e| payload.first() == Some(&e.expected_what));
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

    pub(crate) fn register_query(&self, what: u8) -> Result<(u8, u64, flume::Receiver<Vec<u8>>)> {
        self.register_query_with(what, &query_payload(what))
    }

    // Register a pending reply keyed on `expected_what` and send a custom QUERY request body. The option
    // query needs this: its request is `[Q_OPTIONS][id]`, while the reply still leads with the Q_OPTIONS
    // selector (so correlation matches on that, and the SEQ disambiguates concurrent option reads).
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

    pub(crate) fn query(&self, what: u8) -> Result<Vec<u8>> {
        self.query_timeout(what, self.query_timeout_default())
    }

    pub(crate) fn query_timeout(&self, what: u8, timeout: Duration) -> Result<Vec<u8>> {
        let (seq, gen_id, rx) = self.register_query(what)?;
        self.recv_query(seq, gen_id, &rx, what, timeout)
    }

    /// `QUERY(OPTIONS, id)` — read one persistent box option, correlated on the `Q_OPTIONS` selector.
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
