use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

use crate::device::raw::validate_raw_direction;
use crate::error::{Error, Result};
use crate::link::Link;
use crate::protocol::command::{clip_op_payload, clip_set_payload, clip_trigger_payload};
use crate::protocol::opcode::{
    CLIP_COND_ANY_CLASS, CLIP_COND_ANY_ID, CLIP_OP_CLEAR, CLIP_OP_FINALIZE, CLIP_OP_PAUSE,
    CLIP_OP_RESTART, CLIP_OP_RESUME, CLIP_OP_START, CLIP_OP_STOP, CLIP_OP_TOGGLE,
    CLIP_SET_AUTOLOCK, CLIP_SET_LOOP, CLIP_SET_RETAIN, CLIP_SET_RIDE, CLIP_TRIG_F_CONSUME,
    CLIP_TRIG_F_PRESENT, LOCK_DIR_BOTH, MAX_PAYLOAD, Q_CLIP,
};
use crate::protocol::opcode::{
    CLIP_F_EDGES, CLIP_F_PAN, CLIP_F_RAW, CLIP_F_WHEEL, CLIP_F_XFER, CLIP_F_XY, CLIP_TAG_GAP,
};
use crate::protocol::{FrameType, Resp, parse_resp};
use crate::types::clip::ClipEntry;
use crate::types::lock::blanket_scope;
use crate::types::{
    Blanket, CLIP_EDGES_MAX, CLIP_ENTRY_MAX, CLIP_RAW_MAX, ClipBuilder, ClipFrame, ClipSettings,
    ClipStatus, ClipTrigger, Edge, Usage,
};

use super::Device;

// One frame onto `out`. The fields follow the tag in the order below, which is not the flags' bit order.
fn encode_frame(f: &ClipFrame, out: &mut Vec<u8>) -> Result<()> {
    if f.edges.len() > CLIP_EDGES_MAX {
        return Err(Error::ClipFrameCount {
            what: "edges",
            count: f.edges.len(),
            limit: CLIP_EDGES_MAX,
        });
    }
    if f.raw.len() > CLIP_RAW_MAX {
        return Err(Error::ClipFrameCount {
            what: "raw reports",
            count: f.raw.len(),
            limit: CLIP_RAW_MAX,
        });
    }
    for item in &f.raw {
        validate_raw_direction(item.direction)?;
    }
    for item in &f.transfers {
        let want = if item.setup.is_in() {
            0
        } else {
            item.setup.length as usize
        };
        if item.out.len() != want {
            return Err(Error::ClipTransferData {
                want,
                got: item.out.len(),
            });
        }
    }
    let len = f.byte_len();
    if len > CLIP_ENTRY_MAX {
        return Err(Error::ClipFrameTooLong { len });
    }
    let mut flags = 0u8;
    if f.dx != 0 || f.dy != 0 {
        flags |= CLIP_F_XY;
    }
    if f.wheel != 0 {
        flags |= CLIP_F_WHEEL;
    }
    if f.pan != 0 {
        flags |= CLIP_F_PAN;
    }
    if !f.edges.is_empty() {
        flags |= CLIP_F_EDGES;
    }
    if !f.raw.is_empty() {
        flags |= CLIP_F_RAW;
    }
    if !f.transfers.is_empty() {
        flags |= CLIP_F_XFER;
    }
    if flags == 0 {
        flags = CLIP_F_XY; // an empty content tick would collide with the gap tag: emit a zero XY tick
    }
    out.push(flags);
    if flags & CLIP_F_XY != 0 {
        out.extend_from_slice(&f.dx.to_le_bytes());
        out.extend_from_slice(&f.dy.to_le_bytes());
    }
    if flags & CLIP_F_WHEEL != 0 {
        out.extend_from_slice(&f.wheel.to_le_bytes());
    }
    if flags & CLIP_F_PAN != 0 {
        out.extend_from_slice(&f.pan.to_le_bytes());
    }
    if flags & CLIP_F_EDGES != 0 {
        out.push(f.edges.len() as u8);
        for &(usage, action) in &f.edges {
            let (class, id) = usage.class_id();
            out.push(class);
            out.extend_from_slice(&id.to_le_bytes());
            out.push(action.as_u8());
        }
    }
    if flags & CLIP_F_RAW != 0 {
        out.push(f.raw.len() as u8);
        for item in &f.raw {
            out.push(item.ep & 0x0f);
            out.push(item.direction.as_u8());
            out.extend_from_slice(&(item.bytes.len() as u16).to_le_bytes());
            out.extend_from_slice(&item.bytes);
        }
    }
    if flags & CLIP_F_XFER != 0 {
        // A frame that fits CLIP_ENTRY_MAX holds at most 56 transfers, so the count fits its byte.
        out.push(f.transfers.len() as u8);
        for item in &f.transfers {
            out.push(item.ep);
            out.extend_from_slice(&item.setup.to_bytes());
            out.extend_from_slice(&item.out);
        }
    }
    Ok(())
}

/// The builder's stream cut into pieces of at most `limit` bytes, each ending on an entry boundary: an
/// entry never spans two `CLIP_APPEND` frames. Every entry is validated before any piece is returned.
pub(crate) fn encode_chunks(clip: &ClipBuilder, limit: usize) -> Result<Vec<Vec<u8>>> {
    let mut out: Vec<Vec<u8>> = Vec::new();
    let mut cur: Vec<u8> = Vec::new();
    let mut entry: Vec<u8> = Vec::new();
    for e in &clip.entries {
        entry.clear();
        match e {
            ClipEntry::Gap(n) => {
                entry.push(CLIP_TAG_GAP);
                entry.extend_from_slice(&n.to_le_bytes());
            }
            ClipEntry::Frame(f) => encode_frame(f, &mut entry)?,
        }
        if !cur.is_empty() && cur.len() + entry.len() > limit {
            out.push(std::mem::take(&mut cur));
        }
        cur.extend_from_slice(&entry);
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    Ok(out)
}

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
///
/// The keepalive holds a loaded clip, a setting off its default and a bound trigger past the box's
/// silence window. A reconnect re-sends none of it and keeps alive what the box still holds: a link
/// down for longer than that window leaves nothing, so reload the clip and its config.
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
    ///
    /// Every entry is checked before the first frame goes out, so a refusal sends nothing: a
    /// [`ClipFrame`](crate::ClipFrame) past its edge or raw-report count, one that encodes past
    /// [`CLIP_ENTRY_MAX`](crate::CLIP_ENTRY_MAX), a raw report whose direction is neither IN nor OUT,
    /// or a transfer whose data does not match its setup packet.
    pub fn append(&self, clip: &ClipBuilder) -> Result<()> {
        let chunks = encode_chunks(clip, MAX_PAYLOAD)?;
        let _serial = self.link.reassert_guard();
        for chunk in chunks {
            self.send_chunk(&chunk)?;
            self.link.desired().lock().clip_loaded(true);
        }
        Ok(())
    }

    fn ctrl(&self, op: u8) -> Result<()> {
        self.link.send(FrameType::ClipCtrl, &clip_op_payload(op))
    }

    // What the keepalive holds of the clip is recorded once the frame is out, under the guard a
    // reconnect takes to read the box's clip back, so neither can land between the two.
    fn set(&self, id: u8, value: u8) -> Result<()> {
        let _serial = self.link.reassert_guard();
        self.link
            .send(FrameType::ClipSet, &clip_set_payload(id, value))?;
        self.link.desired().lock().clip_setting(id, value);
        Ok(())
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

    /// Make the clip's motion wait to ride a native report under [`set_movement_riding`](crate::Device::set_movement_riding) (`false` = the box's own clock, the default); only its wheel and pan while rendering is on with a profile armed. Changeable mid-playback. Fire-and-forget.
    pub fn set_ride(&self, on: bool) -> Result<()> {
        self.set(CLIP_SET_RIDE, on as u8)
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
        let _serial = self.link.reassert_guard();
        self.link.send(
            FrameType::ClipTrigger,
            &clip_trigger_payload(
                class,
                id,
                trigger.edge.as_u8(),
                trigger.action.as_u8(),
                flags,
            ),
        )?;
        self.link
            .desired()
            .lock()
            .clip_trigger((class, id, trigger.edge.as_u8()), true);
        Ok(())
    }

    /// Remove the binding on `usage`'s `edge`. Fire-and-forget.
    pub fn unbind(&self, usage: impl Into<Usage>, edge: Edge) -> Result<()> {
        let (class, id) = usage.into().class_id();
        let _serial = self.link.reassert_guard();
        self.link.send(
            FrameType::ClipTrigger,
            &clip_trigger_payload(class, id, edge.as_u8(), 0, 0),
        )?;
        self.link
            .desired()
            .lock()
            .clip_trigger((class, id, edge.as_u8()), false);
        Ok(())
    }

    /// Remove every trigger binding. Fire-and-forget.
    pub fn clear_triggers(&self) -> Result<()> {
        let _serial = self.link.reassert_guard();
        self.link.send(
            FrameType::ClipTrigger,
            &clip_trigger_payload(CLIP_COND_ANY_CLASS, CLIP_COND_ANY_ID, LOCK_DIR_BOTH, 0, 0),
        )?;
        self.link.desired().lock().clip_triggers_clear();
        Ok(())
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
        let _serial = self.link.reassert_guard();
        self.ctrl(CLIP_OP_CLEAR)?;
        self.link.desired().lock().clip_loaded(false);
        Ok(())
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
