use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

use parking_lot::Mutex;

use crate::error::{Error, Result};
use crate::protocol::FrameType;
use crate::protocol::command::{button_payload, query_payload};
use crate::protocol::opcode::Q_HEALTH;
use crate::types::RebootTarget;

use super::reconcile::DesiredState;
use super::{Counters, Device, TransportSlot, write_frame};

const KEEPALIVE_STOP_POLL: Duration = Duration::from_millis(20);

pub(crate) struct KeepaliveCtx {
    pub(crate) transport: Arc<TransportSlot>,
    pub(crate) write_lock: Arc<Mutex<()>>,
    pub(crate) seq: Arc<AtomicU8>,
    pub(crate) counters: Arc<Counters>,
    pub(crate) desired: Arc<Mutex<DesiredState>>,
    pub(crate) stop: Arc<AtomicBool>,
    pub(crate) cadence: Duration,
}

pub(crate) fn spawn_keepalive(ctx: KeepaliveCtx) -> JoinHandle<()> {
    std::thread::Builder::new()
        .name("medius-keepalive".into())
        .spawn(move || keepalive_loop(ctx))
        .expect("spawn medius-keepalive thread")
}

fn keepalive_loop(ctx: KeepaliveCtx) {
    loop {
        if sleep_cadence(&ctx.stop, ctx.cadence) {
            return;
        }
        let idle = ctx.desired.lock().is_idle();
        if idle {
            continue;
        }
        let seq = ctx.seq.fetch_add(1, Ordering::Relaxed);
        let _ = write_frame(
            &ctx.transport,
            &ctx.write_lock,
            &ctx.counters,
            seq,
            FrameType::Query,
            &query_payload(Q_HEALTH),
        );
    }
}

fn sleep_cadence(stop: &AtomicBool, cadence: Duration) -> bool {
    let mut remaining = cadence;
    while !remaining.is_zero() {
        if stop.load(Ordering::SeqCst) {
            return true;
        }
        let slice = remaining.min(KEEPALIVE_STOP_POLL);
        std::thread::sleep(slice);
        remaining -= slice;
    }
    stop.load(Ordering::SeqCst)
}

const AUTO_RECONNECT_MIN: Duration = Duration::from_millis(100);
const AUTO_RECONNECT_MAX: Duration = Duration::from_secs(2);

pub(crate) struct ReconnectCtx {
    pub(crate) transport: Arc<TransportSlot>,
    pub(crate) write_lock: Arc<Mutex<()>>,
    pub(crate) seq: Arc<AtomicU8>,
    pub(crate) counters: Arc<Counters>,
    pub(crate) desired: Arc<Mutex<DesiredState>>,
    pub(crate) reconnect_lock: Arc<Mutex<()>>,
}

fn reconnect(ctx: &ReconnectCtx) -> Result<()> {
    let _guard = ctx.reconnect_lock.lock();
    let port = crate::transport::scan::find_medius()
        .into_iter()
        .next()
        .ok_or(Error::NotFound)?;
    ctx.transport.swap(Arc::new(crate::transport::Disconnected));
    std::thread::sleep(Duration::from_millis(200));
    let serial = crate::transport::serial::SerialTransport::open(std::path::Path::new(&port.path))?;
    ctx.transport.swap(Arc::new(serial));
    ctx.counters.inc_reconnects();
    trace_event!(
        target: "medius::device",
        tracing::Level::INFO,
        port = %port.path,
        reason = "rescan",
        "reconnected",
    );
    reapply_held(ctx)
}

fn reapply_held(ctx: &ReconnectCtx) -> Result<()> {
    let held: Vec<_> = ctx.desired.lock().held().collect();
    for (button, action) in held {
        let seq = ctx.seq.fetch_add(1, Ordering::Relaxed);
        write_frame(
            &ctx.transport,
            &ctx.write_lock,
            &ctx.counters,
            seq,
            FrameType::Button,
            &button_payload(button.as_id(), action.as_u8()),
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

impl Device {
    pub(crate) fn reconnect_ctx(&self) -> ReconnectCtx {
        ReconnectCtx {
            transport: Arc::clone(&self.inner.transport),
            write_lock: Arc::clone(&self.inner.write_lock),
            seq: Arc::clone(&self.inner.seq),
            counters: Arc::clone(&self.inner.counters),
            desired: Arc::clone(&self.inner.desired),
            reconnect_lock: Arc::clone(&self.inner.reconnect_lock),
        }
    }

    /// Re-send every currently held override to re-assert the intended state; no-op while idle.
    pub fn reapply(&self) -> Result<()> {
        reapply_held(&self.reconnect_ctx())
    }

    /// Reboot a chip via `REBOOT_DL` with the [`RebootTarget`] byte (fire-and-go).
    pub fn reboot(&self, target: RebootTarget) -> Result<()> {
        self.send(FrameType::RebootDl, &[target.as_u8()])
    }

    /// Best-effort reconnect: rescan by VID/PID, reopen, re-apply held state, bump the counter.
    pub fn reconnect(&self) -> Result<()> {
        reconnect(&self.reconnect_ctx())
    }
}
