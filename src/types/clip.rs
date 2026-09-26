//! Buffered clip playback (§3.11 / §4.15): the per-frame entries a host preloads into the box's
//! ring, triggers and config, and ring/playback status.
//!
//! A firing trigger runs its clip verb on the box, with no host round trip. A [`ClipTrigger`] fires
//! on a button, key or media edge; a [`ClipPacketTrigger`] on a packet crossing a traffic surface.

use crate::protocol::opcode::{
    CATCH_ID_ANY, CLIP_CFG_F_FINALIZED, CLIP_CFG_F_LOOP, CLIP_CFG_F_RETAIN, CLIP_CFG_F_RIDE,
    CLIP_OP_PAUSE, CLIP_OP_RESTART, CLIP_OP_RESUME, CLIP_OP_START, CLIP_OP_STOP, CLIP_OP_TOGGLE,
    CLIP_PKT_TRIG_ENTRY, CLIP_PKT_TRIG_MAX, CLIP_TRIG_F_CONSUME, CLIP_TRIG_F_RUN, CLIP_TRIG_MAX,
    LOCK_DIR_BOTH, LOCK_DIR_NEG, LOCK_DIR_POS, MAX_PAYLOAD, PKT_MATCH_MAX, RESP_CLIP_HDR,
};
use crate::types::lock::blanket_from_scope;
use crate::types::{Action, Blanket, CatchClass, Class, Direction, Setup, TrafficClass, Usage};

/// Edge of a trigger usage that fires its [`ClipTrigger`]. Encoded as [`Direction`] (`Both`=0,
/// `Press`=1, `Release`=2).
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Edge {
    /// Either edge.
    Both = LOCK_DIR_BOTH,
    /// Press edge (0 → 1).
    Press = LOCK_DIR_POS,
    /// Release edge (1 → 0).
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

/// Engine action a [`ClipTrigger`] or [`ClipPacketTrigger`] drives; also a
/// [`ClipHandle`](crate::ClipHandle) verb.
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

/// Input trigger: the `edge` of a physical `on` usage drives `action`, optionally consuming the input
/// so it never reaches the game. A managed set keyed by `(on, edge)`, like a lock.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ClipTrigger {
    /// Physical button, key or media usage that fires it.
    pub on: Usage,
    /// Edge that fires it.
    pub edge: Edge,
    /// Engine action the trigger drives.
    pub action: ClipAction,
    /// Suppress the trigger input from the game while held.
    pub consume: bool,
}

impl ClipTrigger {
    /// Binding where `on`'s `edge` drives `action` and the input still reaches the game.
    pub fn new(on: impl Into<Usage>, edge: Edge, action: ClipAction) -> ClipTrigger {
        ClipTrigger {
            on: on.into(),
            edge,
            action,
            consume: false,
        }
    }

    /// Suppress the trigger input from the game while it is held.
    pub fn consume(mut self) -> ClipTrigger {
        self.consume = true;
        self
    }
}

/// Packet trigger: a packet on a traffic surface whose head matches under a mask drives `action` on
/// the box's next tick.
///
/// Addressed like a traffic [`CatchFilter`](crate::CatchFilter) or
/// [`RewriteRule`](crate::RewriteRule): a `class`, an `id` within it ([`ANY_ID`](Self::ANY_ID) for
/// every id) and a `direction` ([`Both`](Direction::Both) for either flow). A packet matches when
/// `head[i] & mask[i] == match_bytes[i]` for every match byte; an empty match takes every packet on
/// the address. For [`Control`](TrafficClass::Control) the head is the 8 setup bytes, then the first 8
/// bytes of OUT data. An [`Emit`](TrafficClass::Emit) trigger sees the clip's own frames besides
/// native and injected ones, and none of the clip's raw reports.
///
/// [`bind_packet`](crate::ClipHandle::bind_packet) and the box refuse a trigger no packet can match:
/// a match bit outside its mask (a packet byte is masked before the compare), or a direction the
/// class never carries. [`HidIn`](TrafficClass::HidIn) and [`Emit`](TrafficClass::Emit) flow
/// [`IN`](Direction::IN), [`HidOut`](TrafficClass::HidOut) flows [`OUT`](Direction::OUT); every class
/// takes [`Both`](Direction::Both).
///
/// The box matches triggers against the packet as it arrived, ahead of the rewrite table, and
/// independently of it: one packet can fire a trigger and have a [`RewriteRule`](crate::RewriteRule)
/// act on it. Of the triggers a packet matches, only the most specific acts: an exact `id` ranks above
/// [`ANY_ID`](Self::ANY_ID), more masked bits above fewer, [`IN`](Direction::IN) or
/// [`OUT`](Direction::OUT) above [`Both`](Direction::Both), then the earlier bind.
///
/// A managed set keyed by `(class, id, direction, match_bytes, mask)`: binding a held key overwrites
/// it. The box holds [`CLIP_PKT_TRIG_MAX`](crate::CLIP_PKT_TRIG_MAX) of them, sharing
/// [`CLIP_PKT_MATCH_POOL`](crate::CLIP_PKT_MATCH_POOL) match bytes. They are clip config, cleared with
/// the rest of it on control-PC silence, [`reset`](crate::Device::reset), a detach, a link loss and a
/// re-clone.
///
/// ```no_run
/// # use medius::{ClipAction, ClipPacketTrigger, Device, Direction, Result, TrafficClass};
/// # fn main() -> Result<()> {
/// let device = Device::find()?;
/// let clip = device.clip();
/// // Report ID 7 on interface 2 carries a button in bit 5 of its second byte. Hold it to play.
/// let held = ClipPacketTrigger::new(TrafficClass::HidIn, 2, Direction::IN, ClipAction::Start)
///     .matching([0x07, 0x20], [0xFF, 0x20])
///     .once_per_run(1);
/// let let_go = ClipPacketTrigger::new(TrafficClass::HidIn, 2, Direction::IN, ClipAction::Stop)
///     .matching([0x07, 0x00], [0xFF, 0x20])
///     .once_per_run(1);
/// clip.bind_packet(&held)?;
/// // A run has no priming: where the device repeats its state every poll, `let_go` bound with the
/// // button up drives Stop on the next report. Bind it with the button held to skip that Stop, or
/// // accept it: with no clip playing it stops nothing.
/// clip.bind_packet(&let_go)?;
/// # Ok(()) }
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ClipPacketTrigger {
    /// Traffic surface the packet crosses: any class but [`Bus`](TrafficClass::Bus) and
    /// [`ClipTransfer`](TrafficClass::ClipTransfer).
    pub class: TrafficClass,
    /// Interface number for [`HidIn`](TrafficClass::HidIn), endpoint number for the rest, or
    /// [`ANY_ID`](Self::ANY_ID).
    pub id: u16,
    /// Packet flow: [`Both`](Direction::Both), or whichever of [`IN`](Direction::IN) and
    /// [`OUT`](Direction::OUT) the class carries.
    pub direction: Direction,
    /// Engine action the trigger drives.
    pub action: ClipAction,
    /// Head bytes compared under [`mask`](Self::mask), every set bit inside it; empty matches every
    /// packet.
    pub match_bytes: Vec<u8>,
    /// Mask over [`match_bytes`](Self::match_bytes); same length.
    pub mask: Vec<u8>,
    /// Drop every packet the trigger matches as the top-ranked trigger, before the rewrite table
    /// sees it.
    pub consume: bool,
    /// Drive the action on the first packet of a matching run instead of on each.
    pub once_per_run: bool,
    /// Leading match bytes that select the run's stream within the address.
    pub selector_len: u8,
}

impl ClipPacketTrigger {
    /// `id` addressing every interface or endpoint of the class.
    pub const ANY_ID: u16 = CATCH_ID_ANY;

    /// Trigger on every packet of the address; packets pass untouched. Narrow it with
    /// [`matching`](Self::matching).
    pub fn new(
        class: TrafficClass,
        id: u16,
        direction: Direction,
        action: ClipAction,
    ) -> ClipPacketTrigger {
        ClipPacketTrigger {
            class,
            id,
            direction,
            action,
            match_bytes: Vec::new(),
            mask: Vec::new(),
            consume: false,
            once_per_run: false,
            selector_len: 0,
        }
    }

    /// Narrow to packets whose head equals `match_bytes` under `mask`. Both are one length, at most
    /// [`PKT_MATCH_MAX`](crate::PKT_MATCH_MAX) bytes, and every set bit of `match_bytes` is set in
    /// `mask`; a longer packet matches on its head. Both go to the box as given and form the
    /// trigger's key.
    pub fn matching(mut self, match_bytes: impl Into<Vec<u8>>, mask: impl Into<Vec<u8>>) -> Self {
        self.match_bytes = match_bytes.into();
        self.mask = mask.into();
        self
    }

    /// Drop every packet the trigger matches as the top-ranked trigger, whether or not the action
    /// runs on it. [`Control`](TrafficClass::Control) takes none.
    ///
    /// A consumed [`HidIn`](TrafficClass::HidIn) report is dropped whole, motion and buttons
    /// included; a release edge in it reaches the PC with the next report. A catch subscription still
    /// sees consumed `HidIn` and OUT packets, but not consumed vendor IN or
    /// [`Emit`](TrafficClass::Emit) packets.
    ///
    /// Dropping traffic alters the wire, so the box holds a consuming trigger only under
    /// [`allow_imperfect_clones(true)`](crate::Device::allow_imperfect_clones). With the opt-in off
    /// the box refuses the bind, unseen by [`bind_packet`](crate::ClipHandle::bind_packet) (its docs
    /// say how to confirm one). Turning the opt-in off removes every consuming trigger the box holds.
    pub fn consume(mut self) -> Self {
        self.consume = true;
        self
    }

    /// Drive the action on the first packet of a matching run, so a device repeating a held state
    /// every poll fires once per hold. The release is a second trigger matching the released bytes.
    ///
    /// A run is over one stream, so the trigger names a report class (any but
    /// [`Control`](TrafficClass::Control)), a concrete `id`, and [`IN`](Direction::IN) or
    /// [`OUT`](Direction::OUT). The first `selector_len` match bytes select the stream within that
    /// address (a report ID) and the rest are the condition, so `selector_len` is below the match
    /// length and the mask past it has at least one bit set: a condition every packet of the stream
    /// meets is a run that never ends. A packet failing the selector leaves the run unchanged.
    ///
    /// A run has no priming: a trigger whose condition holds when bound drives its action on the
    /// next packet of its stream, so a release trigger bound with the button up fires at once on a
    /// device that repeats its state every poll. Bind the press trigger first and the release trigger
    /// while the button is held, or accept the one action. An identical re-bind keeps the run; an
    /// overwrite, a bus reset and a configuration change restart it.
    pub fn once_per_run(mut self, selector_len: u8) -> Self {
        self.once_per_run = true;
        self.selector_len = selector_len;
        self
    }

    pub(crate) fn is_surface(class: TrafficClass) -> bool {
        !matches!(class, TrafficClass::Bus | TrafficClass::ClipTransfer)
    }

    pub(crate) fn class_carries(class: TrafficClass, direction: Direction) -> bool {
        match class {
            TrafficClass::HidIn | TrafficClass::Emit => direction == Direction::IN,
            TrafficClass::HidOut => direction == Direction::OUT,
            TrafficClass::VendorInterrupt | TrafficClass::VendorBulk | TrafficClass::Control => {
                matches!(direction, Direction::IN | Direction::OUT)
            }
            TrafficClass::Bus | TrafficClass::ClipTransfer => false,
        }
    }

    pub(crate) fn flags(&self) -> u8 {
        (if self.consume { CLIP_TRIG_F_CONSUME } else { 0 })
            | (if self.once_per_run {
                CLIP_TRIG_F_RUN
            } else {
                0
            })
    }
}

/// Packet trigger the box holds, read back from `RESP(CLIP)`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ClipPacketTriggerEntry {
    /// The trigger in [`bind_packet`](crate::ClipHandle::bind_packet)'s shape, so it replays as a
    /// bind.
    pub trigger: ClipPacketTrigger,
    /// Packets matched as the top-ranked trigger since the bind or overwrite (saturating). A
    /// [`once_per_run`](ClipPacketTrigger::once_per_run) trigger counts every packet of a run and
    /// drives its action on the first.
    pub hits: u16,
}

impl ClipPacketTriggerEntry {
    // `[class][id u16][dir][action][flags][slen][mlen][hits u16][match][mask]`; `None` for a class,
    // direction or action this crate has no name for.
    fn from_wire(e: &[u8]) -> Option<ClipPacketTriggerEntry> {
        let class = TrafficClass::try_from(CatchClass::from_u8(e[0])?).ok()?;
        let direction = Direction::from_u8(e[3]).filter(|d| !d.is_relative())?;
        let action = ClipAction::from_u8(e[4])?;
        if !ClipPacketTrigger::is_surface(class) {
            return None;
        }
        let mlen = e[7] as usize;
        let body = &e[CLIP_PKT_TRIG_ENTRY..];
        Some(ClipPacketTriggerEntry {
            trigger: ClipPacketTrigger {
                class,
                id: u16::from_le_bytes([e[1], e[2]]),
                direction,
                action,
                match_bytes: body[..mlen].to_vec(),
                mask: body[mlen..].to_vec(),
                consume: e[5] & CLIP_TRIG_F_CONSUME != 0,
                once_per_run: e[5] & CLIP_TRIG_F_RUN != 0,
                selector_len: e[6],
            },
            hits: u16::from_le_bytes([e[8], e[9]]),
        })
    }
}

/// Clip lifecycle state on the box ([`ClipStatus::state`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ClipState {
    /// No clip playing (empty, or a loaded clip parked at its start).
    #[default]
    Idle,
    /// Draining the ring, one entry per native frame.
    Playing,
    /// Halted mid-clip, keeping the cursor and held usages; [`resume`](crate::ClipHandle::resume)
    /// continues.
    Paused,
    /// An append was dropped or the ring overflowed; [`clear`](crate::ClipHandle::clear) recovers.
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

/// Clip ring and playback counters (§4.15): the runtime view of `RESP(CLIP)`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct ClipStatus {
    /// Lifecycle state.
    pub state: ClipState,
    /// Free ring bytes, the headroom for the next [`append`](crate::ClipHandle::append).
    pub free: u32,
    /// Retained clip size in bytes (streaming: buffered, undrained bytes).
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
    /// Clip transfers that ended otherwise: refused, unanswered, no room in the box's queue, or
    /// dropped behind one the device did not answer.
    pub xfer_errs: u16,
    /// Raw reports and transfers discarded with the imperfect-clone opt-in off.
    pub gated: u16,
    /// Buttons, keys and media the clip holds down, in one list.
    pub held: Vec<Usage>,
}

// Config tail offset, when the payload is exactly the shape its counts describe: scalar prefix,
// `held_n` usages, `[autolock][flags][n_trig]`, `n_trig` input tuples, `[n_pkt]`, and `n_pkt` packet
// entries each as long as its `mlen` says. Any other length is unreadable.
fn clip_config_offset(p: &[u8]) -> Option<usize> {
    if p.len() < RESP_CLIP_HDR {
        return None;
    }
    let cfg = RESP_CLIP_HDR + p[RESP_CLIP_HDR - 1] as usize * 3;
    let n_trig = *p.get(cfg + 2)? as usize;
    if n_trig > CLIP_TRIG_MAX {
        return None;
    }
    let mut end = cfg + 3 + n_trig * 6;
    let n_pkt = *p.get(end)? as usize;
    if n_pkt > CLIP_PKT_TRIG_MAX {
        return None;
    }
    end += 1;
    for _ in 0..n_pkt {
        let mlen = *p.get(end + 7)? as usize;
        if mlen > PKT_MATCH_MAX {
            return None;
        }
        end += CLIP_PKT_TRIG_ENTRY + 2 * mlen;
    }
    (p.len() == end).then_some(cfg)
}

impl ClipStatus {
    /// Whether the clip holds `usage` (a button, key or media usage) down.
    pub fn is_held(&self, usage: impl Into<Usage>) -> bool {
        let u = usage.into();
        self.held.contains(&u)
    }

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

/// Clip config read back from `RESP(CLIP)` (§4.15): autolock scope, loop/retain settings and both
/// trigger kinds. The config view of the frame [`ClipStatus`] reads.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct ClipSettings {
    /// Groups the clip locks while playing ([`set_autolock`](crate::ClipHandle::set_autolock)).
    pub autolock: Vec<Blanket>,
    /// Whether playback loops at the clip end (retained mode only).
    pub loop_: bool,
    /// Whether the clip is retained and replayable (`false` = streaming, the default).
    pub retain: bool,
    /// Whether a retained clip is finalized (its end fixed).
    pub finalized: bool,
    /// Whether clip motion waits to ride a native report (`false` = the box's own clock, the
    /// default).
    pub ride: bool,
    /// Input triggers.
    pub triggers: Vec<ClipTrigger>,
    /// Packet triggers in the box's order, each with its hit count.
    pub packet_triggers: Vec<ClipPacketTriggerEntry>,
}

impl ClipSettings {
    // Skips a wildcard binding (no concrete class) and a packet trigger this crate has no names for.
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
        let n_pkt = p[off] as usize;
        off += 1;
        let mut packet_triggers = Vec::with_capacity(n_pkt);
        for _ in 0..n_pkt {
            let end = off + CLIP_PKT_TRIG_ENTRY + 2 * p[off + 7] as usize;
            packet_triggers.extend(ClipPacketTriggerEntry::from_wire(&p[off..end]));
            off = end;
        }
        Some(ClipSettings {
            autolock,
            loop_: flags & CLIP_CFG_F_LOOP != 0,
            retain: flags & CLIP_CFG_F_RETAIN != 0,
            finalized: flags & CLIP_CFG_F_FINALIZED != 0,
            ride: flags & CLIP_CFG_F_RIDE != 0,
            triggers,
            packet_triggers,
        })
    }
}

/// Max edges on one [`ClipFrame`], matching the firmware's `CLIP_EDGES_MAX`.
pub const CLIP_EDGES_MAX: usize = 8;
/// Max raw reports on one [`ClipFrame`], matching the firmware's `CLIP_RAW_MAX`.
pub const CLIP_RAW_MAX: usize = 8;
/// Max encoded bytes of one [`ClipFrame`]: one `CLIP_APPEND` payload.
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

/// Clip frame: everything the box does on one tick (§3.11).
///
/// Any mix of cursor motion, wheel, pan, up to [`CLIP_EDGES_MAX`] edges, up to [`CLIP_RAW_MAX`] raw
/// reports and any number of control transfers, encoding to at most [`CLIP_ENTRY_MAX`] bytes;
/// [`append`](crate::ClipHandle::append) checks that.
///
/// Raw reports and transfers need the imperfect-clone opt-in when the frame plays, not when it is
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
    /// Empty frame.
    pub fn new() -> ClipFrame {
        ClipFrame::default()
    }

    /// Relative cursor motion.
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

    /// Edge on a button, key or media usage.
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

    /// Raw report, as [`Device::raw`](crate::Device::raw) sends one: `bytes` verbatim on endpoint
    /// `ep`, [`Direction::IN`] to the game PC or [`Direction::OUT`] to the real device.
    pub fn raw(mut self, ep: u8, direction: Direction, bytes: impl Into<Vec<u8>>) -> ClipFrame {
        self.raw.push(RawItem {
            ep,
            direction,
            bytes: bytes.into(),
        });
        self
    }

    /// Control transfer to the real device, as [`Device::transfer`](crate::Device::transfer) runs
    /// one. `out` is exactly `setup.length` bytes for an OUT request, empty for an IN one. The reply
    /// arrives as a [`TrafficClass::ClipTransfer`](crate::TrafficClass::ClipTransfer) event.
    pub fn transfer(mut self, ep: u8, setup: Setup, out: impl Into<Vec<u8>>) -> ClipFrame {
        self.transfers.push(TransferItem {
            ep,
            setup,
            out: out.into(),
        });
        self
    }

    /// Ring bytes this frame takes; at most [`CLIP_ENTRY_MAX`] for a frame
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

/// Clip entry stream (§3.11) for [`ClipHandle::append`](crate::ClipHandle::append). Each method
/// appends one entry: a gap run or one [`ClipFrame`].
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ClipBuilder {
    pub(crate) entries: Vec<ClipEntry>,
}

impl ClipBuilder {
    /// Empty builder.
    pub fn new() -> ClipBuilder {
        ClipBuilder::default()
    }

    /// Gap run: emit nothing for `frames` native frames (the endpoint NAKs like an idle mouse).
    pub fn gap(&mut self, frames: u16) -> &mut Self {
        if frames != 0 {
            self.entries.push(ClipEntry::Gap(frames));
        }
        self
    }

    /// Appends `frame`.
    pub fn frame(&mut self, frame: ClipFrame) -> &mut Self {
        self.entries.push(ClipEntry::Frame(frame));
        self
    }

    /// Cursor-motion frame.
    pub fn move_by(&mut self, dx: i16, dy: i16) -> &mut Self {
        self.frame(ClipFrame::new().move_by(dx, dy))
    }

    /// Wheel frame.
    pub fn wheel(&mut self, dz: i16) -> &mut Self {
        self.frame(ClipFrame::new().wheel(dz))
    }

    /// Pan (horizontal scroll) frame.
    pub fn pan(&mut self, dpan: i16) -> &mut Self {
        self.frame(ClipFrame::new().pan(dpan))
    }

    /// Frame with one edge (any input class).
    pub fn edge(&mut self, input: impl Into<Usage>, action: Action) -> &mut Self {
        self.frame(ClipFrame::new().edge(input, action))
    }

    /// Frame pressing a button, key or media usage, like [`Device::press`](crate::Device::press).
    pub fn press(&mut self, usage: impl Into<Usage>) -> &mut Self {
        self.edge(usage, Action::Press)
    }

    /// Frame soft-releasing a usage (clears the injected press; a physical hold stays).
    pub fn release(&mut self, usage: impl Into<Usage>) -> &mut Self {
        self.edge(usage, Action::SoftRelease)
    }

    /// Frame force-releasing a usage (masks a physical hold too).
    pub fn force_release(&mut self, usage: impl Into<Usage>) -> &mut Self {
        self.edge(usage, Action::ForceRelease)
    }

    /// Frame with one raw report (see [`ClipFrame::raw`]).
    pub fn raw(&mut self, ep: u8, direction: Direction, bytes: impl Into<Vec<u8>>) -> &mut Self {
        self.frame(ClipFrame::new().raw(ep, direction, bytes))
    }

    /// Frame with one control transfer (see [`ClipFrame::transfer`]).
    pub fn transfer(&mut self, ep: u8, setup: Setup, out: impl Into<Vec<u8>>) -> &mut Self {
        self.frame(ClipFrame::new().transfer(ep, setup, out))
    }

    /// Number of entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the builder holds no entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Ring bytes the entries take, to check against [`ClipStatus::free`] before an
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
