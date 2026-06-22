use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

use parking_lot::Mutex;

use crate::protocol::FrameType;
use crate::protocol::command::{catch_payload, query_payload};
use crate::protocol::opcode::Q_HEALTH;

use super::counters::Counters;
use super::reconcile::DesiredState;
use super::slot::TransportSlot;
use super::write_frame;

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
        let (idle, catch) = {
            let d = ctx.desired.lock();
            (d.is_idle(), d.catch())
        };
        if idle {
            continue;
        }
        let seq = ctx.seq.fetch_add(1, Ordering::Relaxed);
        // Both frames feed the firmware silence timer (§5.4) to keep a held override / subscription
        // alive. When catch is active we re-send CATCH instead of a bare QUERY: that also restores the
        // mask if a device-side blip (mouse detach / inter-chip link loss) made the box clear it.
        let (ty, payload): (FrameType, Vec<u8>) = if catch.is_empty() {
            (FrameType::Query, query_payload(Q_HEALTH).to_vec())
        } else {
            (FrameType::Catch, catch_payload(catch.bits()).to_vec())
        };
        let _ = write_frame(
            &ctx.transport,
            &ctx.write_lock,
            &ctx.counters,
            seq,
            ty,
            &payload,
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
