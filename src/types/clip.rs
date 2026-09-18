//! Buffered clip playback (§3.11 / §4.15): the per-frame entry stream a host preloads into the device-side ring, the trigger/config surface, and the ring/playback status.

use crate::protocol::opcode::{
    CLIP_CFG_F_FINALIZED, CLIP_CFG_F_LOOP, CLIP_CFG_F_RETAIN, CLIP_CFG_F_RIDE, CLIP_OP_PAUSE,
    CLIP_OP_RESTART, CLIP_OP_RESUME, CLIP_OP_START, CLIP_OP_STOP, CLIP_OP_TOGGLE, CLIP_TRIG_MAX,
    LOCK_DIR_BOTH, LOCK_DIR_NEG, LOCK_DIR_POS, MAX_PAYLOAD, RESP_CLIP_HDR,
};
use crate::types::lock::blanket_from_scope;
use crate::types::{Action, Blanket, Class, Direction, Setup, Usage};

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
    /// Content frames played since the box booted.
    pub ticks: u32,
    /// Underrun episodes (the ring ran dry mid-playback, then idled or refilled).
    pub underruns: u16,
    /// Appends dropped because the ring was full.
    pub overruns: u16,
    /// Append-sequence gaps seen (a dropped `CLIP_APPEND` frame).
    pub seq_gaps: u16,
    /// Clip transfers the device completed.
    pub xfers: u16,
    /// Clip transfers that ended any other way: a refusal, no answer, no room in the box's queue, or
    /// dropped behind one the device did not answer.
    pub xfer_errs: u16,
    /// Raw reports and transfers the box discarded because the imperfect-clone opt-in was off.
    pub gated: u16,
    /// The usages the clip is currently holding down: buttons, keys, and media in one list.
    pub held: Vec<Usage>,
}

// The offset of the config tail in a `RESP(CLIP)` payload, when the payload is exactly the shape its
// own counts describe: the scalar prefix, `held_n` held usages, `[autolock][flags][n_trig]`, and
// `n_trig` trigger tuples. A payload of any other length is not a `RESP(CLIP)` this crate can read.
fn clip_config_offset(p: &[u8]) -> Option<usize> {
    if p.len() < RESP_CLIP_HDR {
        return None;
    }
    let cfg = RESP_CLIP_HDR + p[RESP_CLIP_HDR - 1] as usize * 3;
    let n_trig = *p.get(cfg + 2)? as usize;
    (n_trig <= CLIP_TRIG_MAX && p.len() == cfg + 3 + n_trig * 6).then_some(cfg)
}

impl ClipStatus {
    /// Whether the clip is currently holding `usage` (a button, key, or media usage) down.
    pub fn is_held(&self, usage: impl Into<Usage>) -> bool {
        let u = usage.into();
        self.held.contains(&u)
    }

    /// Decode the runtime view of a `RESP(CLIP)` payload (§4.15).
    pub(crate) fn from_payload(p: &[u8]) -> Option<ClipStatus> {
        clip_config_offset(p)?;
        let held = Usage::decode_list(&p[RESP_CLIP_HDR - 1..])?;
        Some(ClipStatus {
            state: ClipState::from_u8(p[1])?,
            free: u32::from_le_bytes([p[2], p[3], p[4], p[5]]),
            total: u32::from_le_bytes([p[6], p[7], p[8], p[9]]),
            played: u32::from_le_bytes([p[10], p[11], p[12], p[13]]),
            ticks: u32::from_le_bytes([p[14], p[15], p[16], p[17]]),
            underruns: u16::from_le_bytes([p[18], p[19]]),
            overruns: u16::from_le_bytes([p[20], p[21]]),
            seq_gaps: u16::from_le_bytes([p[22], p[23]]),
            xfers: u16::from_le_bytes([p[24], p[25]]),
            xfer_errs: u16::from_le_bytes([p[26], p[27]]),
            gated: u16::from_le_bytes([p[28], p[29]]),
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
        let mut off = clip_config_offset(p)?;
        let autolock = blanket_from_scope(p[off]);
        let flags = p[off + 1];
        let n_trig = p[off + 2] as usize;
        off += 3;
        let mut triggers = Vec::with_capacity(n_trig);
        for _ in 0..n_trig {
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

/// Max edges on one [`ClipFrame`], matching the firmware's `CLIP_EDGES_MAX`.
pub const CLIP_EDGES_MAX: usize = 8;
/// Max raw reports on one [`ClipFrame`], matching the firmware's `CLIP_RAW_MAX`.
pub const CLIP_RAW_MAX: usize = 8;
/// The most bytes one [`ClipFrame`] may encode to: one `CLIP_APPEND` payload.
pub const CLIP_ENTRY_MAX: usize = MAX_PAYLOAD;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RawItem {
    pub(crate) ep: u8,
    pub(crate) direction: Direction,
    pub(crate) bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TransferItem {
    pub(crate) ep: u8,
    pub(crate) setup: Setup,
    pub(crate) out: Vec<u8>,
}

/// One frame of a clip: everything the box does on one tick (§3.11).
///
/// A frame carries any mix of cursor motion, wheel, pan, up to [`CLIP_EDGES_MAX`] edges, up to
/// [`CLIP_RAW_MAX`] raw reports and any number of control transfers, as long as it encodes to at most
/// [`CLIP_ENTRY_MAX`] bytes. [`append`](crate::ClipHandle::append) checks that.
///
/// The raw reports and transfers need the imperfect-clone opt-in when the frame plays, not when it is
/// appended. With it off the box discards them and counts each in [`ClipStatus::gated`].
///
/// ```no_run
/// # use medius::{Button, ClipBuilder, ClipFrame, Device, Direction, Result, Setup};
/// # fn main() -> Result<()> {
/// let device = Device::find()?;
/// let mut clip = ClipBuilder::new();
/// clip.frame(
///     ClipFrame::new()
///         .move_by(4, -2)
///         .press(Button::LEFT)
///         .raw(2, Direction::OUT, [0x10, 0xFF, 0x05])
///         .transfer(0, Setup::new(0x21, 0x09, 0x0300, 0, 2), [0x04, 0x01]),
/// );
/// device.clip().append(&clip)?;
/// # Ok(()) }
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClipFrame {
    pub(crate) dx: i16,
    pub(crate) dy: i16,
    pub(crate) wheel: i16,
    pub(crate) pan: i16,
    pub(crate) edges: Vec<(Usage, Action)>,
    pub(crate) raw: Vec<RawItem>,
    pub(crate) transfers: Vec<TransferItem>,
}

impl ClipFrame {
    /// A frame that carries nothing yet.
    pub fn new() -> ClipFrame {
        ClipFrame::default()
    }

    /// Cursor motion, as a relative delta.
    pub fn move_by(mut self, dx: i16, dy: i16) -> ClipFrame {
        self.dx = dx;
        self.dy = dy;
        self
    }

    /// Wheel motion.
    pub fn wheel(mut self, dz: i16) -> ClipFrame {
        self.wheel = dz;
        self
    }

    /// Pan (horizontal scroll) motion.
    pub fn pan(mut self, dpan: i16) -> ClipFrame {
        self.pan = dpan;
        self
    }

    /// An edge on a button, key or media usage.
    pub fn edge(mut self, usage: impl Into<Usage>, action: Action) -> ClipFrame {
        self.edges.push((usage.into(), action));
        self
    }

    /// Press a usage, like [`Device::press`](crate::Device::press).
    pub fn press(self, usage: impl Into<Usage>) -> ClipFrame {
        self.edge(usage, Action::Press)
    }

    /// Soft-release a usage: clear the injected press and leave a physical hold intact.
    pub fn release(self, usage: impl Into<Usage>) -> ClipFrame {
        self.edge(usage, Action::SoftRelease)
    }

    /// Force-release a usage: mask a physical hold too.
    pub fn force_release(self, usage: impl Into<Usage>) -> ClipFrame {
        self.edge(usage, Action::ForceRelease)
    }

    /// A raw report, as [`Device::raw`](crate::Device::raw) sends one: `bytes` verbatim on endpoint
    /// number `ep`, [`Direction::IN`] toward the game PC or [`Direction::OUT`] to the real device.
    pub fn raw(mut self, ep: u8, direction: Direction, bytes: impl Into<Vec<u8>>) -> ClipFrame {
        self.raw.push(RawItem {
            ep,
            direction,
            bytes: bytes.into(),
        });
        self
    }

    /// A control transfer against the real device, as [`Device::transfer`](crate::Device::transfer)
    /// runs one. `out` is exactly `setup.length` bytes for an OUT request and empty for an IN one. The
    /// answer comes back as a [`TrafficClass::ClipTransfer`](crate::TrafficClass::ClipTransfer) event.
    pub fn transfer(mut self, ep: u8, setup: Setup, out: impl Into<Vec<u8>>) -> ClipFrame {
        self.transfers.push(TransferItem {
            ep,
            setup,
            out: out.into(),
        });
        self
    }

    /// The bytes this frame takes in the ring, at most [`CLIP_ENTRY_MAX`] for a frame
    /// [`append`](crate::ClipHandle::append) accepts.
    pub fn byte_len(&self) -> usize {
        let motion = (self.wheel != 0) as usize * 2 + (self.pan != 0) as usize * 2;
        let items = !self.edges.is_empty() as usize * (1 + self.edges.len() * 4)
            + self.raw.iter().map(|r| 4 + r.bytes.len()).sum::<usize>()
            + !self.raw.is_empty() as usize
            + self
                .transfers
                .iter()
                .map(|t| 9 + t.out.len())
                .sum::<usize>()
            + !self.transfers.is_empty() as usize;
        // Cursor motion, or a frame that carries nothing else, is an XY field.
        let xy = (self.dx != 0 || self.dy != 0 || motion + items == 0) as usize * 4;
        1 + xy + motion + items
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ClipEntry {
    Gap(u16),
    Frame(ClipFrame),
}

/// Builds a buffered-clip entry stream (§3.11) for [`ClipHandle::append`](crate::ClipHandle::append).
/// Every method appends one entry: a gap run, or one [`ClipFrame`].
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ClipBuilder {
    pub(crate) entries: Vec<ClipEntry>,
}

impl ClipBuilder {
    /// A new empty builder.
    pub fn new() -> ClipBuilder {
        ClipBuilder::default()
    }

    /// A gap run: emit nothing for `frames` native frames (the endpoint NAKs like an idle mouse).
    pub fn gap(&mut self, frames: u16) -> &mut Self {
        if frames != 0 {
            self.entries.push(ClipEntry::Gap(frames));
        }
        self
    }

    /// One frame carrying whatever `frame` holds.
    pub fn frame(&mut self, frame: ClipFrame) -> &mut Self {
        self.entries.push(ClipEntry::Frame(frame));
        self
    }

    /// A cursor-motion frame.
    pub fn move_by(&mut self, dx: i16, dy: i16) -> &mut Self {
        self.frame(ClipFrame::new().move_by(dx, dy))
    }

    /// A wheel frame.
    pub fn wheel(&mut self, dz: i16) -> &mut Self {
        self.frame(ClipFrame::new().wheel(dz))
    }

    /// A pan (horizontal scroll) frame.
    pub fn pan(&mut self, dpan: i16) -> &mut Self {
        self.frame(ClipFrame::new().pan(dpan))
    }

    /// A frame carrying one edge (any input class).
    pub fn edge(&mut self, input: impl Into<Usage>, action: Action) -> &mut Self {
        self.frame(ClipFrame::new().edge(input, action))
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

    /// A frame carrying one raw report (see [`ClipFrame::raw`]).
    pub fn raw(&mut self, ep: u8, direction: Direction, bytes: impl Into<Vec<u8>>) -> &mut Self {
        self.frame(ClipFrame::new().raw(ep, direction, bytes))
    }

    /// A frame carrying one control transfer (see [`ClipFrame::transfer`]).
    pub fn transfer(&mut self, ep: u8, setup: Setup, out: impl Into<Vec<u8>>) -> &mut Self {
        self.frame(ClipFrame::new().transfer(ep, setup, out))
    }

    /// The number of entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the builder holds no entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The bytes the entries take in the ring: what to hold against [`ClipStatus::free`] before an
    /// [`append`](crate::ClipHandle::append).
    pub fn byte_len(&self) -> usize {
        self.entries
            .iter()
            .map(|e| match e {
                ClipEntry::Gap(_) => 3,
                ClipEntry::Frame(f) => f.byte_len(),
            })
            .sum()
    }

    /// Clear the entries to reuse the allocation.
    pub fn clear(&mut self) {
        self.entries.clear();
    }
}
