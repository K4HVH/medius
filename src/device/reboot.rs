//! Keepalive thread, reboot, and reconnect (§8 keepalive / §9 reboot+reconnect).
//!
//! The firmware auto-clears all injection after **1000 ms** of control-PC silence (§5.4) so a host
//! crash never leaves a button stuck. That same auto-clear would drop an *intentionally* held override
//! if the host went quiet, so the keepalive thread sends one cheap frame per cadence tick (default
//! 500 ms) **only while the desired state is non-idle**; while idle it sends nothing, leaving the
//! safety auto-clear intact for a real crash. The frame is a fire-and-go `QUERY(HEALTH)` with no waiter
//! registered, so its `RESP` is discarded and it never contends with `pending`.
//!
//! [`Device::reconnect`] rescans by VID/PID, reopens, and swaps the transport into the shared
//! [`TransportSlot`] (the running reader/keepalive follow it), then re-applies the held state and bumps
//! the `reconnects` counter.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

use parking_lot::Mutex;

use crate::error::{Error, Result};
use crate::protocol::FrameType;
use crate::protocol::command::query_payload;
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

impl Device {
    /// Re-send every currently held override — used after a reconnect and on demand to re-assert the
    /// intended state on the box (§8 auto-reapply).
    pub(crate) fn reapply(&self) -> Result<()> {
        // Snapshot under the lock, then release before sending (lock-ordering).
        let held: Vec<_> = self.desired().lock().held().collect();
        for (button, action) in held {
            self.button(button, action)?;
        }
        Ok(())
    }

    /// Reboot a chip (§9): `REBOOT_DL` with the [`RebootTarget`] byte, which fully encodes both the chip
    /// (device/host) **and** the mode (run/download) — `2`/`3` run the firmware, `0`/`1` (device/host)
    /// drop into ROM download for a pre-flash handoff. Fire-and-go (the chip is rebooting, no reply).
    pub fn reboot(&self, target: RebootTarget) -> Result<()> {
        self.send(FrameType::RebootDl, &[target.as_u8()])
    }

    /// Best-effort reconnect (§6): rescan by VID/PID, reopen, swap into the shared slot (the running
    /// reader/keepalive follow it), re-apply the held state, and bump the `reconnects` counter.
    ///
    /// # Errors
    /// [`Error::NotFound`] if no port matches; [`Error::Io`] if the reopen fails.
    pub fn reconnect(&self) -> Result<()> {
        let port = crate::transport::scan::find_medius()
            .into_iter()
            .next()
            .ok_or(Error::NotFound)?;
        // serialport holds the port exclusively, so release the current one before reopening: swap in
        // a disconnected placeholder and give the reader one read timeout (~100 ms) to drop the old
        // handle, then reopen. (On a real reconnect the box re-enumerated and the old handle is already
        // dead; this also makes reopening the *same* path work.)
        self.transport_slot()
            .swap(Arc::new(crate::transport::Disconnected));
        std::thread::sleep(Duration::from_millis(200));
        let serial =
            crate::transport::serial::SerialTransport::open(std::path::Path::new(&port.path))?;
        self.transport_slot().swap(Arc::new(serial));
        self.counters_inner().inc_reconnects();
        trace_event!(
            target: "medius::device",
            tracing::Level::INFO,
            port = %port.path,
            reason = "rescan",
            "reconnected",
        );
        self.reapply()
    }
}
