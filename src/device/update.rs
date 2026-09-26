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

// `BEGIN` erases the whole slot before it replies.
pub(crate) const OP_TIMEOUT: Duration = Duration::from_secs(20);
// `ACTIVATE` reboots the host chip and waits for it back on the link before the device chip follows.
pub(crate) const ACTIVATE_TIMEOUT: Duration = Duration::from_secs(60);
// How often a blocked receive wakes to check for a reply another caller parked.
const HELD_POLL: Duration = Duration::from_millis(50);

// Outlasts the mouse-side chip's 40 s probation, the longer of the two.
pub(crate) const CONFIRM_TIMEOUT: Duration = Duration::from_secs(55);

// Chunking and credit accounting for one staged image, transport-free, so the sync and async
// transfers share one copy of the wire logic.
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

    // `None` while everything sent awaits acknowledgement.
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

    // The box owes a reply once the window is full or the image is out.
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
        // The box reports the chunk it expects next; a mismatch means the offsets diverged and
        // writing on would misplace bytes.
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

// Image length and the digest the box checks at `END`.
pub(crate) fn begin_body(image: &[u8]) -> Vec<u8> {
    let mut body = Vec::with_capacity(4 + 32);
    body.extend_from_slice(&(image.len() as u32).to_le_bytes());
    body.extend_from_slice(&Sha256::digest(image));
    body
}

impl Device {
    /// Both chips' firmware versions and booted app slots (§4.16).
    pub fn firmware_info(&self) -> Result<FirmwareInfo> {
        let payload = self.link.query(Q_FIRMWARE)?;
        match parse_resp(&payload) {
            Some(Resp::Firmware(f)) => Ok(f),
            _ => Err(Error::NoReply),
        }
    }

    /// Block until neither chip is on probation; a chip that has not confirmed its booted image
    /// refuses another update. The device chip confirms after ten seconds of running; the mouse-side
    /// chip only on a completed clock exchange over the inter-chip link.
    pub fn wait_firmware_confirmed(&self) -> Result<FirmwareInfo> {
        let deadline = Instant::now() + CONFIRM_TIMEOUT;
        loop {
            // Called right after an activate, while the box reboots into the image it will confirm.
            let info = match self.firmware_info() {
                Ok(i) => i,
                // Only a timeout: the CH343 stays enumerated while the chip behind it reboots, so a
                // reboot reads as an unanswered query.
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

    /// Write one image into the target chip's spare slot. Inert until
    /// [`activate_firmware`](Self::activate_firmware): nothing boots it, and a power cut restores the
    /// running image.
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
        // AFTER the BEGIN reply, not before it.
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

    /// Drop whatever is staged or in flight for one target; the clone returns without a reboot.
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

    /// Commit every staged image and reboot into it. The host chip goes first and must be back on
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
        // A refused activate leaves the image armed for a later unrelated activate; disarm it.
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
        // Anything queued for this op answers an earlier command; taking it would report a stale
        // outcome.
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

    // Matched on the op byte, not `SEQ`: one acknowledgement answers a window of `DATA` frames and
    // carries its own rolling `SEQ`. A reply for another op is parked, not dropped: the MPMC channel
    // is shared and `AsyncDevice::offload` runs transfers on their own threads, so dropping it would
    // time out another caller whose box replied.
    pub(crate) fn recv_update(&self, op: u8, timeout: Duration) -> Result<(UpdateStatus, u32)> {
        let deadline = Instant::now() + timeout;
        loop {
            // Checked on every wake: another caller can park this op's reply at any point.
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
            // Bounded, so a reply parked while blocked is noticed promptly.
            match self.link.updates_rx().recv_timeout(left.min(HELD_POLL)) {
                Ok(p) if p.len() >= UPD_RESP_LEN && p[0] == op => {
                    return Ok((
                        UpdateStatus(p[2]),
                        u32::from_le_bytes([p[3], p[4], p[5], p[6]]),
                    ));
                }
                // Parked where its caller looks; re-sent into the channel later, it would arrive after
                // that caller gave up.
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
