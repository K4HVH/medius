use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use parking_lot::Mutex;

use crate::protocol::FrameType;
use crate::protocol::command::{catch_payload, rewrite_payload, transform_payload};

use super::correlation::PendingEntry;
use super::counters::Counters;
use super::reconcile::DesiredState;
use super::restart::{self, Cause, Outcome, Pending, RestartWatch};
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
    // The query path and the hello watch a restart recovery runs on.
    pub(crate) pending: Arc<Mutex<HashMap<u8, PendingEntry>>>,
    pub(crate) query_gen: Arc<AtomicU64>,
    pub(crate) restart: Arc<RestartWatch>,
}

pub(crate) fn spawn_keepalive(ctx: KeepaliveCtx) -> JoinHandle<()> {
    std::thread::Builder::new()
        .name("medius-keepalive".into())
        .spawn(move || keepalive_loop(ctx))
        .expect("spawn medius-keepalive thread")
}

fn keepalive_loop(ctx: KeepaliveCtx) {
    let mut tick_at = Instant::now() + ctx.cadence;
    let mut pending: Option<Pending> = None;
    loop {
        if ctx.stop.load(Ordering::SeqCst) {
            return;
        }
        let now = Instant::now();
        if ctx.restart.take_owed() {
            pending = Some(Pending::new(Cause::Restart, now));
        }
        if let Some(p) = pending.as_mut()
            && now >= p.next
        {
            match restart::step(&ctx, p.cause) {
                Outcome::Done => pending = None,
                Outcome::Again => *p = Pending::new(Cause::Restart, Instant::now()),
                Outcome::Wait => p.wait(Instant::now()),
            }
            continue;
        }
        let idle = ctx.desired.lock().is_idle();
        if now < tick_at {
            // After a command that can re-present the clone, check every slice, not every tick, so
            // released state goes back as soon as the clone is up.
            if pending.is_none()
                && !idle
                && ctx.restart.expecting_represent(now)
                && restart::session_released(&ctx)
            {
                ctx.restart.begin_release();
                pending = Some(Pending::new(Cause::Released, Instant::now()));
                continue;
            }
            std::thread::sleep(KEEPALIVE_STOP_POLL.min(tick_at - now));
            continue;
        }
        tick_at = now + ctx.cadence;
        if idle {
            continue;
        }
        // The query feeds the firmware silence timer (§5.4) as well.
        if pending.is_none() && restart::session_released(&ctx) {
            ctx.restart.begin_release();
            pending = Some(Pending::new(Cause::Released, Instant::now()));
            continue;
        }
        // Whether the ring holds the clip, so a later release marks lost only a clip the box had.
        if ctx.desired.lock().clip_ring_held() {
            restart::read_ring(&ctx);
        }
        let _serial = ctx.catch_lock.lock();
        let (catch, rewrites, transforms) = {
            let d = ctx.desired.lock();
            (d.catch(), d.held_rewrites(), d.held_transforms())
        };
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
    }
}
