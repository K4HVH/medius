//! Buffered clip playback (§3.11 / §4.15): the per-frame entry stream a host preloads into the device-side
//! ring, and the ring/playback status. The box drains one entry per native frame and renders it through the
//! normal injection engine — same rate pacing, same movement riding, no override.

use crate::protocol::opcode::{
    CLIP_F_EDGES, CLIP_F_WHEEL, CLIP_F_XY, CLIP_TAG_GAP, INJ_BTN, INJ_KEY, INJ_MEDIA,
};
use crate::types::{Action, Button, Key, MediaKey};

/// The device-side clip lifecycle state ([`ClipStatus::state`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ClipState {
    /// No clip active.
    #[default]
    Idle,
    /// A catch-trigger is armed; playback starts on the physical button edge.
    Armed,
    /// Draining the ring, one entry per native frame.
    Playing,
    /// An append was dropped (a `CLIP_APPEND` frame lost) or the ring overflowed. The buffered stream is
    /// out of sync; the host must [`stop`](crate::ClipHandle::stop) and re-preload.
    Faulted,
}

impl ClipState {
    pub(crate) fn from_u8(v: u8) -> Option<ClipState> {
        Some(match v {
            0 => ClipState::Idle,
            1 => ClipState::Armed,
            2 => ClipState::Playing,
            3 => ClipState::Faulted,
            _ => return None,
        })
    }
}

/// A snapshot of the device-side clip ring and playback counters (§4.15). `free`/`used` pace top-ups (append
/// only while `free` has headroom); `state == `[`ClipState::Faulted`] means re-sync (stop + rebuild).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct ClipStatus {
    /// The lifecycle state.
    pub state: ClipState,
    /// Free bytes in the ring — the headroom for the next [`append`](crate::ClipHandle::append).
    pub free: u32,
    /// Buffered bytes not yet drained.
    pub used: u32,
    /// Entries played since the last start.
    pub ticks: u32,
    /// Underrun episodes (the ring ran dry mid-playback, then idled or refilled).
    pub underruns: u16,
    /// Appends dropped because the ring was full.
    pub overruns: u16,
    /// Append-sequence gaps seen (a dropped `CLIP_APPEND` frame).
    pub seq_gaps: u16,
    /// Whether a catch-trigger button is currently held.
    pub held: bool,
}

impl ClipStatus {
    /// Decode a `RESP(CLIP)` payload (§4.15): `[what][state u8][free u32][used u32][ticks u32]
    /// [underruns u16][overruns u16][seq_gaps u16][held u8]`, all little-endian (21 bytes).
    pub(crate) fn from_payload(p: &[u8]) -> Option<ClipStatus> {
        if p.len() < 21 {
            return None;
        }
        Some(ClipStatus {
            state: ClipState::from_u8(p[1])?,
            free: u32::from_le_bytes([p[2], p[3], p[4], p[5]]),
            used: u32::from_le_bytes([p[6], p[7], p[8], p[9]]),
            ticks: u32::from_le_bytes([p[10], p[11], p[12], p[13]]),
            underruns: u16::from_le_bytes([p[14], p[15]]),
            overruns: u16::from_le_bytes([p[16], p[17]]),
            seq_gaps: u16::from_le_bytes([p[18], p[19]]),
            held: p[20] != 0,
        })
    }
}

/// One clip edge: an injection [`Action`] on a class/id (button, key, or media), applied on a
/// [`ClipBuilder::frame`]. Sticky until a later frame changes it, exactly like
/// [`Device::inject`](crate::Device::inject).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ClipEdge {
    pub(crate) class: u8,
    pub(crate) id: u16,
    pub(crate) action: u8,
}

impl ClipEdge {
    /// A button edge.
    pub fn button(button: Button, action: Action) -> ClipEdge {
        ClipEdge {
            class: INJ_BTN,
            id: button.as_id() as u16,
            action: action.as_u8(),
        }
    }

    /// A keyboard key/modifier edge.
    pub fn key(key: Key, action: Action) -> ClipEdge {
        ClipEdge {
            class: INJ_KEY,
            id: key.usage() as u16,
            action: action.as_u8(),
        }
    }

    /// A media (Consumer) usage edge.
    pub fn media(media: MediaKey, action: Action) -> ClipEdge {
        ClipEdge {
            class: INJ_MEDIA,
            id: media.usage(),
            action: action.as_u8(),
        }
    }

    /// Build an edge from the raw INJECT wire tuple (`class` 0 button / 1 key / 2 media, a class-specific
    /// `id`, and the [`Action`] byte). The typed constructors above are preferred; this is for bindings
    /// that carry the tuple across an FFI boundary.
    pub fn raw(class: u8, id: u16, action: u8) -> ClipEdge {
        ClipEdge { class, id, action }
    }

    /// The injection class (0 button / 1 key / 2 media).
    pub fn class(self) -> u8 {
        self.class
    }

    /// The class-specific usage id.
    pub fn id(self) -> u16 {
        self.id
    }

    /// The wire [`Action`] byte.
    pub fn action(self) -> u8 {
        self.action
    }
}

/// Max edges on one [`ClipBuilder::frame`], matching the firmware's `CLIP_EDGES_MAX`. More than this on a
/// single frame is rejected by the box (the frame faults); it is far past any realistic single report.
pub const CLIP_EDGES_MAX: usize = 8;

/// Builds a buffered-clip entry stream (§3.11) for [`ClipHandle::append`](crate::ClipHandle::append). Each
/// method appends one per-frame entry: motion is a relative delta, edges are [`Action`]s that stick until a
/// later frame changes them (like [`Device::inject`](crate::Device::inject)), and a [`gap`](Self::gap) run
/// emits nothing for N frames (a faithful idle poll). Mirrors the firmware entry codec byte-for-byte.
///
/// The builder holds a growing stream you keep topping up; [`ClipHandle::append`](crate::ClipHandle::append)
/// borrows it (call
/// [`clear`](Self::clear) to reuse the allocation, or make a fresh builder per top-up).
#[derive(Debug, Default, Clone)]
pub struct ClipBuilder {
    bytes: Vec<u8>,
    ends: Vec<usize>, // ends[i] = byte offset just past entry i, so an append chunks on entry boundaries
}

impl ClipBuilder {
    /// A new empty builder.
    pub fn new() -> ClipBuilder {
        ClipBuilder::default()
    }

    /// A gap run: emit nothing for `frames` native frames (the endpoint NAKs like an idle mouse). A zero
    /// count is a no-op.
    pub fn gap(&mut self, frames: u16) -> &mut Self {
        if frames == 0 {
            return self;
        }
        self.bytes.push(CLIP_TAG_GAP);
        self.bytes.extend_from_slice(&frames.to_le_bytes());
        self.ends.push(self.bytes.len());
        self
    }

    /// A content frame: a relative motion delta (`dx`/`dy` cursor, `wheel`) plus a list of edges. An
    /// all-zero frame with no edges still emits a report (a zero-motion tick, never a gap). At most
    /// [`CLIP_EDGES_MAX`] edges.
    pub fn frame(&mut self, dx: i16, dy: i16, wheel: i16, edges: &[ClipEdge]) -> &mut Self {
        let mut flags = 0u8;
        if dx != 0 || dy != 0 {
            flags |= CLIP_F_XY;
        }
        if wheel != 0 {
            flags |= CLIP_F_WHEEL;
        }
        if !edges.is_empty() {
            flags |= CLIP_F_EDGES;
        }
        if flags == 0 {
            flags = CLIP_F_XY; // an empty content tick would collide with the gap tag: emit a zero XY tick
        }
        self.bytes.push(flags);
        if flags & CLIP_F_XY != 0 {
            self.bytes.extend_from_slice(&dx.to_le_bytes());
            self.bytes.extend_from_slice(&dy.to_le_bytes());
        }
        if flags & CLIP_F_WHEEL != 0 {
            self.bytes.extend_from_slice(&wheel.to_le_bytes());
        }
        if flags & CLIP_F_EDGES != 0 {
            self.bytes.push(edges.len() as u8);
            for e in edges {
                self.bytes.push(e.class);
                self.bytes.extend_from_slice(&e.id.to_le_bytes());
                self.bytes.push(e.action);
            }
        }
        self.ends.push(self.bytes.len());
        self
    }

    /// A cursor-motion frame.
    pub fn move_by(&mut self, dx: i16, dy: i16) -> &mut Self {
        self.frame(dx, dy, 0, &[])
    }

    /// A wheel frame.
    pub fn wheel(&mut self, dz: i16) -> &mut Self {
        self.frame(0, 0, dz, &[])
    }

    /// A frame carrying only edges (no motion).
    pub fn edges(&mut self, edges: &[ClipEdge]) -> &mut Self {
        self.frame(0, 0, 0, edges)
    }

    /// A frame that presses a button.
    pub fn press(&mut self, button: Button) -> &mut Self {
        self.edges(&[ClipEdge::button(button, Action::Press)])
    }

    /// A frame that soft-releases a button (clears the injected press; a physical hold is left intact).
    pub fn release(&mut self, button: Button) -> &mut Self {
        self.edges(&[ClipEdge::button(button, Action::SoftRelease)])
    }

    /// A frame that force-releases a button (masks a physical hold too).
    pub fn force_release(&mut self, button: Button) -> &mut Self {
        self.edges(&[ClipEdge::button(button, Action::ForceRelease)])
    }

    /// A frame carrying one key edge.
    pub fn key(&mut self, key: Key, action: Action) -> &mut Self {
        self.edges(&[ClipEdge::key(key, action)])
    }

    /// A frame carrying one media edge.
    pub fn media(&mut self, media: MediaKey, action: Action) -> &mut Self {
        self.edges(&[ClipEdge::media(media, action)])
    }

    /// The raw encoded entry stream.
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// The number of entries.
    pub fn len(&self) -> usize {
        self.ends.len()
    }

    /// Whether the builder holds no entries.
    pub fn is_empty(&self) -> bool {
        self.ends.is_empty()
    }

    /// Clear the stream to reuse the allocation.
    pub fn clear(&mut self) {
        self.bytes.clear();
        self.ends.clear();
    }

    /// Entry end offsets, for entry-boundary chunking on append.
    pub(crate) fn entry_ends(&self) -> &[usize] {
        &self.ends
    }
}
