use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

use parking_lot::Mutex;

use crate::protocol::FrameType;
use crate::protocol::command::{catch_payload, query_payload, rewrite_payload, transform_payload};
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
    // The same lock subscribe and unsubscribe commit under.
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
        let (idle, catch, rewrites, transforms) = {
            let d = ctx.desired.lock();
            (
                d.is_idle(),
                d.catch(),
                d.held_rewrites(),
                d.held_transforms(),
            )
        };
        if idle {
            continue;
        }
        // Any frame feeds the firmware silence timer (§5.4) to hold a held
        // override/lock/subscription /rewrite alive.
        let mut sent_any = false;
        if !catch.is_empty() {
            for f in catch.values() {
                let seq = ctx.seq.fetch_add(1, Ordering::Relaxed);
                let (class, id) = f.wire();
                let _ = write_frame(
                    &ctx.transport,
                    &ctx.write_lock,
                    &ctx.counters,
                    seq,
                    FrameType::Catch,
                    &catch_payload(class, id, f.direction().as_u8(), 1, f.capture().as_u8()),
                );
            }
            sent_any = true;
        }
        if !rewrites.is_empty() {
            for r in rewrites {
                let seq = ctx.seq.fetch_add(1, Ordering::Relaxed);
                let _ = write_frame(
                    &ctx.transport,
                    &ctx.write_lock,
                    &ctx.counters,
                    seq,
                    FrameType::Rewrite,
                    &rewrite_payload(
                        r.class,
                        r.id,
                        r.direction,
                        1,
                        r.action,
                        r.offset,
                        &r.match_bytes,
                        &r.mask,
                        &r.payload,
                    ),
                );
            }
            sent_any = true;
        }
        if !transforms.is_empty() {
            for t in transforms {
                let seq = ctx.seq.fetch_add(1, Ordering::Relaxed);
                let _ = write_frame(
                    &ctx.transport,
                    &ctx.write_lock,
                    &ctx.counters,
                    seq,
                    FrameType::Transform,
                    &transform_payload(t.op, t.sclass, t.sid, t.dclass, t.did, 1),
                );
            }
            sent_any = true;
        }
        if sent_any {
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
