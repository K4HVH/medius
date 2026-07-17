use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

use crate::error::{Error, Result};
use crate::link::Link;
use crate::protocol::command::{clip_arm_payload, clip_op_payload, clip_start_payload};
use crate::protocol::opcode::{
    CLIP_COND_ANY_CLASS, CLIP_COND_ANY_ID, CLIP_OP_DISARM, CLIP_OP_STOP, MAX_PAYLOAD, Q_CLIP,
};
use crate::protocol::{FrameType, Resp, parse_resp};
use crate::types::{ClipBuilder, ClipConfig, ClipStatus, Usage};

use super::Device;

impl Device {
    /// A handle to this box's buffered-clip playback (§3.11): preload per-frame input into a device-side ring the box drains one entry per native frame.
    pub fn clip(&self) -> ClipHandle {
        ClipHandle {
            link: self.link.clone(),
            seq: Arc::new(AtomicU8::new(0)),
        }
    }
}

/// A handle to one box's buffered-clip playback, from [`Device::clip`]. Cloning shares the append-sequence counter.
#[derive(Clone, Debug)]
pub struct ClipHandle {
    link: Link,
    seq: Arc<AtomicU8>,
}

impl ClipHandle {
    #[cfg_attr(not(feature = "async"), allow(dead_code))]
    pub(crate) fn link(&self) -> &Link {
        &self.link
    }

    fn send_chunk(&self, chunk: &[u8]) -> Result<()> {
        let seq = self.seq.fetch_add(1, Ordering::Relaxed);
        self.link.send_with_seq(seq, FrameType::ClipAppend, chunk)
    }

    /// Append the builder's entries to the ring, split into whole-entry frames each stamped with the next append-sequence number. Fire-and-forget.
    pub fn append(&self, clip: &ClipBuilder) -> Result<()> {
        if clip.is_empty() {
            return Ok(());
        }
        let bytes = clip.as_bytes();
        let mut chunk_start = 0usize;
        let mut last_end = 0usize;
        for &end in clip.entry_ends() {
            if end - chunk_start > MAX_PAYLOAD {
                self.send_chunk(&bytes[chunk_start..last_end])?;
                chunk_start = last_end;
            }
            last_end = end;
        }
        if chunk_start < last_end {
            self.send_chunk(&bytes[chunk_start..last_end])?;
        }
        Ok(())
    }

    /// `CLIP_CTRL(START)`: begin playback from the ring head with the given [`ClipConfig`]. Fire-and-forget.
    pub fn start(&self, config: &ClipConfig) -> Result<()> {
        self.link.send(
            FrameType::ClipCtrl,
            &clip_start_payload(config.autolock_scope()),
        )
    }

    /// `CLIP_CTRL(STOP)`: stop playback, flush the ring, release any clip-owned auto-lock. Fire-and-forget.
    pub fn stop(&self) -> Result<()> {
        self.link
            .send(FrameType::ClipCtrl, &clip_op_payload(CLIP_OP_STOP))
    }

    /// `CLIP_CTRL(ARM_CATCH)`: arm an on-device trigger so playback starts locally on a physical press of `trigger`. Fire-and-forget.
    pub fn arm_catch(&self, trigger: impl Into<Usage>, config: &ClipConfig) -> Result<()> {
        let (class, id) = trigger.into().class_id();
        self.link.send(
            FrameType::ClipCtrl,
            &clip_arm_payload(class, id, config.autolock_scope()),
        )
    }

    /// `CLIP_CTRL(ARM_CATCH)` on any physical input: the next press of any button, key, or media usage fires playback. Fire-and-forget.
    pub fn arm_catch_any(&self, config: &ClipConfig) -> Result<()> {
        self.link.send(
            FrameType::ClipCtrl,
            &clip_arm_payload(
                CLIP_COND_ANY_CLASS,
                CLIP_COND_ANY_ID,
                config.autolock_scope(),
            ),
        )
    }

    /// `CLIP_CTRL(DISARM)`: clear a pending catch-arm. Fire-and-forget.
    pub fn disarm(&self) -> Result<()> {
        self.link
            .send(FrameType::ClipCtrl, &clip_op_payload(CLIP_OP_DISARM))
    }

    /// `QUERY(CLIP)`: the ring depth and playback counters (§4.15).
    pub fn status(&self) -> Result<ClipStatus> {
        let payload = self.link.query(Q_CLIP)?;
        match parse_resp(&payload) {
            Some(Resp::Clip(s)) => Ok(s),
            _ => Err(Error::NoReply),
        }
    }
}
