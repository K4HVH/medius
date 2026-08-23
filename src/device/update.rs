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
            credit: if credit == 0 { OTA_CREDIT } else { credit as usize },
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
            let info = self.firmware_info()?;
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
        let (status, arg) = self.update_op(OTA_OP_ACTIVATE, UpdateTarget::Device, &[], ACTIVATE_TIMEOUT)?;
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
        self.activate_firmware()
    }

    pub(crate) fn update_op(
        &self,
        op: u8,
        target: UpdateTarget,
        body: &[u8],
        timeout: Duration,
    ) -> Result<(UpdateStatus, u32)> {
        // Drain anything already queued for this op: it answers an earlier command, and taking it as
        // this one's reply would report a stale outcome.
        while let Ok(p) = self.link.updates_rx().try_recv() {
            if p.first() != Some(&op) {
                continue;
            }
        }
        let mut frame = Vec::with_capacity(2 + body.len());
        frame.push(op);
        frame.push(target.as_u8());
        frame.extend_from_slice(body);
        self.link.send(FrameType::Update, &frame)?;
        self.recv_update(op, timeout)
    }

    /// The next `UPDATE_RESP` for `op`. Matched on the op byte, not `SEQ`: one acknowledgement answers
    /// a whole window of `DATA` frames, so it carries a rolling `SEQ` of its own.
    pub(crate) fn recv_update(&self, op: u8, timeout: Duration) -> Result<(UpdateStatus, u32)> {
        let deadline = Instant::now() + timeout;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(Error::QueryTimeout);
            }
            match self.link.updates_rx().recv_timeout(left) {
                Ok(p) if p.len() >= UPD_RESP_LEN && p[0] == op => {
                    return Ok((
                        UpdateStatus(p[2]),
                        u32::from_le_bytes([p[3], p[4], p[5], p[6]]),
                    ));
                }
                Ok(_) => continue,
                Err(_) => return Err(Error::QueryTimeout),
            }
        }
    }
}
