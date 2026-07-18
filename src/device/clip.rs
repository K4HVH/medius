use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

use crate::error::{Error, Result};
use crate::link::Link;
use crate::protocol::command::{clip_op_payload, clip_set_payload, clip_trigger_payload};
use crate::protocol::opcode::{
    CLIP_COND_ANY_CLASS, CLIP_COND_ANY_ID, CLIP_OP_CLEAR, CLIP_OP_FINALIZE, CLIP_OP_PAUSE,
    CLIP_OP_RESTART, CLIP_OP_RESUME, CLIP_OP_START, CLIP_OP_STOP, CLIP_OP_TOGGLE,
    CLIP_SET_AUTOLOCK, CLIP_SET_LOOP, CLIP_SET_RETAIN, CLIP_TRIG_F_CONSUME, CLIP_TRIG_F_PRESENT,
    LOCK_DIR_BOTH, MAX_PAYLOAD, Q_CLIP,
};
use crate::protocol::{FrameType, Resp, parse_resp};
use crate::types::lock::blanket_scope;
use crate::types::{Blanket, ClipBuilder, ClipSettings, ClipStatus, ClipTrigger, Edge, Usage};

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

    fn ctrl(&self, op: u8) -> Result<()> {
        self.link.send(FrameType::ClipCtrl, &clip_op_payload(op))
    }

    fn set(&self, id: u8, value: u8) -> Result<()> {
        self.link
            .send(FrameType::ClipSet, &clip_set_payload(id, value))
    }

    // --- Scalar settings (`CLIP_SET`). Set `retain` before the first `append`. ---

    /// Auto-lock these physical-input groups while the clip plays (clip-owned, released on stop). Fire-and-forget.
    pub fn set_autolock(&self, scope: &[Blanket]) -> Result<()> {
        self.set(CLIP_SET_AUTOLOCK, blanket_scope(scope))
    }

    /// Loop playback at the clip end (retained mode only). Fire-and-forget.
    pub fn set_loop(&self, on: bool) -> Result<()> {
        self.set(CLIP_SET_LOOP, on as u8)
    }

    /// Retain the loaded clip so it can rewind and replay (`false` = streaming, the default). Set it before the first [`append`](Self::append). Fire-and-forget.
    pub fn set_retain(&self, on: bool) -> Result<()> {
        self.set(CLIP_SET_RETAIN, on as u8)
    }

    // --- Trigger set (`CLIP_TRIGGER`), a managed set keyed by `(on, edge)`. ---

    /// Add or overwrite a trigger binding: `trigger`'s edge fires its action on the box, no host round-trip. Fire-and-forget.
    pub fn bind(&self, trigger: ClipTrigger) -> Result<()> {
        let (class, id) = trigger.on.class_id();
        let flags = CLIP_TRIG_F_PRESENT
            | if trigger.consume {
                CLIP_TRIG_F_CONSUME
            } else {
                0
            };
        self.link.send(
            FrameType::ClipTrigger,
            &clip_trigger_payload(
                class,
                id,
                trigger.edge.as_u8(),
                trigger.action.as_u8(),
                flags,
            ),
        )
    }

    /// Remove the binding on `usage`'s `edge`. Fire-and-forget.
    pub fn unbind(&self, usage: impl Into<Usage>, edge: Edge) -> Result<()> {
        let (class, id) = usage.into().class_id();
        self.link.send(
            FrameType::ClipTrigger,
            &clip_trigger_payload(class, id, edge.as_u8(), 0, 0),
        )
    }

    /// Remove every trigger binding. Fire-and-forget.
    pub fn clear_triggers(&self) -> Result<()> {
        self.link.send(
            FrameType::ClipTrigger,
            &clip_trigger_payload(CLIP_COND_ANY_CLASS, CLIP_COND_ANY_ID, LOCK_DIR_BOTH, 0, 0),
        )
    }

    // --- Engine verbs (`CLIP_CTRL`). ---

    /// Rewind and play (resume from a pause). Fire-and-forget.
    pub fn start(&self) -> Result<()> {
        self.ctrl(CLIP_OP_START)
    }

    /// Stop, flush a streaming clip (rewind a retained one), release held input and the clip auto-lock. Fire-and-forget.
    pub fn stop(&self) -> Result<()> {
        self.ctrl(CLIP_OP_STOP)
    }

    /// Halt mid-clip, retaining the cursor and any held input. Fire-and-forget.
    pub fn pause(&self) -> Result<()> {
        self.ctrl(CLIP_OP_PAUSE)
    }

    /// Continue from the paused cursor. Fire-and-forget.
    pub fn resume(&self) -> Result<()> {
        self.ctrl(CLIP_OP_RESUME)
    }

    /// Force a rewind and play, even mid-playback. Fire-and-forget.
    pub fn restart(&self) -> Result<()> {
        self.ctrl(CLIP_OP_RESTART)
    }

    /// Toggle: play if idle/paused, stop if playing. Fire-and-forget.
    pub fn toggle(&self) -> Result<()> {
        self.ctrl(CLIP_OP_TOGGLE)
    }

    /// Discard the loaded clip, free the ring, and clear a fault. Fire-and-forget.
    pub fn clear(&self) -> Result<()> {
        self.ctrl(CLIP_OP_CLEAR)
    }

    /// Finalize a retained clip: fix its end so it can replay and loop. Fire-and-forget.
    pub fn finalize(&self) -> Result<()> {
        self.ctrl(CLIP_OP_FINALIZE)
    }

    // --- Readback (`QUERY(CLIP)`), two views over the one `RESP(CLIP)` frame. ---

    /// `QUERY(CLIP)`: the ring depth, progress, and playback counters (§4.15).
    pub fn query_status(&self) -> Result<ClipStatus> {
        let payload = self.link.query(Q_CLIP)?;
        match parse_resp(&payload) {
            Some(Resp::Clip(s)) => Ok(s),
            _ => Err(Error::NoReply),
        }
    }

    /// `QUERY(CLIP)`: the clip configuration (autolock, loop, retain, finalized, and the trigger set) (§4.15).
    pub fn query_config(&self) -> Result<ClipSettings> {
        let payload = self.link.query(Q_CLIP)?;
        ClipSettings::from_payload(&payload).ok_or(Error::NoReply)
    }
}
