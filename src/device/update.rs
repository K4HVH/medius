use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};

use crate::error::{Error, Result};
use crate::protocol::FrameType;
use crate::protocol::opcode::{
    OTA_CHUNK, OTA_CREDIT, OTA_OP_ABORT, OTA_OP_ACTIVATE, OTA_OP_BEGIN, OTA_OP_DATA, OTA_OP_END,
    Q_FIRMWARE, UPD_RESP_LEN,
};
use crate::protocol::{Resp, parse_resp};
use crate::types::{FirmwareInfo, UpdateProgress, UpdateStatus, UpdateTarget};

use super::Device;

/// How long one op may take to answer. `BEGIN` erases the whole slot before it replies.
pub(crate) const OP_TIMEOUT: Duration = Duration::from_secs(20);
/// `ACTIVATE` reboots the host chip and waits for it back on the link before the device chip follows.
pub(crate) const ACTIVATE_TIMEOUT: Duration = Duration::from_secs(60);
/// How often a blocked receive wakes to check what another caller may have parked for it.
const HELD_POLL: Duration = Duration::from_millis(50);

/// A chip confirms the image it booted after about ten seconds of running.
pub(crate) const CONFIRM_TIMEOUT: Duration = Duration::from_secs(45);

/// The chunking and credit accounting for one staged image, with no transport in it. The sync and
/// async transfers both drive this, so the wire logic exists once and cannot drift between them.
pub(crate) struct ChunkPlan<'a> {
    image: &'a [u8],
    target: UpdateTarget,
    credit: usize,
    seq: u16,
    sent: usize,
    unacked: usize,
}

impl<'a> ChunkPlan<'a> {
    pub(crate) fn new(image: &'a [u8], target: UpdateTarget, credit: u32) -> Self {
        ChunkPlan {
            image,
            target,
            credit: if credit == 0 {
                OTA_CREDIT
            } else {
                credit as usize
            },
            seq: 0,
            sent: 0,
            unacked: 0,
        }
    }

    pub(crate) fn done(&self) -> bool {
        self.sent >= self.image.len() && self.unacked == 0
    }

    /// The next `DATA` frame, or `None` when everything sent is still waiting to be acknowledged.
    pub(crate) fn next_frame(&mut self) -> Option<Vec<u8>> {
        if self.sent >= self.image.len() || self.unacked >= self.credit {
            return None;
        }
        let end = (self.sent + OTA_CHUNK).min(self.image.len());
        let mut frame = Vec::with_capacity(4 + (end - self.sent));
        frame.push(OTA_OP_DATA);
        frame.push(self.target.as_u8());
        frame.extend_from_slice(&self.seq.to_le_bytes());
        frame.extend_from_slice(&self.image[self.sent..end]);
        self.seq = self.seq.wrapping_add(1);
        self.sent = end;
        self.unacked += 1;
        Some(frame)
    }

    /// True once the window is full or the image is out, which is when the box owes an answer.
    pub(crate) fn awaiting_ack(&self) -> bool {
        self.unacked > 0 && (self.unacked >= self.credit || self.sent >= self.image.len())
    }

    pub(crate) fn on_ack(&mut self, status: UpdateStatus, arg: u32) -> Result<UpdateProgress> {
        if status != UpdateStatus::OK && status != UpdateStatus::ACK {
            return Err(Error::Update {
                op: OTA_OP_DATA,
                status,
                arg,
            });
        }
        // The box reports the chunk it expects next. A disagreement means the two sides no longer
        // share an offset, and writing on would put bytes in the wrong place.
        if arg != u32::from(self.seq) {
            return Err(Error::Update {
                op: OTA_OP_DATA,
                status: UpdateStatus::SEQ_GAP,
                arg,
            });
        }
        self.unacked = 0;
        Ok(UpdateProgress {
            target: self.target,
            sent: self.sent,
            total: self.image.len(),
        })
    }
}

/// The `BEGIN` body: the image length and the digest the box checks at `END`.
pub(crate) fn begin_body(image: &[u8]) -> Vec<u8> {
    let mut body = Vec::with_capacity(4 + 32);
    body.extend_from_slice(&(image.len() as u32).to_le_bytes());
    body.extend_from_slice(&Sha256::digest(image));
    body
}

impl Device {
    /// Both chips' firmware versions and which app slot each booted (§4.16).
    pub fn firmware_info(&self) -> Result<FirmwareInfo> {
        let payload = self.link.query(Q_FIRMWARE)?;
        match parse_resp(&payload) {
            Some(Resp::Firmware(f)) => Ok(f),
            _ => Err(Error::NoReply),
        }
    }

    /// Block until neither chip is still on probation. A chip that has not confirmed the image it
    /// booted refuses to open another update, and confirming runs on a timer nobody can hurry.
    pub fn wait_firmware_confirmed(&self) -> Result<FirmwareInfo> {
        let deadline = Instant::now() + CONFIRM_TIMEOUT;
        loop {
            // This is the call you make right after an activate, when the box is rebooting into the
            // image it is about to confirm. A query that goes unanswered there is the expected state,
            // so keep asking until the deadline rather than reporting the reboot as a failure.
            let info = match self.firmware_info() {
                Ok(i) => i,
                // Only a timeout. The CH343 stays enumerated while the chip behind it reboots, so a
                // reboot reads as an unanswered query. Anything else is a real fault, and this is
                // also called at the top of staging, where waiting 45 s on a box that is not there
                // would hide it.
                Err(Error::QueryTimeout) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(500));
                    continue;
                }
                Err(e) => return Err(e),
            };
            if !info.any_pending() {
                return Ok(info);
            }
            if Instant::now() >= deadline {
                return Err(Error::Update {
                    op: OTA_OP_BEGIN,
                    status: UpdateStatus::ON_PROBATION,
                    arg: 0,
                });
            }
            std::thread::sleep(Duration::from_millis(500));
        }
    }

    /// Write one image into the target chip's spare slot. It stays inert until
    /// [`activate_firmware`](Self::activate_firmware): nothing boots it, and a power cut brings the
    /// running image back.
    pub fn stage_firmware(
        &self,
        target: UpdateTarget,
        image: &[u8],
        progress: &mut dyn FnMut(UpdateProgress),
    ) -> Result<u32> {
        if image.is_empty() {
            return Err(Error::Update {
                op: OTA_OP_BEGIN,
                status: UpdateStatus::TOO_BIG,
                arg: 0,
            });
        }
        self.wait_firmware_confirmed()?;

        let (status, arg) = self.update_op(OTA_OP_BEGIN, target, &begin_body(image), OP_TIMEOUT)?;
        if status != UpdateStatus::READY {
            return Err(Error::Update {
                op: OTA_OP_BEGIN,
                status,
                arg,
            });
        }
        // AFTER the BEGIN reply, not before it. A DATA acknowledgement left over from an abandoned
        // attempt has arg == credit, which is exactly what the first window expects, so it passes the
        // offset check and runs the loop a window ahead of the box for the rest of the transfer. But
        // before the BEGIN it is still sitting in the channel, where dropping held replies cannot
        // reach it -- and awaiting the BEGIN is itself what moves it across. The box answers in
        // order, so everything from the old attempt is behind that reply on the wire.
        self.drop_held(OTA_OP_DATA);

        let mut plan = ChunkPlan::new(image, target, arg);
        while !plan.done() {
            while let Some(frame) = plan.next_frame() {
                self.link.send(FrameType::Update, &frame)?;
            }
            if plan.awaiting_ack() {
                let (status, arg) = self.recv_update(OTA_OP_DATA, OP_TIMEOUT)?;
                progress(plan.on_ack(status, arg)?);
            }
        }

        let (status, arg) = self.update_op(OTA_OP_END, target, &[], OP_TIMEOUT)?;
        if status != UpdateStatus::STAGED {
            return Err(Error::Update {
                op: OTA_OP_END,
                status,
                arg,
            });
        }
        Ok(arg)
    }

    /// Drop whatever is staged or in flight for one target. The clone comes back without a reboot.
    pub fn abort_update(&self, target: UpdateTarget) -> Result<()> {
        let (status, arg) = self.update_op(OTA_OP_ABORT, target, &[], OP_TIMEOUT)?;
        if status != UpdateStatus::OK {
            return Err(Error::Update {
                op: OTA_OP_ABORT,
                status,
                arg,
            });
        }
        Ok(())
    }

    /// Commit every staged image and reboot into it. The host chip goes first and has to be back on
    /// the inter-chip link before the device chip follows, so this can take tens of seconds.
    pub fn activate_firmware(&self) -> Result<()> {
        let (status, arg) =
            self.update_op(OTA_OP_ACTIVATE, UpdateTarget::Device, &[], ACTIVATE_TIMEOUT)?;
        if status != UpdateStatus::OK {
            return Err(Error::Update {
                op: OTA_OP_ACTIVATE,
                status,
                arg,
            });
        }
        Ok(())
    }

    /// Stage one image and activate it.
    pub fn update_firmware(
        &self,
        target: UpdateTarget,
        image: &[u8],
        progress: &mut dyn FnMut(UpdateProgress),
    ) -> Result<()> {
        self.stage_firmware(target, image, progress)?;
        // A refused activate leaves the image staged and armed, so a later unrelated activate would
        // commit it on its own. Disarm it. Best effort, because the usual reason for being here is
        // that the box is no longer answering, and the activate's error is the one worth reporting.
        if let Err(e) = self.activate_firmware() {
            let _ = self.abort_update(target);
            return Err(e);
        }
        Ok(())
    }

    pub(crate) fn update_op(
        &self,
        op: u8,
        target: UpdateTarget,
        body: &[u8],
        timeout: Duration,
    ) -> Result<(UpdateStatus, u32)> {
        // Anything already queued for THIS op answers an earlier command, and taking it as this
        // one's reply would report a stale outcome. Everything else belongs to somebody and is
        // parked rather than dropped.
        while let Ok(p) = self.link.updates_rx().try_recv() {
            if p.first() != Some(&op) {
                self.link.hold_update(p);
            }
        }
        self.drop_held(op);
        let mut frame = Vec::with_capacity(2 + body.len());
        frame.push(op);
        frame.push(target.as_u8());
        frame.extend_from_slice(body);
        self.link.send(FrameType::Update, &frame)?;
        self.recv_update(op, timeout)
    }

    /// The next `UPDATE_RESP` for `op`. Matched on the op byte, not `SEQ`: one acknowledgement answers
    /// a whole window of `DATA` frames, so it carries a rolling `SEQ` of its own.
    ///
    /// A reply for another op is put BACK, not dropped. The channel is one shared MPMC receiver and
    /// `AsyncDevice::offload` runs transfers on threads of their own, so discarding here would eat
    /// another caller's answer and leave it timing out against a box that replied correctly.
    pub(crate) fn recv_update(&self, op: u8, timeout: Duration) -> Result<(UpdateStatus, u32)> {
        let deadline = Instant::now() + timeout;
        loop {
            // Checked on EVERY wake, not just before the first wait: another caller can park this
            // op's reply at any point, and only looking once would leave it sitting there until the
            // next call while this one timed out.
            if let Some(p) = self.take_held(op) {
                return Ok((
                    UpdateStatus(p[2]),
                    u32::from_le_bytes([p[3], p[4], p[5], p[6]]),
                ));
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(Error::QueryTimeout);
            }
            // Bounded, so a reply parked while this thread is blocked is noticed promptly rather
            // than only when the channel happens to deliver something.
            match self.link.updates_rx().recv_timeout(left.min(HELD_POLL)) {
                Ok(p) if p.len() >= UPD_RESP_LEN && p[0] == op => {
                    return Ok((
                        UpdateStatus(p[2]),
                        u32::from_le_bytes([p[3], p[4], p[5], p[6]]),
                    ));
                }
                // Park it where its own caller looks. Re-sending it into the channel after this call
                // finished would arrive long after that caller had given up.
                Ok(p) => self.link.hold_update(p),
                Err(flume::RecvTimeoutError::Timeout) => continue,
                Err(_) => return Err(Error::Disconnected),
            }
        }
    }

    fn take_held(&self, op: u8) -> Option<Vec<u8>> {
        let mut held = self.link.held_updates().lock();
        let i = held
            .iter()
            .position(|p: &Vec<u8>| p.len() >= UPD_RESP_LEN && p[0] == op)?;
        Some(held.remove(i))
    }

    fn drop_held(&self, op: u8) {
        self.link
            .held_updates()
            .lock()
            .retain(|p| p.first() != Some(&op));
    }
}
