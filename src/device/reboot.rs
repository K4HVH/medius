//! Keepalive thread, reboot, and reconnect (§8 keepalive / §9 reboot+reconnect).
//!
//! The firmware auto-clears all injection after **1000 ms** of control-PC silence (§5.4) so a host
//! crash never leaves a button stuck. That same auto-clear would drop an *intentionally* held override
//! if the host went quiet, so the keepalive thread sends one cheap frame per cadence tick (default
//! 500 ms) **only while the desired state is non-idle**; while idle it sends nothing, leaving the
//! safety auto-clear intact for a real crash. The frame is a fire-and-go `QUERY(HEALTH)` with no waiter
//! registered, so its `RESP` is discarded and it never contends with `pending`.
//!
//! Reconnect rescans by VID/PID, reopens, and swaps the transport into the shared [`TransportSlot`]
//! (the running reader/keepalive follow it), then re-applies the held state and bumps the `reconnects`
//! counter. [`Device::reconnect`] drives it on demand; the reader drives the *same* path
//! ([`auto_reconnect`]) unattended when a read errors (a likely disconnect). Both share a
//! [`ReconnectCtx`] of plain `Arc`s — never `Arc<Inner>` — so the reader, which `Inner::drop` joins,
//! can never pin `Inner` and self-join.

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

/// Max slice the keepalive sleeps before re-checking `stop`, so shutdown stays prompt under a long
/// cadence (realized as a sum of these slices).
const KEEPALIVE_STOP_POLL: Duration = Duration::from_millis(20);

/// Everything the keepalive thread needs — the write state and `desired`, never `Arc<Inner>` (anti-cycle).
pub(crate) struct KeepaliveCtx {
    pub(crate) transport: Arc<TransportSlot>,
    pub(crate) write_lock: Arc<Mutex<()>>,
    pub(crate) seq: Arc<AtomicU8>,
    pub(crate) counters: Arc<Counters>,
    pub(crate) desired: Arc<Mutex<DesiredState>>,
    pub(crate) stop: Arc<AtomicBool>,
    pub(crate) cadence: Duration,
}

/// Spawn the keepalive thread.
pub(crate) fn spawn_keepalive(ctx: KeepaliveCtx) -> JoinHandle<()> {
    std::thread::Builder::new()
        .name("medius-keepalive".into())
        .spawn(move || keepalive_loop(ctx))
        .expect("spawn medius-keepalive thread")
}

/// The keepalive loop: each cadence tick, send a cheap frame iff the desired state is non-idle.
fn keepalive_loop(ctx: KeepaliveCtx) {
    loop {
        if sleep_cadence(&ctx.stop, ctx.cadence) {
            return; // stop requested
        }
        // Release the lock BEFORE sending (never hold two locks).
        let idle = ctx.desired.lock().is_idle();
        if idle {
            continue; // idle ⇒ send nothing; the firmware safety auto-clear stays intact
        }
        let seq = ctx.seq.fetch_add(1, Ordering::Relaxed);
        // Fire-and-go: no waiter, so the RESP is dropped. A send error is ignored — the next tick
        // retries and reconnect heals a dead port.
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

/// Sleep `cadence` in `KEEPALIVE_STOP_POLL` slices; return `true` if `stop` was set during the wait.
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

/// Initial / max back-off between auto-reconnect attempts. Doubles from min to max so a box that's
/// gone for a while isn't polled tightly.
const AUTO_RECONNECT_MIN: Duration = Duration::from_millis(100);
const AUTO_RECONNECT_MAX: Duration = Duration::from_secs(2);

/// Everything a reconnect needs — the write state, `desired`, and the reconnect lock — never
/// `Arc<Inner>` (anti-cycle). The reader holds one of these to auto-reconnect; because it contains no
/// `Arc<Inner>`, the reader (which `Inner::drop` joins) can never become the last `Inner` owner and
/// self-join. [`Device::reconnect_ctx`] builds it from the shared `Inner` `Arc`s.
pub(crate) struct ReconnectCtx {
    pub(crate) transport: Arc<TransportSlot>,
    pub(crate) write_lock: Arc<Mutex<()>>,
    pub(crate) seq: Arc<AtomicU8>,
    pub(crate) counters: Arc<Counters>,
    pub(crate) desired: Arc<Mutex<DesiredState>>,
    pub(crate) reconnect_lock: Arc<Mutex<()>>,
}

/// One best-effort reconnect: rescan by VID/PID, reopen, swap into the shared slot (the running
/// reader/keepalive follow it), re-apply the held state, and bump the `reconnects` counter.
fn reconnect(ctx: &ReconnectCtx) -> Result<()> {
    // Serialize a manual [`Device::reconnect`] against the reader's auto-reconnect so the port is
    // never opened twice.
    let _guard = ctx.reconnect_lock.lock();
    let port = crate::transport::scan::find_medius()
        .into_iter()
        .next()
        .ok_or(Error::NotFound)?;
    // serialport holds the port exclusively, so release the current one before reopening: swap in a
    // disconnected placeholder and give the reader one read timeout (~100 ms) to drop the old handle,
    // then reopen. (On a real reconnect the box re-enumerated and the old handle is already dead; this
    // also makes reopening the *same* path work.)
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

/// Re-send every currently held override (§8 auto-reapply) — used after a reconnect and on demand.
fn reapply_held(ctx: &ReconnectCtx) -> Result<()> {
    // Snapshot under the lock, then release before sending (lock-ordering).
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

/// On a read error, rescan + reopen the box with exponential back-off until it succeeds or `stop` is
/// set. Runs on the reader thread (nothing to read while disconnected). A successful reconnect bumps
/// the generation, so the reader resets its decoder and the loop resumes on the new transport.
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
    /// Build a [`ReconnectCtx`] from the shared `Inner` `Arc`s (clones only the byte-pipe/write state,
    /// never `Arc<Inner>`).
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

    /// Re-send every currently held override to re-assert the intended state on the box (§8
    /// auto-reapply) — run automatically after a [`reconnect`](Device::reconnect), and available on
    /// demand. A no-op while idle.
    pub fn reapply(&self) -> Result<()> {
        reapply_held(&self.reconnect_ctx())
    }

    /// Reboot a chip (§9): `REBOOT_DL` with the [`RebootTarget`] byte, which fully encodes both the chip
    /// (device/host) **and** the mode (run/download) — `2`/`3` run the firmware, `0`/`1` (device/host)
    /// drop into ROM download for a pre-flash handoff. Fire-and-go (the chip is rebooting, no reply).
    pub fn reboot(&self, target: RebootTarget) -> Result<()> {
        self.send(FrameType::RebootDl, &[target.as_u8()])
    }

    /// Best-effort reconnect (§6): rescan by VID/PID, reopen, swap into the shared slot (the running
    /// reader/keepalive follow it), re-apply the held state, and bump the `reconnects` counter. The
    /// reader performs the same recovery unattended on a read error.
    ///
    /// # Errors
    /// [`Error::NotFound`] if no port matches; [`Error::Io`] if the reopen fails.
    pub fn reconnect(&self) -> Result<()> {
        reconnect(&self.reconnect_ctx())
    }
}
