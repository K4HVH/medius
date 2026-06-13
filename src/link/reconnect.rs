use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::time::Duration;

use parking_lot::Mutex;

use crate::error::{Error, Result};
use crate::protocol::FrameType;
use crate::protocol::command::button_payload;

use super::counters::Counters;
use super::reconcile::DesiredState;
use super::slot::TransportSlot;
use super::{Link, write_frame};

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

impl Link {
    fn reconnect_ctx(&self) -> ReconnectCtx {
        ReconnectCtx {
            transport: Arc::clone(&self.inner.transport),
            write_lock: Arc::clone(&self.inner.write_lock),
            seq: Arc::clone(&self.inner.seq),
            counters: Arc::clone(&self.inner.counters),
            desired: Arc::clone(&self.inner.desired),
            reconnect_lock: Arc::clone(&self.inner.reconnect_lock),
        }
    }

    pub(crate) fn reconnect(&self) -> Result<()> {
        reconnect(&self.reconnect_ctx())
    }

    pub(crate) fn reapply(&self) -> Result<()> {
        reapply_held(&self.reconnect_ctx())
    }
}
