//! Buffered clip playback (§3.11 / §4.15): the per-frame entry stream a host preloads into the device-side
//! ring, and the ring/playback status. The box drains one entry per native frame, routing each edge to its
//! class (mouse, keyboard, media), and emits it through the normal engine (rate pacing, movement riding).

use crate::protocol::opcode::{
    CLIP_F_EDGES, CLIP_F_WHEEL, CLIP_F_XY, CLIP_TAG_GAP, INJ_BTN, INJ_KEY, INJ_MEDIA,
};
use crate::types::lock::blanket_scope;
use crate::types::{Action, Blanket, Button, Input, Key, MediaKey};

/// Playback options for a clip [`start`](crate::ClipHandle::start) or catch trigger
/// ([`arm_catch`](crate::ClipHandle::arm_catch)). The single place clip settings live; extensible as more
/// are added. Build with the chained setters, e.g. `ClipConfig::new().autolock(Blanket::ALL)`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct ClipConfig {
    autolock: u8, // CLIP_LOCK_* scope bitmask (0 = none)
}

impl ClipConfig {
    /// Default options: no auto-lock.
    pub fn new() -> ClipConfig {
        ClipConfig::default()
    }

    /// Auto-lock these physical-input groups while the clip plays (clip-owned, released on stop). Pass
    /// [`Blanket::ALL`] for every class, or a subset like `&[Blanket::Aim, Blanket::Buttons]`.
    pub fn autolock(mut self, scope: &[Blanket]) -> ClipConfig {
        self.autolock = blanket_scope(scope);
        self
    }

    /// The auto-lock scope byte (`CLIP_LOCK_*` bits) this config carries.
    pub(crate) fn autolock_scope(&self) -> u8 {
        self.autolock
    }
}

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
    /// Free bytes in the ring, the headroom for the next [`append`](crate::ClipHandle::append).
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
    /// Held-input flags: bits 0-4 are the clip-held mouse buttons (bit `b` = button id `b`), bit 5 is set
    /// when the clip holds a key, bit 6 when it holds a media usage. Read it with
    /// [`buttons_held`](Self::buttons_held) / [`keys_held`](Self::keys_held) / [`media_held`](Self::media_held).
    pub held: u8,
}

/// `held` bit 5: the clip is holding a key.
const CLIP_HELD_KEYS: u8 = 0x20;
/// `held` bit 6: the clip is holding a media usage.
const CLIP_HELD_MEDIA: u8 = 0x40;

impl ClipStatus {
    /// Bitmask of clip-held mouse buttons (bit `b` = button id `b`).
    pub fn buttons_held(&self) -> u8 {
        self.held & 0x1F
    }
    /// Whether the clip is currently holding a keyboard key down.
    pub fn keys_held(&self) -> bool {
        self.held & CLIP_HELD_KEYS != 0
    }
    /// Whether the clip is currently holding a media usage down.
    pub fn media_held(&self) -> bool {
        self.held & CLIP_HELD_MEDIA != 0
    }
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
            held: p[20],
        })
    }
}

/// Map the field-generic [`Input`] to its INJECT wire class and id.
pub(crate) fn input_class_id(input: Input) -> (u8, u16) {
    match input {
        Input::Button(b) => (INJ_BTN, b.as_id() as u16),
        Input::Key(k) => (INJ_KEY, k.usage() as u16),
        Input::Media(m) => (INJ_MEDIA, m.usage()),
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
    pub fn frame(&mut self, dx: i16, dy: i16, wheel: i16, edges: &[(Input, Action)]) -> &mut Self {
        debug_assert!(
            edges.len() <= CLIP_EDGES_MAX,
            "clip frame: {} edges exceeds CLIP_EDGES_MAX ({CLIP_EDGES_MAX})",
            edges.len()
        );
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
            for &(input, action) in edges {
                let (class, id) = input_class_id(input);
                self.bytes.push(class);
                self.bytes.extend_from_slice(&id.to_le_bytes());
                self.bytes.push(action.as_u8());
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

    /// A frame carrying one edge (any input class).
    pub fn edge(&mut self, input: impl Into<Input>, action: Action) -> &mut Self {
        self.frame(0, 0, 0, &[(input.into(), action)])
    }

    /// A frame that presses a button.
    pub fn press(&mut self, button: Button) -> &mut Self {
        self.edge(button, Action::Press)
    }

    /// A frame that soft-releases a button (clears the injected press; a physical hold is left intact).
    pub fn release(&mut self, button: Button) -> &mut Self {
        self.edge(button, Action::SoftRelease)
    }

    /// A frame that force-releases a button (masks a physical hold too).
    pub fn force_release(&mut self, button: Button) -> &mut Self {
        self.edge(button, Action::ForceRelease)
    }

    /// A frame carrying one key edge.
    pub fn key(&mut self, key: Key, action: Action) -> &mut Self {
        self.edge(key, action)
    }

    /// A frame carrying one media edge.
    pub fn media(&mut self, media: MediaKey, action: Action) -> &mut Self {
        self.edge(media, action)
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
