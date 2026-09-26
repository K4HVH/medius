//! Scriptable fake box (feature = `mock`) for hardware-free testing.

use std::sync::Arc;

use parking_lot::Mutex;

use crate::protocol::opcode::{
    CAP_PAN, CAP_REPORT_ID, CAP_WHEEL, CAP_X, CAP_Y, CAPS_CD_KBD, CAPS_CD_MOUSE, DI_HAS_BOS,
    DI_HAS_SERIAL, KBC_CONSUMER, KBC_NKRO, KBC_REPORT_ID, KBC_SYSTEM, LOCK_AXIS_PAN, LOCK_CLS_AXIS,
    LOCK_CLS_BTN, LOCK_CLS_KEY, LOCK_CLS_MEDIA, LOCK_DIR_AGAINST, LOCK_DIR_BOTH, LOCK_DIR_NEG,
    LOCK_DIR_POS, LOCK_DIR_WITH, LOCK_ID_ALL, LOCK_SCALE_BLOCK, LOCK_SCALE_PASS, MAX_BUTTONS,
    OPT_BEARING, OPT_EMIT, OPT_IMPERFECT, OPT_MOVE_RIDE, OPT_NAME, OPT_RENDER, OPT_SPREAD,
    Q_FIRMWARE, RATE_CONFIDENT, RST_F_NVS,
};
use crate::protocol::opcode::{
    CATCH_CLS_ANY, CATCH_CLS_CONTROL, CATCH_CLS_EMIT, CATCH_CLS_HID_IN, CATCH_CLS_HID_OUT,
    CATCH_CLS_VEND_BULK, CATCH_CLS_VEND_INTR, CATCH_ID_ANY, RW_ANSWER, RW_DROP, RW_NAK, RW_PASS,
    RW_PATCH, RW_REPLACE, RW_REPLY_PATCH, RW_REPLY_REPLACE, RW_STALL,
};
use crate::protocol::opcode::{
    CATCH_CLS_AXIS, CATCH_CLS_BTN, CATCH_CLS_KEY, CATCH_CLS_MEDIA, Q_TRANSFORMS, TF_F_FULL,
    TF_OP_COUNT, TF_REMAP, TF_SWAP, TRANSFORM_MAX_ENTRIES,
};
use crate::protocol::opcode::{
    CLIP_CFG_F_FINALIZED, CLIP_CFG_F_LOOP, CLIP_CFG_F_RETAIN, CLIP_CFG_F_RIDE, CLIP_COND_ANY_CLASS,
    CLIP_COND_ANY_ID, CLIP_OP_CLEAR, CLIP_OP_TOGGLE, CLIP_PKT_MATCH_POOL, CLIP_PKT_TRIG_HDR,
    CLIP_PKT_TRIG_MAX, CLIP_TRIG_F_CONSUME, CLIP_TRIG_F_PRESENT, CLIP_TRIG_F_RUN, CLIP_TRIG_MAX,
    PKT_MATCH_MAX,
};
use crate::protocol::opcode::{
    CLK_RATE_NONE, PATCH_APPLY, PATCH_CLEAR, PATCH_MAX_ENTRIES, Q_PATCH_ENTRY, Q_PATCHES,
    Q_REWRITE, Q_REWRITE_ENTRY, REWRITE_MATCH_MAX, REWRITE_MAX_ENTRIES, REWRITE_PAYLOAD_POOL,
};
use crate::protocol::{DecodedFrame, FrameType, encode};
use crate::types::PatchSection;
use sha2::{Digest, Sha256};

use crate::transport::mock::MockTransport;
use crate::types::lock::blanket_scope;
use crate::types::{
    Axis, Bearing, BearingMode, Caps, CatchClass, CatchState, Class, ClipAction, ClipPacketTrigger,
    ClipSettings, ClipState, ClipStatus, ClockDomain, DeviceInfo, DeviceKind, Direction, EmitPace,
    Health, ImperfectStatus, KbdCaps, LockEntry, LockScope, LockTarget, Locks, LogLevel, MouseCaps,
    Rate, RebootTarget, RenderMode, Stats, TrafficClass, Usage, Version,
};

#[derive(Debug)]
struct State {
    update: MockUpdate,
    replied: Vec<Vec<u8>>,
    version: Version,
    health: Health,
    device_info: DeviceInfo,
    caps: Caps,
    rate: Rate,
    stats: Stats,
    // The table the LOCK frames build, and a pinned reply that wins over it when a test scripts one.
    table: LockTable,
    locks: Option<Locks>,
    catch: CatchState,
    imperfect: ImperfectStatus,
    move_ride_ms: u16,
    bearing: Bearing,
    emit_pace: EmitPace,
    render_mode: RenderMode,
    render_full: bool,
    // A real box arms this off native motion, so the mock starts unarmed.
    render_ready: bool,
    spread_percent: u16,
    // Learned off MOVE arrivals, in microseconds; 0 until enough arrive, as every session starts.
    spread_learned_us: u32,
    emit_force_hz: Option<u16>,
    advertised_hz: u16,
    clip: ClipStatus,
    // The scripted clip config. Its packet triggers live in `packet_triggers`, the one store both a
    // script and a CLIP_TRIGGER frame write.
    clip_settings: ClipSettings,
    packet_triggers: PacketTriggers,
    // The rewrite table the REWRITE frames build, modelled the way the box holds it (keyed rows, a
    // monotonic gen, a full flag) so the mock answers RESP(REWRITE)/RESP(REWRITE_ENTRY) like a box.
    rewrites: Vec<MockRewrite>,
    rewrite_gen: u8,
    rewrite_full: bool,
    // The patch store the PATCH frames build, and the set the clone was last presented with.
    // `applied` and `pending` are derived from the two at pack time.
    patches: Vec<MockPatch>,
    patch_presented: Vec<MockPatch>,
    patch_refused: bool,
    patch_full: bool,
    // The field-transform table the TRANSFORM frames build, modelled the way the box holds it
    // (keyed rows in installation order, a full flag).
    transforms: Vec<MockTransform>,
    transform_full: bool,
    // The canned answer to a TRANSFER (status, IN data). The box answers 0xFC when the opt-in is off.
    transfer_reply: (u8, Vec<u8>),
    recorded: Vec<DecodedFrame>,
    respond: bool,
    // Whether a frame has arrived since the device chip booted: the first one gets a hello first.
    pc_seen: bool,
    // session_ctr.h: a command other than a QUERY since the session counter last counted.
    session_dirty: bool,
    // Whether a clone is up: a device is attached and cloned.
    clone_up: bool,
    // What the box does later on its own, each run by the first frame at or past its time.
    clone_up_at: Option<std::time::Instant>,
    // A detach's grace: when it ends, whether the device came back inside it, and a presentation
    // asked for during it, which waits to see.
    grace_until: Option<std::time::Instant>,
    device_back: bool,
    represent_held: bool,
    represent_at: Option<std::time::Instant>,
    represent_delay: std::time::Duration,
    #[cfg(test)]
    teardown_before_command: bool,
    #[cfg(test)]
    link_lost_before_clip_query: bool,
    // Frames the box sends on its own, drained into the next reply or pushed by `MockBox::restart`.
    unsolicited: Vec<u8>,
}

// One rewrite-table row the mock holds, in wire fields plus a live hit counter.
#[derive(Debug, Clone, PartialEq, Eq)]
struct MockRewrite {
    class: u8,
    id: u16,
    dir: u8,
    action: u8,
    offset: u16,
    match_bytes: Vec<u8>,
    mask: Vec<u8>,
    payload: Vec<u8>,
    hits: u16,
}

impl MockRewrite {
    // The (class, id, dir, match, mask) key two rows collide on.
    fn key(&self) -> (u8, u16, u8, Vec<u8>, Vec<u8>) {
        (
            self.class,
            self.id,
            self.dir,
            self.match_bytes.clone(),
            self.mask.clone(),
        )
    }
}

// Wire fields plus the hit count and whether a RUN trigger's last stream packet met its condition.
#[derive(Debug, Clone, PartialEq, Eq)]
struct MockPacketTrigger {
    class: u8,
    id: u16,
    dir: u8,
    action: u8,
    flags: u8, // CLIP_TRIG_F_CONSUME | CLIP_TRIG_F_RUN
    slen: u8,
    match_bytes: Vec<u8>,
    mask: Vec<u8>,
    hits: u32,
    live: bool,
}

// pkt_head_eq in the firmware's pkt_match.h: the first `n` match bytes against the head, under the mask.
fn pkt_head_eq(match_bytes: &[u8], mask: &[u8], n: usize, head: &[u8]) -> bool {
    head.len() >= n && (0..n).all(|j| head[j] & mask[j] == match_bytes[j])
}

impl MockPacketTrigger {
    // pkt_match_score: how specific this trigger is for a packet, `None` when it is no candidate.
    fn score(&self, class: u8, id: u16, dir: u8, head: &[u8]) -> Option<u32> {
        let id_rank = match (self.class == class, self.id) {
            (true, i) if i == id => 2,
            (true, CATCH_ID_ANY) => 1,
            _ => return None,
        };
        if self.dir != LOCK_DIR_BOTH && dir != LOCK_DIR_BOTH && self.dir != dir {
            return None;
        }
        if !pkt_head_eq(&self.match_bytes, &self.mask, self.match_bytes.len(), head) {
            return None;
        }
        let bits: u32 = self.mask.iter().map(|m| m.count_ones()).sum();
        Some(id_rank * 100_000 + bits * 10 + (self.dir != LOCK_DIR_BOTH) as u32)
    }
}

// The clip trigger set's packet triggers, modelled on the firmware's clip_ptrig.h.
#[derive(Debug, Default)]
struct PacketTriggers {
    rows: Vec<MockPacketTrigger>,
}

impl PacketTriggers {
    fn match_used(&self) -> usize {
        self.rows.iter().map(|r| r.match_bytes.len()).sum()
    }

    // clip_ptrig_set: add, overwrite or remove one trigger, or refuse the frame whole. Returns the
    // index of the trigger it set or removed.
    #[allow(clippy::too_many_arguments)]
    fn set(
        &mut self,
        class: u8,
        id: u16,
        dir: u8,
        action: u8,
        flags: u8,
        slen: u8,
        match_bytes: &[u8],
        mask: &[u8],
        imperfect: bool,
    ) -> Option<usize> {
        let mlen = match_bytes.len();
        let surface = (CATCH_CLS_HID_IN..=CATCH_CLS_EMIT).contains(&class);
        if !surface || dir > LOCK_DIR_NEG || mlen > PKT_MATCH_MAX || mask.len() != mlen {
            return None;
        }
        // A trigger no packet can match, as a set or as the key of a removal: a direction the class
        // never carries, or a match bit outside the mask.
        let one_way_in = class == CATCH_CLS_HID_IN || class == CATCH_CLS_EMIT;
        if (dir == LOCK_DIR_NEG && one_way_in)
            || (dir == LOCK_DIR_POS && class == CATCH_CLS_HID_OUT)
        {
            return None;
        }
        if match_bytes.iter().zip(mask).any(|(m, k)| m & !k != 0) {
            return None;
        }
        let found = self.rows.iter().position(|r| {
            (r.class, r.id, r.dir) == (class, id, dir)
                && r.match_bytes == match_bytes
                && r.mask == mask
        });
        if flags & CLIP_TRIG_F_PRESENT == 0 {
            let i = found?;
            self.rows.remove(i);
            return Some(i);
        }
        if action > CLIP_OP_TOGGLE
            || flags & !(CLIP_TRIG_F_PRESENT | CLIP_TRIG_F_CONSUME | CLIP_TRIG_F_RUN) != 0
        {
            return None;
        }
        let ctl = class == CATCH_CLS_CONTROL;
        if flags & CLIP_TRIG_F_CONSUME != 0 && (ctl || !imperfect) {
            return None;
        }
        if flags & CLIP_TRIG_F_RUN != 0 {
            // The condition is the mask past the selector. With no byte there, or no masked bit in
            // them, every packet of the stream meets it.
            let condition = mask.get(slen as usize..).unwrap_or(&[]);
            if ctl
                || id == CATCH_ID_ANY
                || dir == LOCK_DIR_BOTH
                || condition.iter().all(|&k| k == 0)
            {
                return None;
            }
        } else if slen != 0 {
            return None;
        }
        let keep = flags & (CLIP_TRIG_F_CONSUME | CLIP_TRIG_F_RUN);
        if let Some(i) = found {
            // An identical re-set keeps the run and the count.
            let r = &mut self.rows[i];
            if (r.action, r.flags, r.slen) != (action, keep, slen) {
                (r.action, r.flags, r.slen) = (action, keep, slen);
                (r.hits, r.live) = (0, false);
            }
            return Some(i);
        }
        if self.rows.len() >= CLIP_PKT_TRIG_MAX || self.match_used() + mlen > CLIP_PKT_MATCH_POOL {
            return None;
        }
        self.rows.push(MockPacketTrigger {
            class,
            id,
            dir,
            action,
            flags: keep,
            slen,
            match_bytes: match_bytes.to_vec(),
            mask: mask.to_vec(),
            hits: 0,
            live: false,
        });
        Some(self.rows.len() - 1)
    }

    // clip_ptrig_drop_consuming: the opt-in went off, and consuming a packet is dropping traffic.
    fn drop_consuming(&mut self) {
        self.rows.retain(|r| r.flags & CLIP_TRIG_F_CONSUME == 0);
    }

    // clip_ptrig_packet: every RUN trigger on this exact address updates its run, top-ranked or not;
    // the top-ranked trigger is charged a hit.
    fn packet(&mut self, class: u8, id: u16, dir: u8, head: &[u8]) -> (Option<u8>, bool) {
        let mut top: Option<(usize, u32)> = None;
        for (i, r) in self.rows.iter().enumerate() {
            if let Some(s) = r.score(class, id, dir, head) {
                // Strictly greater, so the earlier of two equally specific triggers ranks first.
                if top.is_none_or(|(_, best)| s > best) {
                    top = Some((i, s));
                }
            }
        }
        let was_live = top.is_some_and(|(i, _)| self.rows[i].live);
        for r in &mut self.rows {
            if r.flags & CLIP_TRIG_F_RUN == 0 || (r.class, r.id, r.dir) != (class, id, dir) {
                continue;
            }
            if !pkt_head_eq(&r.match_bytes, &r.mask, r.slen as usize, head) {
                continue; // another stream's packet
            }
            r.live = pkt_head_eq(&r.match_bytes, &r.mask, r.match_bytes.len(), head);
        }
        let Some((i, _)) = top else {
            return (None, false);
        };
        let r = &mut self.rows[i];
        r.hits = r.hits.saturating_add(1);
        let mid_run = r.flags & CLIP_TRIG_F_RUN != 0 && was_live;
        (
            (!mid_run).then_some(r.action),
            r.flags & CLIP_TRIG_F_CONSUME != 0,
        )
    }
}

// One transform-table row the mock holds, in wire fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MockTransform {
    op: u8,
    sclass: u8,
    sid: u16,
    dclass: u8,
    did: u16,
}

impl MockTransform {
    // The (sclass, sid, dclass, did) key two rows collide on.
    fn key(&self) -> (u8, u16, u8, u16) {
        (self.sclass, self.sid, self.dclass, self.did)
    }
}

// One stored descriptor patch the mock holds.
#[derive(Debug, Clone, PartialEq, Eq)]
struct MockPatch {
    section: u8,
    cfg: u8,
    index: u8,
    offset: u16,
    bytes: Vec<u8>,
}

impl MockPatch {
    fn key(&self) -> (u8, u8, u8, u16) {
        (self.section, self.cfg, self.index, self.offset)
    }
}

impl Default for State {
    fn default() -> Self {
        State {
            update: MockUpdate::default(),
            replied: Vec::new(),
            version: Version {
                proto_ver: crate::protocol::PROTO_VER,
                fw_major: 0,
                fw_minor: 0,
                fw_patch: 0,
                mac: [0; 6],
                name: String::new(),
            },
            health: Health::from_flags(0),
            device_info: DeviceInfo::default(),
            // A five-button mouse, so the lock table's button cap agrees with `RESP(CAPS)`; a test
            // wanting more buttons or AC Pan sets its own caps.
            caps: Caps {
                mouse: MouseCaps {
                    n_buttons: 5,
                    has_x: true,
                    has_y: true,
                    has_wheel: true,
                    pan: false,
                    has_report_id: false,
                    n_hid: 1,
                },
                ..Caps::default()
            },
            rate: Rate::from_payload(&[4, 0, 0, 0, 0, 0]).unwrap(),
            stats: Stats::from_payload(&[
                5, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0,
            ])
            .unwrap(),
            table: LockTable::default(),
            locks: None,
            catch: CatchState::from_payload(&[
                7, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xFF, 0xFF, 0,
            ])
            .unwrap(),
            imperfect: ImperfectStatus::default(),
            move_ride_ms: 0,
            bearing: Bearing::default(),
            emit_pace: EmitPace::Learned,
            render_mode: RenderMode::Despiked,
            render_full: false,
            render_ready: false,
            spread_percent: 100,
            spread_learned_us: 0,
            emit_force_hz: None,
            advertised_hz: 0,
            clip: ClipStatus::default(),
            clip_settings: ClipSettings::default(),
            packet_triggers: PacketTriggers::default(),
            rewrites: Vec::new(),
            rewrite_gen: 0,
            rewrite_full: false,
            patches: Vec::new(),
            patch_presented: Vec::new(),
            patch_refused: false,
            patch_full: false,
            transforms: Vec::new(),
            transform_full: false,
            transfer_reply: (0x00, Vec::new()),
            recorded: Vec::new(),
            respond: true,
            pc_seen: true,
            session_dirty: false,
            clone_up: true,
            clone_up_at: None,
            grace_until: None,
            device_back: false,
            represent_held: false,
            represent_at: None,
            represent_delay: REPRESENT_DELAY,
            #[cfg(test)]
            teardown_before_command: false,
            #[cfg(test)]
            link_lost_before_clip_query: false,
            unsolicited: Vec::new(),
        }
    }
}

// The box's lock table, modelled as the firmware holds it, so `RESP(LOCKS)` replies as a box does.
const LOCK_TGT_BTN_BASE: usize = 4; // CTRL_LOCK_TGT_BTN_BASE: 4 axes (X, Y, wheel, pan) precede the buttons
const LOCK_TGT_COUNT: usize = LOCK_TGT_BTN_BASE + MAX_BUTTONS as usize; // 4 axes + 16 buttons
const LOCK_SLOT_WITH: usize = 2;
const SLOT_DIRS: [u8; 4] = [LOCK_DIR_POS, LOCK_DIR_NEG, LOCK_DIR_WITH, LOCK_DIR_AGAINST];
// CTRL_RESP_LOCKS_MAXN and INPUT_MEDIA_MAX: past either the box drops silently.
const RESP_LOCKS_MAXN: usize = 85;
const MEDIA_LOCK_MAX: usize = 8;
// The rest of ctrl_proto.h's reply bounds.
const NAME_MAX: usize = 32; // CTRL_NAME_MAX
const DEVICE_INFO_PRODUCT_MAX: usize = 127; // CTRL_DEVICE_INFO_PRODUCT_MAX
const CATCH_MAXN: usize = 32; // CTRL_CATCH_MAXN
const USAGE_EVENT_MAX: usize = 40; // CTRL_USAGE_EVENT_MAX
const CLIP_HELD_MAX: usize = USAGE_EVENT_MAX; // CTRL_CLIP_HELD_MAX, defined as CTRL_USAGE_EVENT_MAX
const TRAFFIC_DATA_MAX: usize = 180; // CTRL_TRAFFIC_DATA_MAX
const PATCH_POOL: usize = 1024; // PATCH_POOL, the bytes every stored patch shares
// How long a booted device chip takes to clone the attached device again.
const BOOT_CLONE_DELAY: std::time::Duration = std::time::Duration::from_millis(100);
// main.c's DETACH_GRACE_US: a detached device that comes back inside it keeps the clone.
const DETACH_GRACE: std::time::Duration = std::time::Duration::from_millis(250);
// The main loop's tick at its longest, which presents a clone the opt-in toggled after the command.
const REPRESENT_DELAY: std::time::Duration = std::time::Duration::from_millis(50);

#[derive(Debug, Clone)]
pub(crate) struct LockTable {
    mouse: [[i16; 4]; LOCK_TGT_COUNT],
    key_blanket: u8,
    key_press: [bool; 256],
    key_release: [bool; 256],
    media: [u16; MEDIA_LOCK_MAX], // 0 = free slot, as the firmware's list holds it
    media_blanket: bool,
}

impl Default for LockTable {
    fn default() -> LockTable {
        LockTable {
            mouse: [[LOCK_SCALE_PASS; 4]; LOCK_TGT_COUNT],
            key_blanket: 0,
            key_press: [false; 256],
            key_release: [false; 256],
            media: [0; MEDIA_LOCK_MAX],
            media_blanket: false,
        }
    }
}

fn slot_mask(dir: u8) -> u8 {
    match dir {
        LOCK_DIR_BOTH => 0x0F,
        LOCK_DIR_POS => 0x01,
        LOCK_DIR_NEG => 0x02,
        LOCK_DIR_WITH => 0x04,
        LOCK_DIR_AGAINST => 0x08,
        _ => 0,
    }
}

impl LockTable {
    fn set_mouse(&mut self, target: usize, dir: u8, scale: i16) {
        let slots = slot_mask(dir);
        for i in 0..4 {
            if slots & (1 << i) == 0 {
                continue;
            }
            // One bit is all a button carries, so the box stores the block or pass it amounts to.
            let mut v = scale;
            if target >= LOCK_TGT_BTN_BASE {
                v = if v < LOCK_SCALE_PASS {
                    LOCK_SCALE_BLOCK
                } else {
                    LOCK_SCALE_PASS
                };
            }
            if i >= LOCK_SLOT_WITH {
                // A button has no bearing, so a named relative direction is refused; Both writes a
                // pass to the relative pair, never the scale, since the two multiply.
                if target >= LOCK_TGT_BTN_BASE && dir != LOCK_DIR_BOTH {
                    continue;
                }
                if dir == LOCK_DIR_BOTH {
                    v = LOCK_SCALE_PASS;
                }
            }
            self.mouse[target][i] = v;
        }
    }

    // A button blanket writes `n_buttons` rows and a higher id is dropped, as the firmware caps at
    // `nbtn`.
    pub(crate) fn apply(&mut self, class: u8, id: u16, dir: u8, scale: i16, n_buttons: u8) {
        // One bit has nothing to reverse, so the box writes nothing for a negative on a momentary
        // class (usbdev_set_lock).
        if scale < 0 && class != LOCK_CLS_AXIS {
            return;
        }
        let on = scale < LOCK_SCALE_PASS;
        match class {
            LOCK_CLS_AXIS => {
                if id == LOCK_ID_ALL {
                    for t in 0..=LOCK_AXIS_PAN as usize {
                        self.set_mouse(t, dir, scale);
                    }
                } else if id <= LOCK_AXIS_PAN {
                    self.set_mouse(id as usize, dir, scale);
                }
            }
            LOCK_CLS_BTN => {
                let nbtn = (n_buttons as usize).min(MAX_BUTTONS as usize);
                if id == LOCK_ID_ALL {
                    for b in 0..nbtn {
                        self.set_mouse(LOCK_TGT_BTN_BASE + b, dir, scale);
                    }
                } else if (id as usize) < nbtn {
                    self.set_mouse(LOCK_TGT_BTN_BASE + id as usize, dir, scale);
                }
            }
            LOCK_CLS_KEY => {
                if id == LOCK_ID_ALL {
                    // The blanket carries the two edge slots only; a relative direction names
                    // neither and is dropped.
                    let m = slot_mask(dir) & 0x03;
                    if m == 0 {
                        return;
                    }
                    if on {
                        self.key_blanket |= m;
                    } else {
                        self.key_blanket &= !m;
                    }
                } else {
                    let u = (id & 0xFF) as usize;
                    if u < 0x04 {
                        return;
                    }
                    if dir == LOCK_DIR_BOTH || dir == LOCK_DIR_POS {
                        self.key_press[u] = on;
                    }
                    if dir == LOCK_DIR_BOTH || dir == LOCK_DIR_NEG {
                        self.key_release[u] = on;
                    }
                }
            }
            LOCK_CLS_MEDIA => {
                // A media usage is suppressed whole; the direction byte is ignored.
                if id == LOCK_ID_ALL {
                    self.media_blanket = on;
                } else if id != 0 {
                    if !on {
                        for slot in self.media.iter_mut().filter(|s| **s == id) {
                            *slot = 0;
                        }
                    } else if !self.media.contains(&id)
                        && let Some(slot) = self.media.iter_mut().find(|s| **s == 0)
                    {
                        *slot = id;
                    }
                }
            }
            _ => {}
        }
    }

    // In vector mode one relative scale, the lower of X's and Y's, governs both axes, so the
    // readback reports it on both.
    fn reported(&self, t: usize, slot: usize, vector: bool) -> i16 {
        let sc = self.mouse[t][slot];
        if !vector || slot < LOCK_SLOT_WITH || t > Axis::Y.as_u16() as usize {
            return sc;
        }
        sc.min(self.mouse[1 - t][slot])
    }

    pub(crate) fn pack(&self, mode: BearingMode) -> Locks {
        let vector = mode == BearingMode::Vector;
        let mut out: Vec<LockEntry> = Vec::new();
        let mut push = |scope: LockScope, direction: u8, scale: i16| {
            if out.len() < RESP_LOCKS_MAXN {
                out.push(LockEntry {
                    scope,
                    direction: Direction::from_u8(direction).expect("slot direction"),
                    scale,
                });
            }
        };
        for t in 0..LOCK_TGT_COUNT {
            for (slot, &dir) in SLOT_DIRS.iter().enumerate() {
                let scale = self.reported(t, slot, vector);
                if scale == LOCK_SCALE_PASS {
                    continue;
                }
                let target = if t < LOCK_TGT_BTN_BASE {
                    LockTarget::Axis(match t {
                        0 => Axis::X,
                        1 => Axis::Y,
                        2 => Axis::Wheel,
                        _ => Axis::Pan,
                    })
                } else {
                    LockTarget::Usage(Usage::new(Class::Button, (t - LOCK_TGT_BTN_BASE) as u16))
                };
                push(LockScope::Target(target), dir, scale);
            }
        }
        for (bit, dir) in [(0x01, LOCK_DIR_POS), (0x02, LOCK_DIR_NEG)] {
            if self.key_blanket & bit != 0 {
                push(LockScope::Blanket(Class::Key), dir, LOCK_SCALE_BLOCK);
            }
        }
        // Media (bounded at MEDIA_LOCK_MAX) before the unbounded granular keys, so keys cannot crowd
        // media out at the entry cap. A media usage is suppressed whole, so it reports Both.
        if self.media_blanket {
            push(
                LockScope::Blanket(Class::Media),
                LOCK_DIR_BOTH,
                LOCK_SCALE_BLOCK,
            );
        }
        for &id in self.media.iter().filter(|&&id| id != 0) {
            push(
                LockScope::Target(LockTarget::Usage(Usage::new(Class::Media, id))),
                LOCK_DIR_BOTH,
                LOCK_SCALE_BLOCK,
            );
        }
        // Granular keys last, on what is left of the cap; past it they truncate with no signal, so
        // nothing bounded follows them.
        for u in 0..256u16 {
            let usage = LockScope::Target(LockTarget::Usage(Usage::new(Class::Key, u)));
            if self.key_press[u as usize] {
                push(usage, LOCK_DIR_POS, LOCK_SCALE_BLOCK);
            }
            if self.key_release[u as usize] {
                push(usage, LOCK_DIR_NEG, LOCK_SCALE_BLOCK);
            }
        }
        Locks::from_entries(out)
    }
}

impl State {
    fn apply_lock_frame(&mut self, p: &[u8]) {
        // A lock needs the clone's targets, which exist only while a clone is up.
        if p.len() < 6 || !self.clone_up {
            return;
        }
        let n_buttons = self.caps.mouse.n_buttons;
        self.table.apply(
            p[0],
            u16::from_le_bytes([p[1], p[2]]),
            p[3],
            i16::from_le_bytes([p[4], p[5]]),
            n_buttons,
        );
    }

    // As the box: keyed add/overwrite/remove, a monotonic gen, a whole-table clear, and the caps that
    // raise `full`. Dropped whole while the opt-in is off.
    fn apply_rewrite_frame(&mut self, p: &[u8]) {
        if p.len() < 9 {
            return;
        }
        let framed = p[8] as usize;
        if p.len() < 9 + 2 * framed || 11 + p.len() - 9 > crate::protocol::opcode::MAX_PAYLOAD {
            return; // a truncated match, or a rule its own read-back reply cannot carry
        }
        let clear_all = p[4] == 0 && p[0] == 0xFF && p[1] == 0xFF && p[2] == 0xFF;
        if !self.imperfect.allowed && !clear_all {
            return; // the box drops a REWRITE frame with the opt-in off
        }
        let cls = p[0];
        let id = u16::from_le_bytes([p[1], p[2]]);
        let dir = p[3];
        let state = p[4];
        let action = p[5];
        let offset = u16::from_le_bytes([p[6], p[7]]);
        let mlen = p[8] as usize;
        // The ANY/ANY state-0 blanket clears the table, keeping gen monotonic across the clear.
        if state == 0 && cls == 0xFF && id == 0xFFFF {
            if !self.rewrites.is_empty() {
                self.rewrites.clear();
                self.rewrite_gen = self.rewrite_gen.wrapping_add(1);
            }
            self.rewrite_full = false;
            return;
        }
        let rewritable = matches!(
            cls,
            CATCH_CLS_HID_IN
                | CATCH_CLS_HID_OUT
                | CATCH_CLS_VEND_INTR
                | CATCH_CLS_VEND_BULK
                | CATCH_CLS_CONTROL
                | CATCH_CLS_EMIT
                | CATCH_CLS_ANY
        );
        if !rewritable || dir > LOCK_DIR_NEG {
            return; // the box refuses a class that is never rewritten and a relative direction
        }
        if mlen > REWRITE_MATCH_MAX {
            return; // the box compares at most this many bytes
        }
        let match_bytes = p[9..9 + mlen].to_vec();
        let mask = p[9 + mlen..9 + 2 * mlen].to_vec();
        let payload = p[9 + 2 * mlen..].to_vec();
        let key = (cls, id, dir, match_bytes.clone(), mask.clone());
        let pos = self.rewrites.iter().position(|r| r.key() == key);
        if state != 0 && !rewrite_admissible(cls, action, offset, &payload) {
            return; // refused whole, and an existing rule on the key stays as it was
        }
        if state == 0 {
            if let Some(i) = pos {
                self.rewrites.remove(i);
                self.rewrite_gen = self.rewrite_gen.wrapping_add(1);
                self.rewrite_full = false;
            }
            return;
        }
        // An identical re-set changes nothing: gen stays (§3.14) and so does full, or the keepalive's
        // re-sends would clear a refusal as soon as it was reported.
        if let Some(i) = pos {
            let r = &self.rewrites[i];
            if r.action == action && r.offset == offset && r.payload == payload {
                return;
            }
        }
        // rewrite_tab_set costs the payload against the pool less what an overwrite frees, then the
        // entry count; either refusal for room sets full.
        let used: usize = self
            .rewrites
            .iter()
            .filter(|r| r.key() != key)
            .map(|r| r.payload.len())
            .sum();
        let no_room = used + payload.len() > REWRITE_PAYLOAD_POOL
            || (pos.is_none() && self.rewrites.len() >= REWRITE_MAX_ENTRIES);
        if no_room {
            self.rewrite_full = true;
            return;
        }
        // An overwrite resets the rule's hits and moves it to the end, as the box re-adds it.
        if let Some(i) = pos {
            self.rewrites.remove(i);
        }
        self.rewrites.push(MockRewrite {
            class: cls,
            id,
            dir,
            action,
            offset,
            match_bytes,
            mask,
            payload,
            hits: 0,
        });
        self.rewrite_gen = self.rewrite_gen.wrapping_add(1);
        self.rewrite_full = false;
    }

    // Apply a CLIP_TRIGGER frame. A traffic class makes it a packet trigger, which the table takes
    // or refuses whole. The clear-all sentinel drops both kinds.
    fn apply_clip_trigger_frame(&mut self, p: &[u8]) {
        let Some(&class) = p.first() else {
            return;
        };
        if (CATCH_CLS_HID_IN..=CATCH_CLS_EMIT).contains(&class) {
            let Some(&mlen) = p.get(CLIP_PKT_TRIG_HDR - 1) else {
                return;
            };
            let mlen = mlen as usize;
            let Some(body) = p.get(CLIP_PKT_TRIG_HDR..CLIP_PKT_TRIG_HDR + 2 * mlen) else {
                return; // a match or mask cut short
            };
            let imperfect = self.imperfect.allowed;
            self.packet_triggers.set(
                class,
                u16::from_le_bytes([p[1], p[2]]),
                p[3],
                p[4],
                p[5],
                p[6],
                &body[..mlen],
                &body[mlen..],
                imperfect,
            );
            return;
        }
        if p.len() < 6 {
            return;
        }
        let id = u16::from_le_bytes([p[1], p[2]]);
        if class == CLIP_COND_ANY_CLASS
            && id == CLIP_COND_ANY_ID
            && p[3] == LOCK_DIR_BOTH
            && p[5] & CLIP_TRIG_F_PRESENT == 0
        {
            self.clip_settings.triggers.clear();
            self.packet_triggers.rows.clear();
        }
    }

    // The clip config goes with the rest of the box's soft state (clip_lifecycle_reset_locked).
    // RST_F_NVS wipes everything in NVS, back to `State::default`'s values, where a wiped box boots.
    fn reset_persistent(&mut self) {
        self.version.name = String::new();
        // Only `allowed` is stored; the box re-derives over_capacity and clone_imperfect when it
        // clones again after the reboot.
        self.imperfect.allowed = ImperfectStatus::default().allowed;
        self.move_ride_ms = 0;
        self.bearing = Bearing::default();
        self.emit_pace = EmitPace::Learned;
        self.emit_force_hz = None;
        self.render_mode = RenderMode::Despiked;
        self.render_full = false;
        self.spread_percent = 100;
        self.patches.clear();
        self.patch_presented.clear();
        self.patch_refused = false;
        self.patch_full = false;
    }

    // A device-chip boot: the session goes, NVS stays, the clone is presented afresh from the stored
    // patch set, and the box says hello now and on the first frame it hears.
    fn restart(&mut self) {
        self.release_session();
        // The RAM counters restart at zero; the clone returns once the snapshot is in, with nothing to
        // release.
        self.stats.reset_count = 0;
        self.stats.config_count = 0;
        self.stats.session = 0;
        self.session_dirty = false;
        self.clone_up = false;
        self.clone_up_at = Some(std::time::Instant::now() + BOOT_CLONE_DELAY);
        self.grace_until = None;
        self.represent_held = false;
        self.represent_at = None;
        self.patch_presented = self.patches_to_serve();
        self.patch_refused = false;
        self.patch_full = false;
        self.pc_seen = false;
        let hello = encode(FrameType::Resp, 0, &version_payload(&self.version)).expect("fits");
        self.unsolicited.extend(hello);
    }

    // What the box has done on its own by `now`.
    fn advance(&mut self, now: std::time::Instant) {
        if self.clone_up_at.is_some_and(|t| now >= t) {
            self.clone_up_at = None;
            self.clone_up = true;
        }
        if self.grace_until.is_some_and(|t| now >= t) {
            self.grace_until = None;
            if self.device_back {
                if std::mem::take(&mut self.represent_held) {
                    self.present_patches();
                }
            } else {
                self.represent_held = false;
                self.tear_down();
            }
        }
        if self.represent_at.is_some_and(|t| now >= t) {
            self.represent_at = None;
            self.present_patches();
        }
    }

    // The clone's teardown after a detach's grace: counted only when a command came since the detach.
    fn tear_down(&mut self) {
        self.count_release();
        self.release_session();
        self.clone_up = false;
    }

    // session_released: one release counts once, and only after a command that could have set something.
    fn count_release(&mut self) {
        if self.session_dirty {
            self.stats.session = self.stats.session.wrapping_add(1);
        }
        self.session_dirty = false;
    }

    // What stop_locked and usbdev_safety_clear release: the session, as a replug of the device.
    fn release_session(&mut self) {
        self.table = LockTable::default();
        self.rewrites.clear();
        self.rewrite_gen = 0;
        self.rewrite_full = false;
        self.transforms.clear();
        self.transform_full = false;
        self.clip = ClipStatus::default();
        self.clear_clip_config();
    }

    fn patches_to_serve(&self) -> Vec<MockPatch> {
        if self.imperfect.allowed {
            self.patches.clone()
        } else {
            Vec::new()
        }
    }

    // APPLY, CLEAR and an opt-in toggle re-present the clone when what it serves changes: a re-clone,
    // so the session goes and the game PC enumerates the clone again.
    fn present_patches(&mut self) {
        // Inside a detach's grace the snapshot may be of a device that has gone: the request waits.
        if self.grace_until.is_some() {
            self.represent_held = true;
            return;
        }
        let serve = self.patches_to_serve();
        if serve != self.patch_presented {
            self.patch_presented = serve;
            self.patch_refused = false;
            self.count_release();
            self.release_session();
            self.stats.reset_count = self.stats.reset_count.saturating_add(1);
            self.stats.config_count = self.stats.config_count.saturating_add(1);
        }
    }

    // PATCH CLEAR: erase the stored set and re-present a patched clone without patches, as APPLY
    // presents it.
    fn clear_patches(&mut self) {
        self.patches.clear();
        self.patch_refused = false;
        self.patch_full = false;
        self.present_patches();
    }

    fn clear_clip_config(&mut self) {
        self.clip_settings = ClipSettings::default();
        self.packet_triggers.rows.clear();
    }

    // Script the clip config.
    fn script_clip_settings(&mut self, mut settings: ClipSettings) {
        self.packet_triggers.rows.clear();
        let imperfect = self.imperfect.allowed;
        for e in std::mem::take(&mut settings.packet_triggers) {
            let t = e.trigger;
            let taken = self.packet_triggers.set(
                t.class.as_u8(),
                t.id,
                t.direction.as_u8(),
                t.action.as_u8(),
                CLIP_TRIG_F_PRESENT | t.flags(),
                t.selector_len,
                &t.match_bytes,
                &t.mask,
                imperfect,
            );
            if let Some(i) = taken {
                self.packet_triggers.rows[i].hits = e.hits as u32;
            }
        }
        self.clip_settings = settings;
    }

    // A scripted opt-in, which goes off as OPTION(IMPERFECT) takes it off: the consuming packet
    // triggers go with it.
    fn script_imperfect(&mut self, imperfect: ImperfectStatus) {
        self.imperfect = imperfect;
        if !imperfect.allowed {
            self.packet_triggers.drop_consuming();
        }
    }

    // Modelled on transform_tab_set (§3.15). A transform is faithful, so it runs whatever the opt-in.
    fn apply_transform_frame(&mut self, p: &[u8]) {
        if p.len() < 8 {
            return;
        }
        let op = p[0];
        let sclass = p[1];
        let sid = u16::from_le_bytes([p[2], p[3]]);
        let dclass = p[4];
        let did = u16::from_le_bytes([p[5], p[6]]);
        let state = p[7];
        // The all-0xFF state-0 blanket clears the table.
        if state == 0 && sclass == 0xFF && dclass == 0xFF && sid == 0xFFFF && did == 0xFFFF {
            self.transforms.clear();
            self.transform_full = false;
            return;
        }
        let key = (sclass, sid, dclass, did);
        let pos = self.transforms.iter().position(|t| t.key() == key);
        if state == 0 {
            if let Some(i) = pos {
                self.transforms.remove(i);
                self.transform_full = false;
            }
            return;
        }
        // state 1: add or overwrite, after the same admissibility gauntlet the box runs.
        if op >= TF_OP_COUNT  // `2` is the retired SCALE op, which a stale host still sends
            || !transform_pair_ok(op, sclass, sid, dclass, did)
            || !self.transform_field_present(sclass, sid)
            || !self.transform_field_present(dclass, did)
        {
            return;
        }
        match pos {
            Some(i) => {
                // The key matches: only the op changes, keeping the row's position.
                self.transforms[i].op = op;
            }
            None => {
                if self.transforms.len() >= TRANSFORM_MAX_ENTRIES {
                    self.transform_full = true;
                    return;
                }
                self.transforms.push(MockTransform {
                    op,
                    sclass,
                    sid,
                    dclass,
                    did,
                });
            }
        }
    }

    // Mirrors transform_field_present over RESP(CAPS): an axis when its flag is set, a button under
    // the declared count and the box's ceiling, a key with a keyboard collection bound, media with a
    // consumer collection.
    fn transform_field_present(&self, cls: u8, id: u16) -> bool {
        if !self.clone_up {
            return false;
        }
        match cls {
            CATCH_CLS_AXIS => match id {
                0 => self.caps.mouse.has_x,
                1 => self.caps.mouse.has_y,
                2 => self.caps.mouse.has_wheel,
                3 => self.caps.mouse.pan,
                _ => false,
            },
            CATCH_CLS_BTN => id < self.caps.mouse.n_buttons as u16 && id < MAX_BUTTONS as u16,
            // A keycode below 0x04 is no key, and the field is a byte wide; media usage 0 is no usage.
            CATCH_CLS_KEY => self.caps.keyboard.n_keys > 0 && (0x04..=0xFF).contains(&id),
            CATCH_CLS_MEDIA => self.caps.keyboard.has_consumer && id != 0,
            _ => false,
        }
    }

    // APPLY (only under the opt-in), CLEAR, or a keyed store (whatever the opt-in, as the box does;
    // empty bytes removes the patch at that key).
    fn apply_patch_frame(&mut self, p: &[u8]) {
        let Some(&section) = p.first() else {
            return;
        };
        match section {
            PATCH_APPLY => {
                if self.imperfect.allowed {
                    self.present_patches();
                }
                return;
            }
            PATCH_CLEAR => {
                self.clear_patches();
                return;
            }
            _ => {}
        }
        if p.len() < 5 || PatchSection::from_u8(section).is_none() {
            return;
        }
        let cfg = p[1];
        let index = p[2];
        let offset = u16::from_le_bytes([p[3], p[4]]);
        let bytes = p[5..].to_vec();
        let key = (section, cfg, index, offset);
        let pos = self.patches.iter().position(|q| q.key() == key);
        if bytes.is_empty() {
            if let Some(i) = pos {
                self.patches.remove(i);
                self.patch_full = false;
            }
            return;
        }
        // The bytes already held under the key change nothing: the patch keeps its place and full stays.
        if pos.is_some_and(|i| self.patches[i].bytes == bytes) {
            return;
        }
        // patch_set_put costs the bytes against the pool less what an overwrite frees, then the entry
        // count; either refusal for room sets full and keeps what was stored.
        let used: usize = self
            .patches
            .iter()
            .filter(|q| q.key() != key)
            .map(|q| q.bytes.len())
            .sum();
        if used + bytes.len() > PATCH_POOL
            || (pos.is_none() && self.patches.len() >= PATCH_MAX_ENTRIES)
        {
            self.patch_full = true;
            return;
        }
        // An overwrite moves the patch to the end of the set, as the box re-adds it.
        if let Some(i) = pos {
            self.patches.remove(i);
        }
        self.patches.push(MockPatch {
            section,
            cfg,
            index,
            offset,
            bytes,
        });
        self.patch_full = false;
    }

    fn apply_option_frame(&mut self, p: &[u8]) {
        if p.is_empty() {
            return;
        }
        match (p.first().copied(), &p[1..]) {
            (Some(OPT_IMPERFECT), [allow, ..]) => {
                let changed = self.imperfect.allowed != (*allow != 0);
                self.imperfect.allowed = *allow != 0;
                if !self.imperfect.allowed {
                    // Opt-off clears the rewrite table (usbdev_set_imperfect_allowed); gen stays
                    // monotonic across the clear.
                    let held = self.packet_triggers.rows.len();
                    self.packet_triggers.drop_consuming();
                    let dropped =
                        !self.rewrites.is_empty() || self.packet_triggers.rows.len() != held;
                    // The table is emptied whole, its generation with it.
                    self.rewrites.clear();
                    self.rewrite_gen = 0;
                    self.rewrite_full = false;
                    // Some of what a host set went and the rest stands: counted whatever came before.
                    if changed && dropped {
                        self.stats.session = self.stats.session.wrapping_add(1);
                    }
                }
                // The stored set is applied or withdrawn with the opt-in, re-presenting the clone on the
                // main loop's next tick.
                if changed {
                    self.represent_at = Some(std::time::Instant::now() + self.represent_delay);
                }
            }
            (Some(OPT_MOVE_RIDE), [lo, hi, ..]) => {
                self.move_ride_ms = u16::from_le_bytes([*lo, *hi])
            }
            (Some(OPT_EMIT), [mode, lo, hi, flo, fhi, ..]) => {
                // The box discards the whole command on an unknown mode, force_hz included, with no
                // reply.
                if let Some(p) = match *mode {
                    0 => Some(EmitPace::Learned),
                    1 => Some(EmitPace::Interval),
                    2 => Some(EmitPace::Fixed(u16::from_le_bytes([*lo, *hi]))),
                    _ => None,
                } {
                    let force = u16::from_le_bytes([*flo, *fhi]);
                    self.emit_pace = p;
                    self.emit_force_hz = (force != 0).then_some(force);
                }
            }
            (Some(OPT_RENDER), [mode, full, ..]) => {
                // Same rule: an unknown mode, or a `full` past 1, discards the whole command.
                if let (Some(m), true) = (RenderMode::from_u8(*mode), *full <= 1) {
                    self.render_mode = m;
                    self.render_full = *full != 0;
                }
            }
            (Some(OPT_SPREAD), [lo, hi, ..]) => {
                self.spread_percent = u16::from_le_bytes([*lo, *hi]);
            }
            (Some(OPT_NAME), name) => {
                self.version.name = String::from_utf8_lossy(name).into_owned()
            }
            (Some(OPT_BEARING), [lo, hi, mode, ..]) => {
                // An unknown mode is ignored whole, as the firmware ignores it, window and all.
                let Some(mode) = BearingMode::from_u8(*mode) else {
                    return;
                };
                let ms = u16::from_le_bytes([*lo, *hi]);
                self.bearing = Bearing {
                    window: (ms != 0).then(|| std::time::Duration::from_millis(ms as u64)),
                    mode,
                };
            }
            _ => {}
        }
    }
}

fn version_payload(v: &Version) -> Vec<u8> {
    let mut p = vec![0u8, v.proto_ver, v.fw_major, v.fw_minor, v.fw_patch];
    p.extend_from_slice(&v.mac);
    // usbdev_box_name_copy stops at CTRL_NAME_MAX, so a longer name reads back cut.
    p.extend_from_slice(&v.name.as_bytes()[..v.name.len().min(NAME_MAX)]);
    p
}

fn device_info_payload(m: &DeviceInfo) -> Vec<u8> {
    let mut flags = 0u8;
    if m.has_serial {
        flags |= DI_HAS_SERIAL;
    }
    if m.has_bos {
        flags |= DI_HAS_BOS;
    }
    let kind = match m.kind {
        DeviceKind::Unknown => 0,
        DeviceKind::Keyboard => 1,
        DeviceKind::Mouse => 2,
    };
    let mut p = vec![2u8];
    p.extend_from_slice(&m.vid.to_le_bytes());
    p.extend_from_slice(&m.pid.to_le_bytes());
    p.extend_from_slice(&m.bcd_device.to_le_bytes());
    p.extend_from_slice(&m.bcd_usb.to_le_bytes());
    p.push(flags);
    p.push(kind);
    // The reply copies at most CTRL_DEVICE_INFO_PRODUCT_MAX bytes of the product tail.
    p.extend_from_slice(&m.product.as_bytes()[..m.product.len().min(DEVICE_INFO_PRODUCT_MAX)]);
    p
}

fn caps_payload(c: Caps) -> Vec<u8> {
    let mut axis = 0u8;
    if c.mouse.has_x {
        axis |= CAP_X;
    }
    if c.mouse.has_y {
        axis |= CAP_Y;
    }
    if c.mouse.has_wheel {
        axis |= CAP_WHEEL;
    }
    if c.mouse.pan {
        axis |= CAP_PAN;
    }
    if c.mouse.has_report_id {
        axis |= CAP_REPORT_ID;
    }
    let mut kf = 0u8;
    if c.keyboard.nkro {
        kf |= KBC_NKRO;
    }
    if c.keyboard.has_consumer {
        kf |= KBC_CONSUMER;
    }
    if c.keyboard.has_system {
        kf |= KBC_SYSTEM;
    }
    if c.keyboard.has_report_id {
        kf |= KBC_REPORT_ID;
    }
    let mut cd = 0u8;
    if c.mouse_change_driven {
        cd |= CAPS_CD_MOUSE;
    }
    if c.kbd_change_driven {
        cd |= CAPS_CD_KBD;
    }
    vec![
        3u8,
        c.mouse.n_buttons,
        axis,
        c.mouse.n_hid,
        c.keyboard.n_keys,
        kf,
        cd,
    ]
}

fn rate_payload(r: Rate) -> Vec<u8> {
    let flags = if r.confident { RATE_CONFIDENT } else { 0 };
    let mut p = vec![4u8];
    p.extend_from_slice(&r.native_period_us.to_le_bytes());
    p.extend_from_slice(&r.poll_period_us.to_le_bytes());
    p.push(flags);
    p
}

fn stats_payload(s: Stats) -> Vec<u8> {
    let mut p = vec![5u8];
    p.extend_from_slice(&s.inject_emits.to_le_bytes());
    p.extend_from_slice(&s.tx_drops.to_le_bytes());
    p.extend_from_slice(&s.tx_merges.to_le_bytes());
    p.push(s.tx_maxdepth);
    p.push(s.tx_wedges);
    p.extend_from_slice(&s.wakeups.to_le_bytes());
    p.extend_from_slice(&s.reset_count.to_le_bytes());
    p.extend_from_slice(&s.config_count.to_le_bytes());
    p.extend_from_slice(&s.link_rx_drops.to_le_bytes());
    p.extend_from_slice(&s.host_rx_drops.to_le_bytes());
    p.extend_from_slice(&s.relay_drops.to_le_bytes());
    p.extend_from_slice(&s.session.to_le_bytes());
    p
}

fn locks_payload(l: &Locks) -> Vec<u8> {
    use crate::protocol::opcode::{LOCK_CLS_AXIS, LOCK_ID_ALL};
    use crate::types::{LockScope, LockTarget};
    // The box stops appending at RESP_LOCKS_MAXN and answers with what fit (ctrl_locks_append), so
    // a longer `Locks` truncates here.
    let entries = &l.entries()[..l.entries().len().min(RESP_LOCKS_MAXN)];
    let mut p = vec![6u8, entries.len() as u8];
    for e in entries {
        let (class, id) = match e.scope {
            LockScope::Blanket(class) => (class.as_u8(), LOCK_ID_ALL),
            LockScope::Target(LockTarget::Axis(a)) => (LOCK_CLS_AXIS, a.as_u16()),
            LockScope::Target(LockTarget::Usage(u)) => u.class_id(),
        };
        p.push(class);
        p.extend_from_slice(&id.to_le_bytes());
        p.push(e.direction.as_u8());
        p.extend_from_slice(&e.scale.to_le_bytes());
    }
    p
}

fn catch_resp_payload(c: &CatchState) -> Vec<u8> {
    let mut p = vec![7u8, c.table_full as u8];
    p.extend_from_slice(&c.dropped.to_le_bytes());
    p.extend_from_slice(&c.clock.offset_us.to_le_bytes());
    p.extend_from_slice(&c.clock.rate_ppb.unwrap_or(CLK_RATE_NONE).to_le_bytes());
    p.extend_from_slice(&c.clock.delay_us.to_le_bytes());
    // 0xFFFF is "no estimate", which a consumer must be able to tell from a zero-age one.
    let age = c
        .clock
        .age
        .map_or(u16::MAX, |d| d.as_millis().min(u16::MAX as u128 - 1) as u16);
    p.extend_from_slice(&age.to_le_bytes());
    // ctrl_catch_append stops at CTRL_CATCH_MAXN, which is the table's own size.
    let entries = &c.entries[..c.entries.len().min(CATCH_MAXN)];
    p.push(entries.len() as u8);
    for e in entries {
        let (class, id) = e.filter.wire();
        p.push(class);
        p.extend_from_slice(&id.to_le_bytes());
        p.push(e.filter.direction().as_u8());
        p.push(e.filter.capture().as_u8());
        p.extend_from_slice(&e.dropped.to_le_bytes());
    }
    p
}

fn options_imperfect_payload(i: ImperfectStatus) -> Vec<u8> {
    vec![
        9u8,
        OPT_IMPERFECT,
        i.allowed as u8,
        i.over_capacity as u8,
        i.clone_imperfect as u8,
    ]
}

fn options_move_ride_payload(ms: u16) -> Vec<u8> {
    let mut p = vec![9u8, OPT_MOVE_RIDE];
    p.extend_from_slice(&ms.to_le_bytes());
    p
}

fn options_bearing_payload(b: Bearing) -> Vec<u8> {
    let mut p = vec![9u8, OPT_BEARING];
    p.extend_from_slice(&crate::device::options::ride_window_ms(b.window).to_le_bytes());
    p.push(b.mode.as_u8());
    p
}

fn options_emit_payload(
    pace: EmitPace,
    render: RenderMode,
    render_ready: bool,
    force_hz: Option<u16>,
    native_hz: u16,
    allowed: bool,
) -> Vec<u8> {
    // Rendering has its own option, but its gate still sets the resolved rate in the pace reply.
    let (mode, fixed_hz, mut resolved) = match pace {
        EmitPace::Learned => (0u8, 0u16, 0u16),
        EmitPace::Interval => (1, 0, 0),
        EmitPace::Fixed(h) => {
            let hz = if h == 0 { 1000 } else { h.min(1000) };
            let n = (((1_000_000u32 / hz as u32) + 500) / 1000).max(1);
            (2, hz, (1000 / n) as u16)
        }
    };
    // The texture forces resolved to 1 kHz only when the pace resolved to no period of its own; a
    // Fixed rate keeps its snapped value.
    if render != RenderMode::Off && render_ready && resolved == 0 {
        resolved = 1000;
    }
    // The box resolves a forced rate to a bInterval in whole 1 ms frames and advertises 1000/n, so
    // a request that is not a divisor of 1000 comes back as something else.
    let (advertised, active) = match force_hz.filter(|hz| *hz != 0 && allowed) {
        None => (native_hz, false),
        Some(hz) => {
            // As rate_force_binterval: a host rounds a full-speed interval down to a power of two,
            // so the box advertises only those.
            let n = ((1000u32 + hz as u32 / 2) / hz as u32).min(128);
            let mut p = 1u32;
            while p * 2 <= n {
                p *= 2;
            }
            ((1000u32 / p) as u16, true)
        }
    };
    let mut p = vec![9u8, OPT_EMIT, mode];
    p.extend_from_slice(&fixed_hz.to_le_bytes());
    p.extend_from_slice(&resolved.to_le_bytes());
    p.extend_from_slice(&force_hz.unwrap_or(0).to_le_bytes());
    p.extend_from_slice(&advertised.to_le_bytes());
    p.push(active as u8);
    p
}

fn options_render_payload(mode: RenderMode, full: bool, ready: bool) -> Vec<u8> {
    vec![9u8, OPT_RENDER, mode.to_wire(), full as u8, ready as u8]
}

// The interval comes from the command period learned off MOVE arrivals; 0 while none is learned or
// the option is off.
fn options_spread_payload(percent: u16, learned_us: u32) -> Vec<u8> {
    let span = if percent == 0 || learned_us == 0 {
        0
    } else {
        ((learned_us as u64 * percent as u64) / 100) as u32
    };
    let mut p = vec![9u8, OPT_SPREAD];
    p.extend_from_slice(&percent.to_le_bytes());
    p.extend_from_slice(&span.to_le_bytes());
    p
}

fn clip_status_payload(c: &ClipStatus, cfg: &ClipSettings, packets: &PacketTriggers) -> Vec<u8> {
    let state = match c.state {
        ClipState::Idle => 0u8,
        ClipState::Playing => 1,
        ClipState::Paused => 2,
        ClipState::Faulted => 3,
    };
    let mut p = vec![10u8, state];
    p.extend_from_slice(&c.free.to_le_bytes());
    p.extend_from_slice(&c.total.to_le_bytes());
    p.extend_from_slice(&c.played.to_le_bytes());
    p.extend_from_slice(&c.ticks.to_le_bytes());
    p.extend_from_slice(&c.underruns.to_le_bytes());
    p.extend_from_slice(&c.overruns.to_le_bytes());
    p.extend_from_slice(&c.seq_gaps.to_le_bytes());
    p.extend_from_slice(&c.xfers.to_le_bytes());
    p.extend_from_slice(&c.xfer_errs.to_le_bytes());
    p.extend_from_slice(&c.gated.to_le_bytes());
    // ctrl_clip_held_append stops at CTRL_CLIP_HELD_MAX, ctrl_clip_trig_append at CLIP_TRIG_MAX.
    let held = &c.held[..c.held.len().min(CLIP_HELD_MAX)];
    p.push(held.len() as u8);
    for u in held {
        u.push_le(&mut p);
    }
    p.push(blanket_scope(&cfg.autolock));
    let flags = (if cfg.loop_ { CLIP_CFG_F_LOOP } else { 0 })
        | (if cfg.retain { CLIP_CFG_F_RETAIN } else { 0 })
        | (if cfg.finalized {
            CLIP_CFG_F_FINALIZED
        } else {
            0
        })
        | (if cfg.ride { CLIP_CFG_F_RIDE } else { 0 });
    p.push(flags);
    let triggers = &cfg.triggers[..cfg.triggers.len().min(CLIP_TRIG_MAX)];
    p.push(triggers.len() as u8);
    for t in triggers {
        let (class, id) = t.on.class_id();
        p.push(class);
        p.extend_from_slice(&id.to_le_bytes());
        p.push(t.edge.as_u8());
        p.push(t.action.as_u8());
        p.push(t.consume as u8);
    }
    // ctrl_clip_ptrig_append: the command that set each trigger, with the count spliced in.
    p.push(packets.rows.len() as u8);
    for r in &packets.rows {
        p.push(r.class);
        p.extend_from_slice(&r.id.to_le_bytes());
        p.extend_from_slice(&[r.dir, r.action, r.flags, r.slen, r.match_bytes.len() as u8]);
        p.extend_from_slice(&(r.hits.min(0xFFFF) as u16).to_le_bytes());
        p.extend_from_slice(&r.match_bytes);
        p.extend_from_slice(&r.mask);
    }
    p
}

// Whether the box stores the rule, mirroring rewrite_action_ok in rewrite_tab.h.
fn rewrite_admissible(cls: u8, action: u8, offset: u16, payload: &[u8]) -> bool {
    let ctl = cls == CATCH_CLS_CONTROL;
    let any = cls == CATCH_CLS_ANY;
    // The head the box holds for the class: a 64-byte report, or an 8+2048-byte control image.
    const HEAD_CONTROL: usize = 8 + 2048;
    let head = if ctl { HEAD_CONTROL } else { 64 };
    let fits = match action {
        RW_PATCH | RW_REPLY_PATCH => offset as usize + payload.len() <= head,
        RW_REPLACE => payload.len() <= head,
        RW_ANSWER | RW_REPLY_REPLACE => payload.len() <= HEAD_CONTROL,
        _ => true,
    };
    if !fits {
        return false;
    }
    match action {
        RW_PASS | RW_PATCH | RW_REPLACE => true,
        RW_DROP => !ctl && !any,
        RW_ANSWER | RW_STALL | RW_NAK | RW_REPLY_PATCH | RW_REPLY_REPLACE => ctl,
        _ => false,
    }
}

// Which (op, class pair) a transform can take, mirroring transform_pair_ok in the firmware.
fn transform_pair_ok(op: u8, sc: u8, si: u16, dc: u8, di: u16) -> bool {
    // Neither op takes one field as both ends: a move needs two.
    if sc == dc && si == di {
        return false;
    }
    match op {
        TF_SWAP => sc == CATCH_CLS_AXIS && dc == CATCH_CLS_AXIS,
        TF_REMAP => {
            (sc == CATCH_CLS_AXIS && dc == CATCH_CLS_AXIS)
                || (sc == CATCH_CLS_BTN && dc == CATCH_CLS_BTN)
                || (sc == CATCH_CLS_BTN && dc == CATCH_CLS_KEY)
                || (sc == CATCH_CLS_BTN && dc == CATCH_CLS_MEDIA)
        }
        _ => false,
    }
}

// RESP(TRANSFORMS): [16][flags][n] then n × [op][sclass][sid u16][dclass][did u16]. No state
// byte per entry: a readback row is always a live one, hardcoded state 1 when it rebuilds.
fn transforms_resp_payload(st: &State) -> Vec<u8> {
    let mut p = vec![
        Q_TRANSFORMS,
        if st.transform_full { TF_F_FULL } else { 0x00 },
        st.transforms.len() as u8,
    ];
    for t in &st.transforms {
        p.push(t.op);
        p.push(t.sclass);
        p.extend_from_slice(&t.sid.to_le_bytes());
        p.push(t.dclass);
        p.extend_from_slice(&t.did.to_le_bytes());
    }
    p
}

// RESP(REWRITE): [12][flags][gen][n] then n × [cls][id u16][dir][action][mlen][off u16][plen u16][hits u16].
fn rewrite_resp_payload(st: &State) -> Vec<u8> {
    let mut p = vec![
        Q_REWRITE,
        if st.rewrite_full { 0x01 } else { 0x00 },
        st.rewrite_gen,
        st.rewrites.len() as u8,
    ];
    for r in &st.rewrites {
        p.push(r.class);
        p.extend_from_slice(&r.id.to_le_bytes());
        p.push(r.dir);
        p.push(r.action);
        p.push(r.match_bytes.len() as u8);
        p.extend_from_slice(&r.offset.to_le_bytes());
        p.extend_from_slice(&(r.payload.len() as u16).to_le_bytes());
        p.extend_from_slice(&r.hits.to_le_bytes());
    }
    p
}

// RESP(REWRITE_ENTRY): [13][index] then the rule in the REWRITE command's shape with state = 1.
fn rewrite_entry_resp_payload(st: &State, index: u8) -> Vec<u8> {
    let mut p = vec![Q_REWRITE_ENTRY, index];
    if let Some(r) = st.rewrites.get(index as usize) {
        p.push(r.class);
        p.extend_from_slice(&r.id.to_le_bytes());
        p.push(r.dir);
        p.push(1); // state
        p.push(r.action);
        p.extend_from_slice(&r.offset.to_le_bytes());
        p.push(r.match_bytes.len() as u8);
        p.extend_from_slice(&r.match_bytes);
        p.extend_from_slice(&r.mask);
        p.extend_from_slice(&r.payload);
    }
    p
}

// RESP(PATCHES): [14][flags][n] then n × [section][cfg][index][offset u16][len u16].
fn patches_resp_payload(st: &State) -> Vec<u8> {
    // applied = the clone serves a non-empty patched set; pending = the stored set is not that set.
    let mut flags = 0u8;
    if !st.patch_presented.is_empty() {
        flags |= 0x01;
    }
    if st.patches != st.patch_presented {
        flags |= 0x02;
    }
    if st.patch_refused {
        flags |= 0x04;
    }
    if st.patch_full {
        flags |= 0x08;
    }
    let mut p = vec![Q_PATCHES, flags, st.patches.len() as u8];
    for q in &st.patches {
        p.push(q.section);
        p.push(q.cfg);
        p.push(q.index);
        p.extend_from_slice(&q.offset.to_le_bytes());
        p.extend_from_slice(&(q.bytes.len() as u16).to_le_bytes());
    }
    p
}

// RESP(PATCH_ENTRY): [15][index] then [section][cfg][index][offset u16][bytes].
fn patch_entry_resp_payload(st: &State, index: u8) -> Vec<u8> {
    let mut p = vec![Q_PATCH_ENTRY, index];
    if let Some(q) = st.patches.get(index as usize) {
        p.push(q.section);
        p.push(q.cfg);
        p.push(q.index);
        p.extend_from_slice(&q.offset.to_le_bytes());
        p.extend_from_slice(&q.bytes);
    }
    p
}

fn motion_event_payload(ts_us: u32, dx: i16, dy: i16, dz: i16, dpan: i16) -> Vec<u8> {
    let mut p = Vec::with_capacity(13);
    p.extend_from_slice(&ts_us.to_le_bytes());
    p.push(0); // clk: a motion event only exists for a real device's report

    p.extend_from_slice(&dx.to_le_bytes());
    p.extend_from_slice(&dy.to_le_bytes());
    p.extend_from_slice(&dz.to_le_bytes());
    p.extend_from_slice(&dpan.to_le_bytes());
    p
}

fn usage_event_payload(
    ts_us: u32,
    class: Class,
    direction: Direction,
    usages: &[Usage],
) -> Vec<u8> {
    // ctrl_usage_append stops at CTRL_USAGE_EVENT_MAX.
    let usages = &usages[..usages.len().min(USAGE_EVENT_MAX)];
    let mut p = Vec::with_capacity(8 + 3 * usages.len());
    p.extend_from_slice(&ts_us.to_le_bytes());
    p.push(0); // clk: host chip, as for motion
    p.push(class.as_u8());
    p.push(direction.as_u8());
    p.push(usages.len() as u8);
    for u in usages {
        u.push_le(&mut p);
    }
    p
}

/// Scriptable fake box for hardware-free tests (feature = `mock`).
#[derive(Clone, Debug)]
pub struct MockBox {
    state: Arc<Mutex<State>>,
    transport: Arc<MockTransport>,
}

// The box's side of an update session, sequenced as the firmware does; a handler replying OK to
// everything would pass every deliberate break.
#[derive(Debug, Clone, Default)]
pub(crate) struct MockUpdate {
    pub(crate) active: bool,
    pub(crate) size: u32,
    pub(crate) got: u32,
    pub(crate) next_seq: u16,
    pub(crate) since_ack: u16,
    pub(crate) sha: [u8; 32],
    pub(crate) hasher: Vec<u8>,
    pub(crate) staged: bool,
    pub(crate) data_seq: u8,
}

impl MockUpdate {
    #[cfg(test)]
    pub(crate) fn begin_for_test(&mut self, body: &[u8]) -> (u8, u32) {
        self.begin(body)
    }

    #[cfg(test)]
    pub(crate) fn data_for_test(&mut self, body: &[u8]) -> Option<(u8, u32)> {
        self.data(body)
    }

    fn begin(&mut self, body: &[u8]) -> (u8, u32) {
        if body.len() < 36 {
            return (0x1A, 0);
        }
        if self.active {
            return (0x10, 0);
        }
        let size = u32::from_le_bytes([body[0], body[1], body[2], body[3]]);
        if size == 0 || size > MOCK_SLOT_SIZE {
            return (0x12, MOCK_SLOT_SIZE);
        }
        self.active = true;
        self.staged = false;
        self.size = size;
        self.got = 0;
        self.next_seq = 0;
        self.since_ack = 0;
        self.hasher.clear();
        self.sha.copy_from_slice(&body[4..36]);
        (0x01, 16)
    }

    // `Some((status, arg))` only when the box owes a reply, as in the firmware: a chunk inside an open
    // window is written unacknowledged.
    fn data(&mut self, body: &[u8]) -> Option<(u8, u32)> {
        // Length before state, and BAD_STATE names the op it wanted, exactly as the firmware does.
        if body.len() < 3 {
            return Some((0x1A, 1));
        }
        if !self.active {
            return Some((0x1A, 0));
        }
        let seq = u16::from_le_bytes([body[0], body[1]]);
        let bytes = &body[2..];
        if self.next_seq > 0 && seq == self.next_seq.wrapping_sub(1) {
            return Some((0x02, u32::from(self.next_seq)));
        }
        if seq != self.next_seq {
            let want = u32::from(self.next_seq);
            self.active = false;
            return Some((0x13, want));
        }
        // The firmware separates these: a chunk outside 1..=504 is BAD_STATE naming the chunk size,
        // and only an overrun of the declared image is TOO_BIG.
        if bytes.is_empty() || bytes.len() > 504 {
            self.active = false;
            return Some((0x1A, 504));
        }
        if self.got + bytes.len() as u32 > self.size {
            self.active = false;
            return Some((0x12, self.size));
        }
        self.hasher.extend_from_slice(bytes);
        self.got += bytes.len() as u32;
        self.next_seq = self.next_seq.wrapping_add(1);
        self.since_ack += 1;
        if self.since_ack >= 16 || self.got == self.size {
            self.since_ack = 0;
            return Some((0x02, u32::from(self.next_seq)));
        }
        None
    }

    fn end(&mut self) -> (u8, u32) {
        if !self.active {
            return (0x1A, 0);
        }
        if self.got != self.size {
            self.active = false;
            return (0x1A, self.size - self.got);
        }
        let digest: [u8; 32] = Sha256::digest(&self.hasher).into();
        self.active = false;
        if digest != self.sha {
            return (0x15, 0);
        }
        self.staged = true;
        (0x03, self.size)
    }
}

pub(crate) const MOCK_SLOT_SIZE: u32 = 0xF_0000;

fn firmware_payload(dev: &Version, staged: bool) -> Vec<u8> {
    let mut p = vec![Q_FIRMWARE, dev.fw_major, dev.fw_minor, dev.fw_patch, 0, 2];
    p.extend_from_slice(&[1, dev.fw_major, dev.fw_minor, dev.fw_patch, 0, 2]);
    p.extend_from_slice(&MOCK_SLOT_SIZE.to_le_bytes());
    p.push(u8::from(staged));
    p
}

impl Default for MockBox {
    fn default() -> Self {
        Self::new()
    }
}

impl MockBox {
    /// Mock box with default config that records commands and replies to `QUERY`.
    pub fn new() -> Self {
        let state = Arc::new(Mutex::new(State::default()));
        let responder_state = Arc::clone(&state);

        let transport = Arc::new(MockTransport::with_responder(move |ty, seq, payload| {
            let mut st = responder_state.lock();
            st.recorded.push(DecodedFrame {
                ty,
                seq,
                payload: payload.to_vec(),
            });
            // The first frame after a boot is answered after a hello, as ctrl_link's first contact.
            let mut lead = Vec::new();
            if !st.pc_seen {
                st.pc_seen = true;
                lead = encode(FrameType::Resp, 0, &version_payload(&st.version)).expect("fits");
            }
            st.advance(std::time::Instant::now());
            // A command may set session state: marked before it runs and again after (ctrl_cmd.c). A
            // RESET releases everything, so it marks nothing.
            let command = ty != FrameType::Query && ty != FrameType::Reset;
            #[cfg(test)]
            if command && std::mem::take(&mut st.teardown_before_command) {
                st.tear_down();
            }
            #[cfg(test)]
            if ty == FrameType::Query
                && payload.first() == Some(&crate::protocol::opcode::Q_CLIP)
                && std::mem::take(&mut st.link_lost_before_clip_query)
            {
                st.count_release();
                st.release_session();
            }
            if command {
                st.session_dirty = true;
            }
            match ty {
                FrameType::Lock => st.apply_lock_frame(payload),
                FrameType::Option => st.apply_option_frame(payload),
                FrameType::Rewrite => st.apply_rewrite_frame(payload),
                FrameType::Patch => st.apply_patch_frame(payload),
                FrameType::Transform => st.apply_transform_frame(payload),
                FrameType::ClipTrigger => st.apply_clip_trigger_frame(payload),
                // The ring takes an append once a clone is up (clip_clock_ready), and CLEAR empties it.
                FrameType::ClipAppend if st.clone_up => {
                    st.clip.total = st.clip.total.saturating_add(payload.len() as u32);
                }
                FrameType::ClipCtrl if payload.first() == Some(&CLIP_OP_CLEAR) => {
                    st.clip.total = 0;
                    st.clip.played = 0;
                    st.clip.state = ClipState::Idle;
                }
                // RESET clears every lock along with the injection, as input_reset does. The bearing
                // option is NVS-backed and survives it. The rewrite table clears too (§3.14).
                // A short frame does nothing, the way the box gates every command on its length.
                FrameType::Reset if !payload.is_empty() => {
                    st.table = LockTable::default();
                    if !st.rewrites.is_empty() {
                        st.rewrites.clear();
                        st.rewrite_gen = st.rewrite_gen.wrapping_add(1);
                    }
                    st.rewrite_full = false;
                    // The transform table clears on RESET too (§3.15); the patch store does not.
                    st.transforms.clear();
                    st.transform_full = false;
                    // The clip config is soft state and goes with the locks.
                    st.clear_clip_config();
                    st.count_release();
                    // With RST_F_NVS the stored half goes as well and the box reboots into its
                    // defaults, so the options that otherwise survive a RESET do not.
                    if payload[0] & RST_F_NVS != 0 {
                        st.reset_persistent();
                        st.restart();
                    }
                }
                FrameType::RebootDl
                    if payload.first() == Some(&RebootTarget::DeviceRun.as_u8()) =>
                {
                    st.restart()
                }
                _ => {}
            }
            let out: Vec<u8> = 'reply: {
                if ty == FrameType::Update && st.respond {
                    let Some(&op) = payload.first() else {
                        return Vec::new();
                    };
                    let body = if payload.len() > 2 {
                        &payload[2..]
                    } else {
                        &[][..]
                    };
                    let answer = match op {
                        0 => Some(st.update.begin(body)),
                        1 => st.update.data(body),
                        2 => Some(st.update.end()),
                        3 => {
                            let seq = st.update.data_seq;
                            st.update = MockUpdate::default();
                            st.update.data_seq = seq;
                            Some((0x00, 0))
                        }
                        4 => {
                            if st.update.staged {
                                let seq = st.update.data_seq;
                                st.update = MockUpdate::default();
                                st.update.data_seq = seq;
                                Some((0x00, 0))
                            } else {
                                Some((0x19, 0))
                            }
                        }
                        _ => None,
                    };
                    break 'reply match answer {
                        Some((status, arg)) => {
                            let mut p = vec![op, payload.get(1).copied().unwrap_or(0), status];
                            p.extend_from_slice(&arg.to_le_bytes());
                            // A DATA acknowledgement answers a whole window, so the firmware gives
                            // it a rolling SEQ of its own rather than echoing the command's.
                            let rseq = if op == 1 {
                                let v = st.update.data_seq;
                                st.update.data_seq = st.update.data_seq.wrapping_add(1);
                                v
                            } else {
                                seq
                            };
                            encode(FrameType::UpdateResp, rseq, &p).expect("resp fits")
                        }
                        None => Vec::new(),
                    };
                }
                if ty == FrameType::Transfer && st.respond {
                    // TRANSFER_RESP [ep][status][IN data], SEQ echoes. The box answers 0xFC (refused)
                    // while the opt-in is off; otherwise the canned reply the test scripted.
                    let ep = payload.first().copied().unwrap_or(0);
                    let (status, mut data) = if st.imperfect.allowed {
                        st.transfer_reply.clone()
                    } else {
                        (0xFC, Vec::new())
                    };
                    // The box sets in_len = 0 unless status == 0 (usbdev_transfer): a non-OK answer
                    // carries no IN data, so drop any the test scripted alongside a failing status.
                    if status != 0 {
                        data.clear();
                    }
                    let mut p = vec![ep, status];
                    p.extend_from_slice(&data);
                    break 'reply encode(FrameType::TransferResp, seq, &p).expect("resp fits");
                }
                if ty == FrameType::Query && st.respond {
                    match payload.first().copied() {
                        Some(0) => encode(FrameType::Resp, seq, &version_payload(&st.version))
                            .expect("resp fits"),
                        Some(1) => {
                            // HEALTH is a u16 LE since proto 7; rewrite_on/patch_on/transform_on reflect live state.
                            let mut h = st.health;
                            h.rewrite_on |= !st.rewrites.is_empty();
                            h.patch_on |= !st.patch_presented.is_empty();
                            h.transform_on |= !st.transforms.is_empty();
                            let f = h.to_flags().to_le_bytes();
                            encode(FrameType::Resp, seq, &[1, f[0], f[1]]).expect("resp fits")
                        }
                        Some(2) => {
                            encode(FrameType::Resp, seq, &device_info_payload(&st.device_info))
                                .expect("resp fits")
                        }
                        Some(3) => {
                            // With no clone every field reads zero, as usbdev_clone_caps leaves them.
                            let caps = if st.clone_up {
                                st.caps
                            } else {
                                Caps::default()
                            };
                            encode(FrameType::Resp, seq, &caps_payload(caps)).expect("resp fits")
                        }
                        Some(4) => {
                            encode(FrameType::Resp, seq, &rate_payload(st.rate)).expect("resp fits")
                        }
                        Some(5) => encode(FrameType::Resp, seq, &stats_payload(st.stats))
                            .expect("resp fits"),
                        Some(6) => {
                            let locks = st
                                .locks
                                .clone()
                                .unwrap_or_else(|| st.table.pack(st.bearing.mode));
                            encode(FrameType::Resp, seq, &locks_payload(&locks)).expect("resp fits")
                        }
                        Some(7) => encode(FrameType::Resp, seq, &catch_resp_payload(&st.catch))
                            .expect("resp fits"),
                        Some(9) => match payload.get(1).copied() {
                            Some(OPT_IMPERFECT) => encode(
                                FrameType::Resp,
                                seq,
                                &options_imperfect_payload(st.imperfect),
                            )
                            .expect("resp fits"),
                            Some(OPT_MOVE_RIDE) => encode(
                                FrameType::Resp,
                                seq,
                                &options_move_ride_payload(st.move_ride_ms),
                            )
                            .expect("resp fits"),
                            Some(OPT_BEARING) => {
                                encode(FrameType::Resp, seq, &options_bearing_payload(st.bearing))
                                    .expect("resp fits")
                            }
                            Some(OPT_EMIT) => encode(
                                FrameType::Resp,
                                seq,
                                &options_emit_payload(
                                    st.emit_pace,
                                    st.render_mode,
                                    st.render_ready,
                                    st.emit_force_hz,
                                    st.advertised_hz,
                                    st.imperfect.allowed,
                                ),
                            )
                            .expect("resp fits"),
                            Some(OPT_RENDER) => encode(
                                FrameType::Resp,
                                seq,
                                &options_render_payload(
                                    st.render_mode,
                                    st.render_full,
                                    st.render_ready,
                                ),
                            )
                            .expect("resp fits"),
                            Some(OPT_SPREAD) => encode(
                                FrameType::Resp,
                                seq,
                                &options_spread_payload(st.spread_percent, st.spread_learned_us),
                            )
                            .expect("resp fits"),
                            _ => Vec::new(),
                        },
                        Some(11) => encode(
                            FrameType::Resp,
                            seq,
                            &firmware_payload(&st.version, st.update.staged),
                        )
                        .expect("resp fits"),
                        Some(10) => encode(
                            FrameType::Resp,
                            seq,
                            &clip_status_payload(&st.clip, &st.clip_settings, &st.packet_triggers),
                        )
                        .expect("resp fits"),
                        Some(12) => encode(FrameType::Resp, seq, &rewrite_resp_payload(&st))
                            .expect("resp fits"),
                        Some(13) => encode(
                            FrameType::Resp,
                            seq,
                            &rewrite_entry_resp_payload(&st, payload.get(1).copied().unwrap_or(0)),
                        )
                        .expect("resp fits"),
                        Some(14) => encode(FrameType::Resp, seq, &patches_resp_payload(&st))
                            .expect("resp fits"),
                        Some(15) => encode(
                            FrameType::Resp,
                            seq,
                            &patch_entry_resp_payload(&st, payload.get(1).copied().unwrap_or(0)),
                        )
                        .expect("resp fits"),
                        Some(16) => encode(FrameType::Resp, seq, &transforms_resp_payload(&st))
                            .expect("resp fits"),
                        _ => Vec::new(),
                    }
                } else {
                    Vec::new()
                }
            };
            // A frame that rebooted the chip leaves it booted clean.
            if command && st.pc_seen {
                st.session_dirty = true;
            }
            let unsolicited = std::mem::take(&mut st.unsolicited);
            let out = [lead, out, unsolicited].concat();
            // Kept so a test can assert the box's replies, not just the host's requests.
            if !out.is_empty() {
                st.replied.push(out.clone());
            }
            out
        }));

        MockBox { state, transport }
    }

    /// Set the [`Version`] answered to `QUERY(VERSION)` (builder style).
    #[must_use]
    pub fn with_version(self, version: Version) -> Self {
        self.state.lock().version = version;
        self
    }

    /// Set the [`Health`] answered to `QUERY(HEALTH)` (builder style).
    #[must_use]
    pub fn with_health(self, health: Health) -> Self {
        self.state.lock().health = health;
        self
    }

    /// Set the [`DeviceInfo`] answered to `QUERY(DEVICE_INFO)` (builder style).
    #[must_use]
    pub fn with_device_info(self, device_info: DeviceInfo) -> Self {
        self.state.lock().device_info = device_info;
        self
    }

    /// Set the whole [`Caps`] answered to `QUERY(CAPS)` (builder style).
    #[must_use]
    pub fn with_caps(self, caps: Caps) -> Self {
        self.state.lock().caps = caps;
        self
    }

    /// Set just the mouse half of the [`Caps`] answered to `QUERY(CAPS)` (builder style).
    #[must_use]
    pub fn with_mouse_caps(self, mouse: MouseCaps) -> Self {
        self.state.lock().caps.mouse = mouse;
        self
    }

    /// Set the keyboard half of the [`Caps`] answered to `QUERY(CAPS)`, marking the keyboard class
    /// change-driven.
    #[must_use]
    pub fn with_kbd_caps(self, keyboard: KbdCaps) -> Self {
        let mut st = self.state.lock();
        st.caps.keyboard = keyboard;
        st.caps.kbd_change_driven = true;
        drop(st);
        self
    }

    /// Set the [`Rate`] answered to `QUERY(RATE)` (builder style).
    #[must_use]
    pub fn with_rate(self, rate: Rate) -> Self {
        self.state.lock().rate = rate;
        self
    }

    /// Set the [`Stats`] answered to `QUERY(STATS)` (builder style).
    #[must_use]
    pub fn with_stats(self, stats: Stats) -> Self {
        self.state.lock().stats = stats;
        self
    }

    /// Pin the [`Locks`] answered to `QUERY(LOCKS)` (builder style), for a reply the mock's lock
    /// table would never build. Otherwise it answers from that table, which `LOCK` frames maintain as
    /// on the box.
    #[must_use]
    pub fn with_locks(self, locks: Locks) -> Self {
        self.state.lock().locks = Some(locks);
        self
    }

    /// Pin the [`Locks`] answered to `QUERY(LOCKS)` in place; see [`with_locks`](Self::with_locks).
    pub fn set_locks(&self, locks: Locks) {
        self.state.lock().locks = Some(locks);
    }

    /// Set the [`CatchState`] answered to `QUERY(CATCH)` (builder style).
    #[must_use]
    pub fn with_catch_state(self, catch: CatchState) -> Self {
        self.state.lock().catch = catch;
        self
    }

    /// Set the [`ImperfectStatus`] answered to `QUERY(OPTIONS, IMPERFECT)` (builder style).
    /// With the opt-in off the mock drops its consuming clip packet triggers, as the box does when
    /// `OPTION(IMPERFECT)` goes off.
    #[must_use]
    pub fn with_imperfect_status(self, imperfect: ImperfectStatus) -> Self {
        self.state.lock().script_imperfect(imperfect);
        self
    }

    /// Set the movement-riding window answered to `QUERY(OPTIONS, MOVE_RIDE)` (builder style);
    /// `None` = off.
    #[must_use]
    pub fn with_movement_riding(self, window: Option<std::time::Duration>) -> Self {
        self.state.lock().move_ride_ms = crate::device::options::ride_window_ms(window);
        self
    }

    /// Update the configured [`Version`] in place (e.g. mid-test).
    pub fn set_version(&self, version: Version) {
        self.state.lock().version = version;
    }

    /// Update the configured [`Health`] in place (e.g. to simulate the mouse attaching).
    pub fn set_health(&self, health: Health) {
        self.state.lock().health = health;
    }

    /// Update the configured [`ImperfectStatus`] in place (e.g. to simulate an over-capacity device).
    /// With the opt-in off the mock drops its consuming clip packet triggers, as the box does when
    /// `OPTION(IMPERFECT)` goes off.
    pub fn set_imperfect_status(&self, imperfect: ImperfectStatus) {
        self.state.lock().script_imperfect(imperfect);
    }

    /// Imperfect-clone opt-in, which gates the advanced control layer (§3.14) (builder style);
    /// shorthand for scripting [`ImperfectStatus::allowed`] before a `raw`/`transfer`/`rewrite`.
    /// With the opt-in off the mock drops its consuming clip packet triggers, as the box does when
    /// `OPTION(IMPERFECT)` goes off.
    pub fn with_imperfect(self, allow: bool) -> Self {
        {
            let mut st = self.state.lock();
            let imperfect = ImperfectStatus {
                allowed: allow,
                ..st.imperfect
            };
            st.script_imperfect(imperfect);
        }
        self
    }

    /// Canned `(status, IN data)` reply to a `TRANSFER` while the opt-in is on (builder style). With
    /// the opt-in off the mock replies `0xFC` (refused).
    pub fn with_transfer_reply(self, status: u8, data: &[u8]) -> Self {
        {
            let mut st = self.state.lock();
            st.transfer_reply = (status, data.to_vec());
        }
        self
    }

    /// Set the canned `TRANSFER` reply after construction.
    pub fn set_transfer_reply(&self, status: u8, data: &[u8]) {
        self.state.lock().transfer_reply = (status, data.to_vec());
    }

    /// Update the configured movement-riding window in place; `None` = off.
    pub fn set_movement_riding(&self, window: Option<std::time::Duration>) {
        self.state.lock().move_ride_ms = crate::device::options::ride_window_ms(window);
    }

    /// Set the [`Bearing`] answered to `QUERY(OPTIONS, BEARING)` (builder style).
    #[must_use]
    pub fn with_bearing(self, bearing: Bearing) -> Self {
        self.state.lock().bearing = bearing;
        self
    }

    /// Update the configured [`Bearing`] answered to `QUERY(OPTIONS, BEARING)` in place.
    pub fn set_bearing(&self, bearing: Bearing) {
        self.state.lock().bearing = bearing;
    }

    /// Set the [`EmitPace`] answered to `QUERY(OPTIONS, EMIT)` (builder style).
    #[must_use]
    pub fn with_emit_pace(self, pace: EmitPace) -> Self {
        self.state.lock().emit_pace = pace;
        self
    }

    /// Update the configured [`EmitPace`] answered to `QUERY(OPTIONS, EMIT)` in place.
    pub fn set_emit_pace(&self, pace: EmitPace) {
        self.state.lock().emit_pace = pace;
    }

    /// Set what `QUERY(OPTIONS, RENDER)` answers (builder style).
    #[must_use]
    pub fn with_render(self, mode: RenderMode, full: bool) -> Self {
        self.set_render(mode, full);
        self
    }

    /// Whether the mock reports a learned profile, which gates rendering on a real box.
    #[must_use]
    pub fn with_render_ready(self, ready: bool) -> Self {
        self.set_render_ready(ready);
        self
    }

    /// Update what `QUERY(OPTIONS, RENDER)` answers in place.
    pub fn set_render(&self, mode: RenderMode, full: bool) {
        let mut st = self.state.lock();
        st.render_mode = mode;
        st.render_full = full;
    }

    /// Update the learned-profile flag in place.
    pub fn set_render_ready(&self, ready: bool) {
        self.state.lock().render_ready = ready;
    }

    /// Learned command period in microseconds (builder style). 0 is a box that has not seen enough
    /// `MOVE`s, which answers a span of 0 whatever the percent.
    #[must_use]
    pub fn with_spread_learned(self, period_us: u32) -> Self {
        self.set_spread_learned(period_us);
        self
    }

    /// Update the learned command period in place.
    pub fn set_spread_learned(&self, period_us: u32) {
        self.state.lock().spread_learned_us = period_us;
    }

    /// Set the forced wire rate answered to `QUERY(OPTIONS, EMIT)` (builder style); `Some(0)` is off.
    #[must_use]
    pub fn with_rate_force(self, force_hz: Option<u16>) -> Self {
        self.set_rate_force(force_hz);
        self
    }

    /// Update the forced wire rate answered to `QUERY(OPTIONS, EMIT)` in place; `Some(0)` is off.
    pub fn set_rate_force(&self, force_hz: Option<u16>) {
        self.state.lock().emit_force_hz = force_hz.filter(|hz| *hz != 0);
    }

    /// Unforced rate the clone advertises, in Hz; 0 (the default) means no clone.
    #[must_use]
    pub fn with_advertised_hz(self, hz: u16) -> Self {
        self.state.lock().advertised_hz = hz;
        self
    }

    /// Set the [`ClipStatus`] answered to `QUERY(CLIP)` (builder style).
    #[must_use]
    pub fn with_clip_status(self, clip: ClipStatus) -> Self {
        self.state.lock().clip = clip;
        self
    }

    /// Update the [`ClipStatus`] answered to `QUERY(CLIP)` in place (e.g. the ring draining).
    pub fn set_clip_status(&self, clip: ClipStatus) {
        self.state.lock().clip = clip;
    }

    /// Set the [`ClipSettings`] answered to `QUERY(CLIP)` (builder style).
    ///
    /// Packet triggers are bound in order, as [`bind_packet`](crate::ClipHandle::bind_packet) binds
    /// them, under the opt-in held when scripted. The mock keeps those the box would take, each with
    /// its scripted `hits`, and omits the rest as the box would: a direction the class never
    /// carries, a match bit outside the mask, a run with no condition,
    /// [`consume`](crate::ClipPacketTrigger::consume) on `Control` or with the opt-in off, and entries
    /// past the count or the match pool. Script the opt-in before a consuming trigger. The held
    /// triggers are the set `bind_packet` adds to and [`clip_packet`](Self::clip_packet) runs.
    #[must_use]
    pub fn with_clip_settings(self, settings: ClipSettings) -> Self {
        self.state.lock().script_clip_settings(settings);
        self
    }

    /// Update the [`ClipSettings`] answered to `QUERY(CLIP)` in place, as
    /// [`with_clip_settings`](Self::with_clip_settings) scripts them.
    pub fn set_clip_settings(&self, settings: ClipSettings) {
        self.state.lock().script_clip_settings(settings);
    }

    /// Run one packet through the packet triggers, as the box does for a packet crossing `class` at
    /// `id` in `direction`. The most specific trigger `head` matches counts it in `hits`. Returns
    /// that trigger's action on this packet and whether it consumes the packet; the action is `None`
    /// when no trigger matches, or the trigger is
    /// [`once_per_run`](crate::ClipPacketTrigger::once_per_run) and the packet continues a run.
    ///
    /// A packet travels [`IN`](Direction::IN) or [`OUT`](Direction::OUT) on a surface carrying that
    /// flow: `IN` for [`HidIn`](TrafficClass::HidIn) and [`Emit`](TrafficClass::Emit), `OUT` for
    /// [`HidOut`](TrafficClass::HidOut), either for the vendor classes and
    /// [`Control`](TrafficClass::Control). Any other `class` and `direction` returns `(None, false)`,
    /// counts in no `hits` and leaves every run unchanged.
    pub fn clip_packet(
        &self,
        class: TrafficClass,
        id: u16,
        direction: Direction,
        head: &[u8],
    ) -> (Option<ClipAction>, bool) {
        if !ClipPacketTrigger::class_carries(class, direction) {
            return (None, false);
        }
        let (verb, consumed) =
            self.state
                .lock()
                .packet_triggers
                .packet(class.as_u8(), id, direction.as_u8(), head);
        (verb.and_then(ClipAction::from_u8), consumed)
    }

    /// Simulate a device-chip restart: session state goes (locks, rules, transforms, the clip),
    /// stored state stays, the box says hello now and on the next frame, and the clone is back 100 ms
    /// later. `RESET` with its store flag and
    /// [`RebootTarget::DeviceRun`](crate::RebootTarget::DeviceRun) restart it too.
    pub fn restart(&self) {
        let hello = {
            let mut st = self.state.lock();
            st.restart();
            let hello = std::mem::take(&mut st.unsolicited);
            st.replied.push(hello.clone());
            hello
        };
        self.transport.push_bytes(&hello);
    }

    /// Simulate an inter-chip link drop and return: the box releases host-set session state (counted
    /// in [`Stats::session`](crate::Stats::session)) and the clone stays up.
    pub fn link_lost(&self) {
        let mut st = self.state.lock();
        st.advance(std::time::Instant::now());
        st.count_release();
        st.release_session();
    }

    /// Simulate the real device detaching: the box releases host-set session state at once. With
    /// `back_within_grace` the device re-attaches inside the 250 ms grace and the clone stays up;
    /// otherwise the clone is torn down when the grace ends (counted again only if a command arrived
    /// during it) and stays down until [`attach`](Self::attach).
    pub fn detach(&self, back_within_grace: bool) {
        let mut st = self.state.lock();
        st.advance(std::time::Instant::now());
        st.count_release();
        st.release_session();
        if !back_within_grace {
            st.grace_until = Some(std::time::Instant::now() + DETACH_GRACE);
            st.device_back = false;
        }
    }

    /// Simulate the device re-attaching. Inside a detach's grace the clone is unchanged; after the
    /// teardown a fresh clone starts, with nothing to release.
    pub fn attach(&self) {
        let mut st = self.state.lock();
        st.advance(std::time::Instant::now());
        if st.grace_until.is_some() {
            st.device_back = true;
        } else if !st.clone_up {
            st.clone_up = true;
            st.patch_presented = st.patches_to_serve();
            st.patch_refused = false;
        }
    }

    // A slower main loop presenting the clone after an opt-in toggle, so a test can separate the
    // release's second part from the first one's recovery.
    #[cfg(test)]
    pub(crate) fn set_represent_delay(&self, delay: std::time::Duration) {
        self.state.lock().represent_delay = delay;
    }

    // The inter-chip link lost just before the next `QUERY(CLIP)` is answered.
    #[cfg(test)]
    pub(crate) fn link_lost_before_next_clip_query(&self) {
        self.state.lock().link_lost_before_clip_query = true;
    }

    // A detach whose teardown lands just before the next command, nothing marked since the detach
    // counted: the window a re-send can fall into.
    #[cfg(test)]
    pub(crate) fn detach_torn_down_before_next_command(&self) {
        let mut st = self.state.lock();
        st.count_release();
        st.release_session();
        st.teardown_before_command = true;
    }

    /// Unresponsive box (builder style): records commands, never replies to a `QUERY`.
    #[must_use]
    pub fn silent(self) -> Self {
        self.state.lock().respond = false;
        self
    }

    /// Inject raw bytes into the host's inbound stream, as if from the box.
    pub fn push_raw(&self, bytes: &[u8]) {
        self.transport.push_bytes(bytes);
    }

    /// Push a `LOG` line as if from the box; it surfaces on `logs()`.
    pub fn push_log(&self, level: LogLevel, text: &str) {
        // The only bound on LOG text is the frame's, less the level byte (emit_log_frame's 160-byte
        // buffer is one emitter's). Cut in bytes, as the box copies; a split char decodes lossily.
        let n = text.len().min(crate::protocol::opcode::MAX_PAYLOAD - 1);
        let mut payload = Vec::with_capacity(1 + n);
        payload.push(level.as_u8());
        payload.extend_from_slice(&text.as_bytes()[..n]);
        self.transport.push_frame(FrameType::Log, 0, &payload);
    }

    /// Push a `MOTION_EVENT` as if from the box; surfaces as
    /// [`CatchEvent::Motion`](crate::CatchEvent). `ts_us` is the raw wire timestamp, so a test can
    /// drive the `u32` wrap and the clock restart. The axes are X, Y, wheel and AC Pan (`dpan`).
    pub fn push_motion(&self, seq: u8, ts_us: u32, dx: i16, dy: i16, dz: i16, dpan: i16) {
        self.transport.push_frame(
            FrameType::MotionEvent,
            seq,
            &motion_event_payload(ts_us, dx, dy, dz, dpan),
        );
    }

    /// Push a `TRAFFIC_EVENT` as if from the box; surfaces as a `Traffic` catch event. `true_len`
    /// may exceed `bytes.len()`, as in a snaplen-truncated capture.
    #[allow(clippy::too_many_arguments)]
    pub fn push_traffic(
        &self,
        seq: u8,
        ts_us: u32,
        clock: ClockDomain,
        class: CatchClass,
        id: u16,
        direction: Direction,
        flags: u8,
        true_len: u16,
        bytes: &[u8],
    ) {
        let mut p = Vec::with_capacity(12 + bytes.len());
        p.extend_from_slice(&ts_us.to_le_bytes());
        p.push(match clock {
            ClockDomain::HostChip => 0,
            ClockDomain::DeviceChip => 1,
        });
        p.push(class.as_u8());
        p.extend_from_slice(&id.to_le_bytes());
        p.push(direction.as_u8());
        p.push(flags);
        p.extend_from_slice(&true_len.to_le_bytes());
        // ctrl_pack_traffic_event cuts the copy at CTRL_TRAFFIC_DATA_MAX; `true_len` keeps the
        // pre-cut length, so a truncated capture describes itself.
        p.extend_from_slice(&bytes[..bytes.len().min(TRAFFIC_DATA_MAX)]);
        self.transport.push_frame(FrameType::TrafficEvent, seq, &p);
    }

    /// Push a `USAGE_EVENT` (held-usage snapshot); surfaces as
    /// [`CatchEvent::Usages`](crate::CatchEvent). `ts_us` is the raw wire timestamp, as for
    /// [`push_motion`](Self::push_motion). `class` travels in the frame, so a test can push the empty
    /// snapshot (release of the last held usage) and still name its class. `direction` is the edge
    /// that produced it: the subscribed set grew or shrank.
    pub fn push_usages(
        &self,
        seq: u8,
        ts_us: u32,
        class: Class,
        direction: Direction,
        usages: &[Usage],
    ) {
        self.transport.push_frame(
            FrameType::UsageEvent,
            seq,
            &usage_event_payload(ts_us, class, direction, usages),
        );
    }

    /// Every reply frame the mock sent, decoded, in order: the command log omits replies, and the
    /// reply `SEQ` is what a client must not correlate on.
    pub fn replied_frames(&self) -> Vec<DecodedFrame> {
        let mut out = Vec::new();
        let mut dec = crate::protocol::FrameDecoder::new();
        for bytes in self.state.lock().replied.iter() {
            dec.feed(bytes, |f| out.push(f));
        }
        out
    }

    /// Every command the host has sent so far, decoded, in order.
    pub fn recorded_frames(&self) -> Vec<DecodedFrame> {
        self.state.lock().recorded.clone()
    }

    /// Commands recorded so far.
    pub fn recorded(&self) -> usize {
        self.state.lock().recorded.len()
    }

    /// Whether the host has sent at least one frame of the given [`FrameType`].
    pub fn saw(&self, ty: FrameType) -> bool {
        self.state.lock().recorded.iter().any(|f| f.ty == ty)
    }

    /// Clear the command log (e.g. to assert only on commands after setup).
    pub fn clear_recorded(&self) {
        self.state.lock().recorded.clear();
    }

    pub(crate) fn transport(&self) -> Arc<dyn crate::transport::Transport> {
        Arc::clone(&self.transport) as Arc<dyn crate::transport::Transport>
    }
}

impl crate::Device {
    /// [`Device`](crate::Device) over a [`MockBox`], without the handshake.
    pub fn with_mock(mock: MockBox) -> crate::Device {
        crate::Device::from_transport(mock.transport())
    }

    /// [`Device`](crate::Device) over a [`MockBox`], with the version handshake.
    pub fn open_mock(mock: MockBox) -> crate::Result<crate::Device> {
        crate::Device::open_transport(mock.transport())
    }
}
