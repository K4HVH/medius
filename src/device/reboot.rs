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
    #[cfg(any(target_os = "linux", windows))]
    pub fn reconnect(&self) -> Result<()> {
        let port = crate::transport::scan::find_medius()
            .into_iter()
            .next()
            .ok_or(Error::NotFound)?;
        let transport = open_raw(&port.path)?;
        self.transport_slot().swap(transport);
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

/// Open the raw platform serial transport at `path` (no handshake), for [`Device::reconnect`].
#[cfg(target_os = "linux")]
fn open_raw(path: &str) -> Result<Arc<dyn crate::transport::Transport>> {
    let serial = crate::transport::linux::LinuxSerial::open(std::path::Path::new(path))?;
    Ok(Arc::new(serial))
}

/// Open the raw platform serial transport at `path` (no handshake), for [`Device::reconnect`].
#[cfg(windows)]
fn open_raw(path: &str) -> Result<Arc<dyn crate::transport::Transport>> {
    let serial = crate::transport::windows::WindowsSerial::open(std::path::Path::new(path))?;
    Ok(Arc::new(serial))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use crate::protocol::{DecodedFrame, FrameDecoder, FrameType, encode};
    use crate::transport::mock::MockTransport;
    use crate::types::{Button, LogLevel, RebootTarget};

    use super::Device;

    /// Device with a SHORT keepalive cadence so its behaviour is observable in milliseconds.
    fn device_fast_keepalive(cadence_ms: u64) -> (Device, Arc<MockTransport>) {
        let mock = Arc::new(MockTransport::new());
        let device =
            Device::from_transport_with_cadence(mock.clone(), Duration::from_millis(cadence_ms));
        (device, mock)
    }

    fn written_frames(mock: &MockTransport) -> Vec<DecodedFrame> {
        FrameDecoder::new().feed_collect(&mock.written())
    }

    /// While non-idle (a held press), the keepalive emits frames within a few cadence windows.
    #[test]
    fn keepalive_fires_while_non_idle() {
        let (device, mock) = device_fast_keepalive(5);
        device.press(Button::Left).unwrap();
        let _ = mock.written(); // drain the PRESS so we only see keepalive frames next

        std::thread::sleep(Duration::from_millis(60));
        let frames = written_frames(&mock);
        assert!(
            frames.iter().any(|f| f.ty == FrameType::Query),
            "expected at least one keepalive QUERY while non-idle, saw {} frames",
            frames.len()
        );
    }

    /// While idle, the keepalive emits NOTHING (so the firmware safety auto-clear stays intact).
    #[test]
    fn keepalive_silent_while_idle() {
        let (device, mock) = device_fast_keepalive(5);
        let _ = mock.written();
        // Never touch desired state → stays idle.
        std::thread::sleep(Duration::from_millis(60));
        let frames = written_frames(&mock);
        assert!(
            frames.is_empty(),
            "idle keepalive must send nothing, saw {} frames",
            frames.len()
        );
        // Sanity: a command still works.
        device.move_rel(1, 0).unwrap();
        assert_eq!(written_frames(&mock).len(), 1);
    }

    /// Dropping the device halts the keepalive: no new frames appear after drop.
    #[test]
    fn keepalive_stops_on_drop() {
        let (device, mock) = device_fast_keepalive(5);
        device.press(Button::Left).unwrap();
        std::thread::sleep(Duration::from_millis(30));
        drop(device);
        let _ = mock.written(); // clear what was emitted so far
        std::thread::sleep(Duration::from_millis(40));
        assert!(
            written_frames(&mock).is_empty(),
            "keepalive must stop after the device is dropped"
        );
    }

    /// `reapply` re-emits exactly the held overrides (press + force-release), skipping released ones.
    #[test]
    fn reapply_re_emits_held_overrides() {
        // Long cadence so the keepalive doesn't add frames during this window.
        let mock = Arc::new(MockTransport::new());
        let device = Device::from_transport_with_cadence(mock.clone(), Duration::from_secs(60));
        device.press(Button::Left).unwrap();
        device.force_release(Button::Side1).unwrap();
        device.press(Button::Middle).unwrap();
        device.soft_release(Button::Middle).unwrap(); // soft-release → not held
        let _ = mock.written(); // drain command frames

        device.reapply().unwrap();
        let frames = written_frames(&mock);
        // Held overrides re-emitted: Left=press [0,1], Side1=force [3,2]. Middle is NOT re-sent.
        let buttons: Vec<Vec<u8>> = frames
            .iter()
            .filter(|f| f.ty == FrameType::Button)
            .map(|f| f.payload.clone())
            .collect();
        assert_eq!(buttons, vec![vec![0, 1], vec![3, 2]]);
    }

    /// `reboot` emits `REBOOT_DL` with the target byte — run (`2`/`3`) and download (`0`/`1`) alike,
    /// since [`RebootTarget`] fully encodes both chip and mode.
    #[test]
    fn reboot_emits_correct_target_bytes() {
        let mock = Arc::new(MockTransport::new());
        let device = Device::from_transport_with_cadence(mock.clone(), Duration::from_secs(60));

        device.reboot(RebootTarget::DeviceRun).unwrap();
        device.reboot(RebootTarget::HostRun).unwrap();
        device.reboot(RebootTarget::DeviceDownload).unwrap();
        device.reboot(RebootTarget::HostDownload).unwrap();

        let frames = written_frames(&mock);
        let reboots: Vec<u8> = frames
            .iter()
            .filter(|f| f.ty == FrameType::RebootDl)
            .map(|f| f.payload[0])
            .collect();
        assert_eq!(reboots, vec![2, 3, 0, 1]);
    }

    /// FIX 3 — a transport swap must reset the reader's `FrameDecoder`, so a frame interrupted mid-parse
    /// on the old port does NOT mis-frame the first bytes of the new one. Without the reset, A's
    /// dangling prefix would corrupt B's leading bytes.
    #[test]
    fn transport_swap_resets_decoder() {
        let mock_a = Arc::new(MockTransport::new());
        // Long cadence so the keepalive doesn't inject frames during the window.
        let device = Device::from_transport_with_cadence(mock_a.clone(), Duration::from_secs(60));
        let rx = device.logs();

        // Push only the first half of a LOG frame on A, leaving the decoder mid-frame.
        let partial = encode(FrameType::Log, 0, &[2, b'o', b'l', b'd']).unwrap();
        let cut = partial.len() / 2;
        mock_a.push_bytes(&partial[..cut]);
        std::thread::sleep(Duration::from_millis(20)); // let the reader consume the partial bytes

        // Swap in a fresh transport (as reconnect does) and push a COMPLETE LOG on it.
        let mock_b = Arc::new(MockTransport::new());
        device.transport_slot().swap(mock_b.clone());
        mock_b.push_frame(FrameType::Log, 0, &[2, b'n', b'e', b'w']);

        // The complete LOG arriving intact proves the decoder was reset.
        let line = rx
            .recv_timeout(Duration::from_secs(1))
            .expect("the post-swap LOG must decode cleanly");
        assert_eq!(line.level, LogLevel::Info);
        assert_eq!(line.text, "new");
    }
}
