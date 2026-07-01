use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::time::{Duration, Instant};

use parking_lot::Mutex;

use crate::error::{Error, Result};
use crate::protocol::command::{catch_payload, inject_payload, lock_payload};
use crate::protocol::opcode::{INJ_BTN, INJ_KEY, INJ_MEDIA, Q_VERSION};
use crate::protocol::{FrameDecoder, FrameType, Resp, encode, parse_resp};
use crate::transport::Transport;
use crate::types::Version;

use super::counters::Counters;
use super::reconcile::DesiredState;
use super::slot::TransportSlot;
use super::{Link, write_frame};

const AUTO_RECONNECT_MIN: Duration = Duration::from_millis(100);
const AUTO_RECONNECT_MAX: Duration = Duration::from_secs(2);

/// How long to wait for a `RESP(VERSION)` when confirming a rescanned port is our box.
const PROBE_DEADLINE: Duration = Duration::from_millis(1200);
/// Re-send the version probe this often within the deadline (the box drops PC-owned state on a fresh
/// control-link open and can miss the first query while it settles).
const PROBE_QUERY_GAP: Duration = Duration::from_millis(300);

/// The opened box's stable identity: the CH343 serial (scan-time, may be absent) plus the device
/// chip's base MAC (authoritative, from `RESP(VERSION)`).
#[derive(Clone, Debug)]
pub(crate) struct BoxIdentity {
    pub(crate) serial: Option<String>,
    pub(crate) mac: [u8; 6],
}

pub(crate) struct ReconnectCtx {
    pub(crate) transport: Arc<TransportSlot>,
    pub(crate) write_lock: Arc<Mutex<()>>,
    pub(crate) seq: Arc<AtomicU8>,
    pub(crate) counters: Arc<Counters>,
    pub(crate) desired: Arc<Mutex<DesiredState>>,
    pub(crate) reconnect_lock: Arc<Mutex<()>>,
    pub(crate) identity: Arc<Mutex<Option<BoxIdentity>>>,
}

/// Probe a freshly-opened, not-yet-adopted transport for its `RESP(VERSION)` so a rescan can confirm
/// the MAC before committing. Runs a self-contained query loop off the reader thread.
fn probe_version(transport: &dyn Transport) -> Option<Version> {
    let frame = encode(FrameType::Query, 0, &[Q_VERSION]).ok()?;
    let mut decoder = FrameDecoder::new();
    let start = Instant::now();
    let mut last_query: Option<Instant> = None;
    let mut found = None;
    let mut rx = [0u8; 256];
    while found.is_none() && start.elapsed() < PROBE_DEADLINE {
        if last_query.is_none_or(|t| t.elapsed() >= PROBE_QUERY_GAP) {
            if transport.write_all(&frame).is_err() {
                return None;
            }
            last_query = Some(Instant::now());
        }
        match transport.read(&mut rx) {
            Ok(0) => {}
            Ok(n) => decoder.feed(&rx[..n], |f| {
                if f.ty == FrameType::Resp {
                    if let Some(Resp::Version(v)) = parse_resp(&f.payload) {
                        found = Some(v);
                    }
                }
            }),
            Err(_) => return None,
        }
    }
    found
}

fn reconnect(ctx: &ReconnectCtx) -> Result<()> {
    let _guard = ctx.reconnect_lock.lock();
    let identity = ctx.identity.lock().clone();
    let ports = crate::transport::scan::find_medius();

    // Candidate order: with a known serial, try the port(s) that match it first — fast and
    // unambiguous. If none match (the adapter serves no serial, or it changed), fall back to every
    // port and let the MAC confirm which one is ours.
    let candidates: Vec<_> = match &identity {
        Some(id) if id.serial.is_some() => {
            let matched: Vec<_> = ports
                .iter()
                .filter(|p| p.serial == id.serial)
                .cloned()
                .collect();
            if matched.is_empty() { ports } else { matched }
        }
        _ => ports,
    };
    if candidates.is_empty() {
        return Err(Error::NotFound);
    }

    ctx.transport.swap(Arc::new(crate::transport::Disconnected));
    std::thread::sleep(Duration::from_millis(200));

    for port in candidates {
        let serial =
            match crate::transport::serial::SerialTransport::open(std::path::Path::new(&port.path))
            {
                Ok(s) => s,
                Err(_) => continue,
            };
        // With an identity on record, confirm the MAC before committing so a rescan never adopts the
        // wrong box. Without one (a transport opened bare, e.g. a mock), accept the first that opens.
        if let Some(id) = &identity {
            match probe_version(&serial) {
                Some(v) if v.mac == id.mac => {}
                _ => continue,
            }
        }
        ctx.transport.swap(Arc::new(serial));
        ctx.counters.inc_reconnects();
        trace_event!(
            target: "medius::device",
            tracing::Level::INFO,
            port = %port.path,
            reason = "rescan",
            "reconnected",
        );
        return reapply_held(ctx);
    }
    Err(Error::NotFound)
}

fn reapply_held(ctx: &ReconnectCtx) -> Result<()> {
    let (held, held_keys, held_media, held_locks, catch) = {
        let d = ctx.desired.lock();
        (
            d.held().collect::<Vec<_>>(),
            d.held_keys().collect::<Vec<_>>(),
            d.held_media().collect::<Vec<_>>(),
            d.held_locks().collect::<Vec<_>>(),
            d.catch(),
        )
    };
    for (button, action) in held {
        let seq = ctx.seq.fetch_add(1, Ordering::Relaxed);
        write_frame(
            &ctx.transport,
            &ctx.write_lock,
            &ctx.counters,
            seq,
            FrameType::Inject,
            &inject_payload(INJ_BTN, button.as_id() as u16, action.as_u8()),
        )?;
    }
    for (key, action) in held_keys {
        let seq = ctx.seq.fetch_add(1, Ordering::Relaxed);
        write_frame(
            &ctx.transport,
            &ctx.write_lock,
            &ctx.counters,
            seq,
            FrameType::Inject,
            &inject_payload(INJ_KEY, key.usage() as u16, action.as_u8()),
        )?;
    }
    for (key, action) in held_media {
        let seq = ctx.seq.fetch_add(1, Ordering::Relaxed);
        write_frame(
            &ctx.transport,
            &ctx.write_lock,
            &ctx.counters,
            seq,
            FrameType::Inject,
            &inject_payload(INJ_MEDIA, key.usage(), action.as_u8()),
        )?;
    }
    // Re-assert held locks: like injection, the firmware silence-clears every lock after the ~1 s
    // window, so a blip past it would unlock physical input without this. (class, usage, direction).
    for (class, usage, direction) in held_locks {
        let seq = ctx.seq.fetch_add(1, Ordering::Relaxed);
        write_frame(
            &ctx.transport,
            &ctx.write_lock,
            &ctx.counters,
            seq,
            FrameType::Lock,
            &lock_payload(class, usage, direction, 1),
        )?;
    }
    // Re-assert the catch subscription: a control-link drop longer than the firmware's ~1 s silence
    // window makes the box silence-clear the mask, so without this the stream would stay dead after a
    // long blip. Idempotent if the drop was short (the mask was never cleared).
    if !catch.is_empty() {
        let seq = ctx.seq.fetch_add(1, Ordering::Relaxed);
        write_frame(
            &ctx.transport,
            &ctx.write_lock,
            &ctx.counters,
            seq,
            FrameType::Catch,
            &catch_payload(catch.bits()),
        )?;
    }
    Ok(())
}

pub(crate) fn auto_reconnect(ctx: &ReconnectCtx, stop: &AtomicBool) {
    let mut backoff = AUTO_RECONNECT_MIN;
    while !stop.load(Ordering::SeqCst) {
        if reconnect(ctx).is_ok() {
            return;
        }
        std::thread::sleep(backoff);
        backoff = (backoff * 2).min(AUTO_RECONNECT_MAX);
    }
}

impl Link {
    fn reconnect_ctx(&self) -> ReconnectCtx {
        ReconnectCtx {
            transport: Arc::clone(&self.inner.transport),
            write_lock: Arc::clone(&self.inner.write_lock),
            seq: Arc::clone(&self.inner.seq),
            counters: Arc::clone(&self.inner.counters),
            desired: Arc::clone(&self.inner.desired),
            reconnect_lock: Arc::clone(&self.inner.reconnect_lock),
            identity: Arc::clone(&self.inner.identity),
        }
    }

    /// Record the box's stable identity so a later rescan reconnects to this same box, not whichever
    /// one happens to be plugged in.
    pub(crate) fn set_identity(&self, id: BoxIdentity) {
        *self.inner.identity.lock() = Some(id);
    }

    pub(crate) fn reconnect(&self) -> Result<()> {
        reconnect(&self.reconnect_ctx())
    }

    pub(crate) fn reapply(&self) -> Result<()> {
        reapply_held(&self.reconnect_ctx())
    }
}
