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
    /// The same lock subscribe and unsubscribe commit under. Held across this thread's read of the
    /// desired set AND its sends, because between the two an unsubscribe can commit -- and then this
    /// thread re-adds the entry it just removed. The box would hold a table no subscriber wants and
    /// the crate's own set does not contain, so no later diff would ever remove it, and because the
    /// table stays non-empty the firmware's silence clear never fires either. On a vendor-bulk entry
    /// that is a quarter of a megabyte a second the link cannot carry, for the life of the connection.
    pub(crate) catch_lock: Arc<Mutex<()>>,
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
        let _serial = ctx.catch_lock.lock();
        let (idle, catch) = {
            let d = ctx.desired.lock();
            (d.is_idle(), d.catch())
        };
        if idle {
            continue;
        }
        // Both frames feed the firmware silence timer (§5.4) to keep a held override/subscription
        // alive. Re-sending the CATCH entries (not a bare QUERY) also restores the table if a device
        // blip cleared it box-side. Only subscribes go out, never an unsubscribe: a blanket clear and
        // re-add here would punch a hole in the stream on every cadence.
        if !catch.is_empty() {
            for f in &catch {
                let seq = ctx.seq.fetch_add(1, Ordering::Relaxed);
                let (class, id) = f.wire();
                let _ = write_frame(
                    &ctx.transport,
                    &ctx.write_lock,
                    &ctx.counters,
                    seq,
                    FrameType::Catch,
                    &catch_payload(class, id, f.direction.as_u8(), 1, f.snaplen),
                );
            }
            continue;
        }
        let seq = ctx.seq.fetch_add(1, Ordering::Relaxed);
        let (ty, payload): (FrameType, Vec<u8>) =
            (FrameType::Query, query_payload(Q_HEALTH).to_vec());
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
