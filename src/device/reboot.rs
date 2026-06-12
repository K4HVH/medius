//! Keepalive thread, reboot, and reconnect (§8 keepalive / §9 reboot+reconnect).
//!
//! ## Keepalive
//!
//! The firmware auto-clears all injection after **1000 ms** of control-PC silence (§5.4) — a safety
//! property so a host crash never leaves a button stuck. That same auto-clear would, however, drop an
//! *intentionally* held override (`press`/`force_release`) if the host went quiet. The keepalive
//! thread defeats it **only while the desired state is non-idle**: it sends one cheap frame per
//! cadence tick (default 500 ms, sub-1 s) so a held override survives, and when the state is idle it
//! sends **nothing** — leaving the firmware safety auto-clear fully intact for a real crash.
//!
//! The cheap frame is a fire-and-go `QUERY(HEALTH)` whose `RESP` (if any) the reader simply discards
//! (no waiter is registered), so it never contends with the `pending` map or the pacer.
//!
//! ## Reconnect
//!
//! [`Device::reconnect`] rescans by VID/PID, reopens the transport, and **swaps it into the shared
//! [`TransportSlot`]** — the same reader and keepalive threads load the current transport each
//! operation, so they follow onto the new port with no thread restart. It then re-applies the held
//! desired state and bumps the `reconnects` counter.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

use parking_lot::Mutex;

use crate::error::{Error, Result};
use crate::protocol::FrameType;
use crate::protocol::command::query_payload;
use crate::protocol::opcode::Q_HEALTH;
use crate::protocol::types::RebootTarget;

use super::reconcile::DesiredState;
use super::{Counters, Device, TransportSlot, write_frame};

/// Maximum slice the keepalive sleeps before re-checking `stop`, so shutdown stays prompt even with a
/// long cadence. The cadence is realized as a sum of these slices.
const KEEPALIVE_STOP_POLL: Duration = Duration::from_millis(20);

/// Everything the keepalive thread needs — the *write* state and `desired`, never `Arc<Inner>` (same
/// anti-cycle discipline as the reader).
pub(crate) struct KeepaliveCtx {
    pub(crate) transport: Arc<TransportSlot>,
    pub(crate) write_lock: Arc<Mutex<()>>,
    pub(crate) seq: Arc<AtomicU8>,
    pub(crate) counters: Arc<Counters>,
    pub(crate) desired: Arc<Mutex<DesiredState>>,
    pub(crate) stop: Arc<AtomicBool>,
    pub(crate) cadence: Duration,
}

/// Spawn the keepalive thread (see the [module docs](self)).
pub(crate) fn spawn_keepalive(ctx: KeepaliveCtx) -> JoinHandle<()> {
    std::thread::Builder::new()
        .name("medius-keepalive".into())
        .spawn(move || keepalive_loop(ctx))
        .expect("spawn medius-keepalive thread")
}

/// The keepalive loop: each cadence tick, send a cheap frame **iff** the desired state is non-idle.
fn keepalive_loop(ctx: KeepaliveCtx) {
    loop {
        // Sleep the cadence in short slices so `stop` is observed promptly.
        if sleep_cadence(&ctx.stop, ctx.cadence) {
            return; // stop requested
        }
        // Snapshot idleness under the lock, then release BEFORE sending (never hold two locks).
        let idle = ctx.desired.lock().is_idle();
        if idle {
            continue; // idle ⇒ send nothing; the firmware safety auto-clear stays intact
        }
        let seq = ctx.seq.fetch_add(1, Ordering::Relaxed);
        // Fire-and-go QUERY(HEALTH): no waiter registered, so the RESP is harmlessly dropped by the
        // reader. A send error is ignored — the next tick retries, and reconnect heals a dead port.
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
    /// Re-send every currently held override (`press`/`force_release`) — used after a reconnect and
    /// available on demand to re-assert the intended state on the box (§8 auto-reapply).
    ///
    /// Soft-released (`None`) buttons are skipped: there is no held state to restore.
    pub fn reapply(&self) -> Result<()> {
        // Snapshot the held set under the lock, then release before sending (lock-ordering).
        let held: Vec<_> = self.desired().lock().held().collect();
        for (button, action) in held {
            self.button(button, action)?;
        }
        Ok(())
    }

    /// Reboot a chip **to run** the firmware (§9): `REBOOT_DL` with target `2` (device) or `3` (host).
    ///
    /// Fire-and-go (no reply — the chip is rebooting). Accepts any [`RebootTarget`]; the run targets
    /// are [`RebootTarget::DeviceRun`] / [`RebootTarget::HostRun`].
    pub fn reboot(&self, target: RebootTarget) -> Result<()> {
        self.send(FrameType::RebootDl, &[target.as_u8()])
    }

    /// Reboot a chip **to ROM download** (§9, pre-flash): `REBOOT_DL` with target `0` (device) or `1`
    /// (host). Fire-and-go.
    pub fn reboot_download(&self, target: RebootTarget) -> Result<()> {
        self.send(FrameType::RebootDl, &[target.as_u8()])
    }

    /// Best-effort reconnect (§6): rescan by VID/PID, reopen the transport, swap it into the shared
    /// slot (the running reader/keepalive follow onto it), re-apply the held desired state, and bump
    /// the `reconnects` counter.
    ///
    /// # Errors
    /// [`Error::NotFound`] if no candidate port matches; [`Error::Io`] if the reopen fails.
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
        // Re-assert held overrides on the fresh link.
        self.reapply()
    }
}

/// Open the raw platform serial transport at `path` (no handshake) — shared by [`Device::reconnect`].
#[cfg(target_os = "linux")]
fn open_raw(path: &str) -> Result<Arc<dyn crate::transport::Transport>> {
    let serial = crate::transport::linux::LinuxSerial::open(std::path::Path::new(path))?;
    Ok(Arc::new(serial))
}

/// Open the raw platform serial transport at `path` (no handshake) — shared by [`Device::reconnect`].
#[cfg(windows)]
fn open_raw(path: &str) -> Result<Arc<dyn crate::transport::Transport>> {
    let serial = crate::transport::windows::WindowsSerial::open(std::path::Path::new(path))?;
    Ok(Arc::new(serial))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use crate::protocol::types::{Button, LogLevel, RebootTarget};
    use crate::protocol::{DecodedFrame, FrameDecoder, FrameType, encode};
    use crate::transport::mock::MockTransport;

    use super::Device;

    /// Build a device over a mock with a SHORT keepalive cadence so the keepalive's behaviour is
    /// observable in milliseconds, not the real 500 ms.
    fn device_fast_keepalive(cadence_ms: u64) -> (Device, Arc<MockTransport>) {
        let mock = Arc::new(MockTransport::new());
        let device =
            Device::from_transport_with_cadence(mock.clone(), Duration::from_millis(cadence_ms));
        (device, mock)
    }

    fn written_frames(mock: &MockTransport) -> Vec<DecodedFrame> {
        FrameDecoder::new().feed_collect(&mock.written())
    }

    /// While the desired state is non-idle (a held press), the keepalive emits frames within a few
    /// cadence windows.
    #[test]
    fn keepalive_fires_while_non_idle() {
        let (device, mock) = device_fast_keepalive(5);
        device.press(Button::Left).unwrap();
        let _ = mock.written(); // drain the PRESS frame so we only see keepalive frames next

        // Wait a handful of cadence windows.
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
        let _ = mock.written(); // ensure clean slate
        // Never touch desired state → stays idle.
        std::thread::sleep(Duration::from_millis(60));
        let frames = written_frames(&mock);
        assert!(
            frames.is_empty(),
            "idle keepalive must send nothing, saw {} frames",
            frames.len()
        );
        // sanity: the device is real (a command still works)
        device.move_rel(1, 0).unwrap();
        assert_eq!(written_frames(&mock).len(), 1);
    }

    /// Stopping (drop) halts the keepalive: no new frames appear after drop.
    #[test]
    fn keepalive_stops_on_drop() {
        let (device, mock) = device_fast_keepalive(5);
        device.press(Button::Left).unwrap();
        std::thread::sleep(Duration::from_millis(30));
        drop(device);
        let _ = mock.written(); // clear everything emitted so far
        std::thread::sleep(Duration::from_millis(40));
        assert!(
            written_frames(&mock).is_empty(),
            "keepalive must stop after the device is dropped"
        );
    }

    /// `reapply` re-emits exactly the held overrides (press + force-release), skipping released ones.
    #[test]
    fn reapply_re_emits_held_overrides() {
        // Long cadence so the keepalive doesn't add frames during this test window.
        let mock = Arc::new(MockTransport::new());
        let device = Device::from_transport_with_cadence(mock.clone(), Duration::from_secs(60));
        device.press(Button::Left).unwrap();
        device.force_release(Button::Side1).unwrap();
        device.press(Button::Middle).unwrap();
        device.release(Button::Middle).unwrap(); // soft-release → not held
        let _ = mock.written(); // drain the command frames

        device.reapply().unwrap();
        let frames = written_frames(&mock);
        // Two held overrides re-emitted: Left=press [0,1], Side1=force [3,2]. Middle is NOT re-sent.
        let buttons: Vec<Vec<u8>> = frames
            .iter()
            .filter(|f| f.ty == FrameType::Button)
            .map(|f| f.payload.clone())
            .collect();
        assert_eq!(buttons, vec![vec![0, 1], vec![3, 2]]);
    }

    /// `reboot` emits `REBOOT_DL` with the run target byte; `reboot_download` with the download byte.
    #[test]
    fn reboot_emits_correct_target_bytes() {
        let mock = Arc::new(MockTransport::new());
        let device = Device::from_transport_with_cadence(mock.clone(), Duration::from_secs(60));

        device.reboot(RebootTarget::DeviceRun).unwrap();
        device.reboot(RebootTarget::HostRun).unwrap();
        device
            .reboot_download(RebootTarget::DeviceDownload)
            .unwrap();
        device.reboot_download(RebootTarget::HostDownload).unwrap();

        let frames = written_frames(&mock);
        let reboots: Vec<u8> = frames
            .iter()
            .filter(|f| f.ty == FrameType::RebootDl)
            .map(|f| f.payload[0])
            .collect();
        assert_eq!(reboots, vec![2, 3, 0, 1]);
    }

    /// FIX 3 — a transport swap (reconnect) must reset the reader's `FrameDecoder`, so a frame
    /// interrupted mid-parse on the old port does NOT mis-frame the first bytes of the new one.
    ///
    /// We feed a *partial* LOG frame on mock A (leaving the decoder mid-frame), swap in mock B, push a
    /// *complete* LOG frame, and assert that the complete LOG decodes cleanly. Without the reset, A's
    /// dangling prefix would corrupt B's leading bytes and the LOG would never arrive intact.
    #[test]
    fn transport_swap_resets_decoder() {
        let mock_a = Arc::new(MockTransport::new());
        // Long cadence so the keepalive doesn't inject frames during the test window.
        let device = Device::from_transport_with_cadence(mock_a.clone(), Duration::from_secs(60));
        let rx = device.logs();

        // Feed a TRUNCATED LOG frame on A: encode a full frame, push only its first half so the reader's
        // decoder is left waiting for the remainder.
        let partial = encode(FrameType::Log, 0, &[2, b'o', b'l', b'd']).unwrap();
        let cut = partial.len() / 2;
        mock_a.push_bytes(&partial[..cut]);
        // Give the reader a moment to consume the partial bytes into its decoder.
        std::thread::sleep(Duration::from_millis(20));

        // Swap in a fresh transport (as reconnect does) and push a COMPLETE LOG frame on it.
        let mock_b = Arc::new(MockTransport::new());
        device.transport_slot().swap(mock_b.clone());
        mock_b.push_frame(FrameType::Log, 0, &[2, b'n', b'e', b'w']);

        // The complete LOG must arrive intact — proving the decoder was reset (the old partial discarded).
        let line = rx
            .recv_timeout(Duration::from_secs(1))
            .expect("the post-swap LOG must decode cleanly");
        assert_eq!(line.level, LogLevel::Info);
        assert_eq!(line.text, "new");
    }
}
