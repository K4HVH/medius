//! Buffered clip playback (§3.11 / §4.15): the per-frame entry stream a host preloads into the device-side ring, the trigger/config surface, and the ring/playback status.

use crate::protocol::opcode::{
    CLIP_CFG_F_FINALIZED, CLIP_CFG_F_LOOP, CLIP_CFG_F_RETAIN, CLIP_CFG_F_RIDE, CLIP_F_EDGES,
    CLIP_F_WHEEL, CLIP_F_XY, CLIP_OP_PAUSE, CLIP_OP_RESTART, CLIP_OP_RESUME, CLIP_OP_START,
    CLIP_OP_STOP, CLIP_OP_TOGGLE, CLIP_TAG_GAP, CLIP_TRIG_MAX, LOCK_DIR_BOTH, LOCK_DIR_NEG,
    LOCK_DIR_POS,
};
use crate::types::lock::blanket_from_scope;
use crate::types::{Action, Blanket, Class, Direction, Usage};

/// Which edge of a trigger usage fires its [`ClipTrigger`]. The wire encoding matches [`Direction`]
/// (`Both`=0, `Press`=1, `Release`=2).
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Edge {
    /// Either edge.
    Both = LOCK_DIR_BOTH,
    /// The press edge (0 → 1).
    Press = LOCK_DIR_POS,
    /// The release edge (1 → 0).
    Release = LOCK_DIR_NEG,
}

impl Edge {
    pub(crate) fn as_u8(self) -> u8 {
        self as u8
    }
    pub(crate) fn from_u8(v: u8) -> Option<Edge> {
        Some(match v {
            LOCK_DIR_BOTH => Edge::Both,
            LOCK_DIR_POS => Edge::Press,
            LOCK_DIR_NEG => Edge::Release,
            _ => return None,
        })
    }
}

impl From<Edge> for Direction {
    fn from(e: Edge) -> Direction {
        match e {
            Edge::Both => Direction::Both,
            Edge::Press => Direction::Positive,
            Edge::Release => Direction::Negative,
        }
    }
}

/// The engine action a [`ClipTrigger`] drives (and a host [`ClipHandle`](crate::ClipHandle) verb).
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClipAction {
    /// Rewind and play (resume from a pause).
    Start = CLIP_OP_START,
    /// Stop, release held input and the clip auto-lock.
    Stop = CLIP_OP_STOP,
    /// Halt mid-clip, retaining the cursor and held input.
    Pause = CLIP_OP_PAUSE,
    /// Continue from the paused cursor.
    Resume = CLIP_OP_RESUME,
    /// Force a rewind and play, even mid-playback.
    Restart = CLIP_OP_RESTART,
    /// Play if idle/paused, stop if playing.
    Toggle = CLIP_OP_TOGGLE,
}

impl ClipAction {
    pub(crate) fn as_u8(self) -> u8 {
        self as u8
    }
    pub(crate) fn from_u8(v: u8) -> Option<ClipAction> {
        Some(match v {
            CLIP_OP_START => ClipAction::Start,
            CLIP_OP_STOP => ClipAction::Stop,
            CLIP_OP_PAUSE => ClipAction::Pause,
            CLIP_OP_RESUME => ClipAction::Resume,
            CLIP_OP_RESTART => ClipAction::Restart,
            CLIP_OP_TOGGLE => ClipAction::Toggle,
            _ => return None,
        })
    }
}

/// One clip trigger binding: the `edge` of a physical `on` usage drives `action`, optionally consuming
/// the input so it never reaches the game. Bindings are a managed set keyed by `(on, edge)`, like a lock.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ClipTrigger {
    /// The physical usage that fires this trigger (a button, key, or media usage).
    pub on: Usage,
    /// Which edge fires it.
    pub edge: Edge,
    /// The engine action it drives.
    pub action: ClipAction,
    /// Suppress the trigger input from the game while held.
    pub consume: bool,
}

impl ClipTrigger {
    /// A pass-through binding: `on`'s `edge` drives `action`, and the input still reaches the game.
    pub fn new(on: impl Into<Usage>, edge: Edge, action: ClipAction) -> ClipTrigger {
        ClipTrigger {
            on: on.into(),
            edge,
            action,
            consume: false,
        }
    }

    /// Consume the trigger input: suppress it from the game while the trigger usage is held.
    pub fn consume(mut self) -> ClipTrigger {
        self.consume = true;
        self
    }
}

/// The device-side clip lifecycle state ([`ClipStatus::state`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ClipState {
    /// No clip playing (empty, or a loaded clip parked at its start).
    #[default]
    Idle,
    /// Draining the ring, one entry per native frame.
    Playing,
    /// Halted mid-clip; the cursor and any held usages are retained ([`resume`](crate::ClipHandle::resume) to continue).
    Paused,
    /// An append was dropped or the ring overflowed; recover with [`clear`](crate::ClipHandle::clear).
    Faulted,
}

impl ClipState {
    pub(crate) fn from_u8(v: u8) -> Option<ClipState> {
        Some(match v {
            0 => ClipState::Idle,
            1 => ClipState::Playing,
            2 => ClipState::Paused,
            3 => ClipState::Faulted,
            _ => return None,
        })
    }
}

/// A snapshot of the device-side clip ring and playback counters (§4.15): the runtime view of `RESP(CLIP)`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct ClipStatus {
    /// The lifecycle state.
    pub state: ClipState,
    /// Free bytes in the ring, the headroom for the next [`append`](crate::ClipHandle::append).
    pub free: u32,
    /// The retained clip size in bytes (streaming: the buffered-but-undrained bytes).
    pub total: u32,
    /// Bytes played from the clip start (retained progress; ~0 while streaming).
    pub played: u32,
    /// Entries played since the last start.
    pub ticks: u32,
    /// Underrun episodes (the ring ran dry mid-playback, then idled or refilled).
    pub underruns: u16,
    /// Appends dropped because the ring was full.
    pub overruns: u16,
    /// Append-sequence gaps seen (a dropped `CLIP_APPEND` frame).
    pub seq_gaps: u16,
    /// The usages the clip is currently holding down: buttons, keys, and media in one list.
    pub held: Vec<Usage>,
}

impl ClipStatus {
    /// Whether the clip is currently holding `usage` (a button, key, or media usage) down.
    pub fn is_held(&self, usage: impl Into<Usage>) -> bool {
        let u = usage.into();
        self.held.contains(&u)
    }

    /// Decode the runtime view of a `RESP(CLIP)` payload (§4.15).
    pub(crate) fn from_payload(p: &[u8]) -> Option<ClipStatus> {
        if p.len() < 25 {
            return None;
        }
        let held = Usage::decode_list(&p[24..])?;
        Some(ClipStatus {
            state: ClipState::from_u8(p[1])?,
            free: u32::from_le_bytes([p[2], p[3], p[4], p[5]]),
            total: u32::from_le_bytes([p[6], p[7], p[8], p[9]]),
            played: u32::from_le_bytes([p[10], p[11], p[12], p[13]]),
            ticks: u32::from_le_bytes([p[14], p[15], p[16], p[17]]),
            underruns: u16::from_le_bytes([p[18], p[19]]),
            overruns: u16::from_le_bytes([p[20], p[21]]),
            seq_gaps: u16::from_le_bytes([p[22], p[23]]),
            held,
        })
    }
}

/// The clip configuration read back from `RESP(CLIP)` (§4.15): the autolock scope, the loop/retain
/// scalar settings, and the trigger binding set. The config view of the same frame [`ClipStatus`] reads.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct ClipSettings {
    /// The autolock groups the clip locks while playing ([`set_autolock`](crate::ClipHandle::set_autolock)).
    pub autolock: Vec<Blanket>,
    /// Whether playback loops at the clip end (retained mode only).
    pub loop_: bool,
    /// Whether the clip is retained/replayable (`false` = streaming, the default).
    pub retain: bool,
    /// Whether a retained clip has been finalized (its end fixed).
    pub finalized: bool,
    /// Whether the clip's motion waits to ride a native report (`false` = the box's own clock, the default).
    pub ride: bool,
    /// The trigger binding set.
    pub triggers: Vec<ClipTrigger>,
}

impl ClipSettings {
    // Decode the config view of a `RESP(CLIP)` payload: skip the runtime prefix + held list, then read
    // `[autolock][flags][n_trig]` and the trigger tuples. A wildcard binding (no concrete class) is skipped.
    pub(crate) fn from_payload(p: &[u8]) -> Option<ClipSettings> {
        if p.len() < 25 {
            return None;
        }
        let held_n = p[24] as usize;
        let mut off = 25 + held_n * 3;
        if p.len() < off + 3 {
            return None;
        }
        let autolock = blanket_from_scope(p[off]);
        let flags = p[off + 1];
        let n_trig = (p[off + 2] as usize).min(CLIP_TRIG_MAX);
        off += 3;
        let mut triggers = Vec::with_capacity(n_trig);
        for _ in 0..n_trig {
            if p.len() < off + 6 {
                return None;
            }
            if let (Some(class), Some(edge), Some(action)) = (
                Class::from_u8(p[off]),
                Edge::from_u8(p[off + 3]),
                ClipAction::from_u8(p[off + 4]),
            ) {
                let id = u16::from_le_bytes([p[off + 1], p[off + 2]]);
                triggers.push(ClipTrigger {
                    on: Usage::new(class, id),
                    edge,
                    action,
                    consume: p[off + 5] != 0,
                });
            }
            off += 6;
        }
        Some(ClipSettings {
            autolock,
            loop_: flags & CLIP_CFG_F_LOOP != 0,
            retain: flags & CLIP_CFG_F_RETAIN != 0,
            finalized: flags & CLIP_CFG_F_FINALIZED != 0,
            ride: flags & CLIP_CFG_F_RIDE != 0,
            triggers,
        })
    }
}

/// Max edges on one [`ClipBuilder::frame`], matching the firmware's `CLIP_EDGES_MAX`.
pub const CLIP_EDGES_MAX: usize = 8;

/// Builds a buffered-clip entry stream (§3.11) for [`ClipHandle::append`](crate::ClipHandle::append).
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

    /// A gap run: emit nothing for `frames` native frames (the endpoint NAKs like an idle mouse).
    pub fn gap(&mut self, frames: u16) -> &mut Self {
        if frames == 0 {
            return self;
        }
        self.bytes.push(CLIP_TAG_GAP);
        self.bytes.extend_from_slice(&frames.to_le_bytes());
        self.ends.push(self.bytes.len());
        self
    }

    /// A content frame: a relative motion delta (`dx`/`dy` cursor, `wheel`) plus a list of edges.
    pub fn frame(&mut self, dx: i16, dy: i16, wheel: i16, edges: &[(Usage, Action)]) -> &mut Self {
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
                let (class, id) = input.class_id();
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
    pub fn edge(&mut self, input: impl Into<Usage>, action: Action) -> &mut Self {
        self.frame(0, 0, 0, &[(input.into(), action)])
    }

    /// A frame that presses a usage (a button, key, or media usage), like [`Device::press`](crate::Device::press).
    pub fn press(&mut self, usage: impl Into<Usage>) -> &mut Self {
        self.edge(usage, Action::Press)
    }

    /// A frame that soft-releases a usage (clears the injected press; a physical hold is left intact).
    pub fn release(&mut self, usage: impl Into<Usage>) -> &mut Self {
        self.edge(usage, Action::SoftRelease)
    }

    /// A frame that force-releases a usage (masks a physical hold too).
    pub fn force_release(&mut self, usage: impl Into<Usage>) -> &mut Self {
        self.edge(usage, Action::ForceRelease)
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
