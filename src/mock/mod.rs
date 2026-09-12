//! Scriptable fake box (feature = `mock`) for hardware-free testing.

use std::sync::Arc;

use parking_lot::Mutex;

use crate::protocol::opcode::{
    CAP_PAN, CAP_REPORT_ID, CAP_WHEEL, CAP_X, CAP_Y, CAPS_CD_KBD, CAPS_CD_MOUSE, DI_HAS_BOS,
    DI_HAS_SERIAL, KBC_CONSUMER, KBC_NKRO, KBC_REPORT_ID, KBC_SYSTEM, LOCK_AXIS_PAN, LOCK_CLS_AXIS,
    LOCK_CLS_BTN, LOCK_CLS_KEY, LOCK_CLS_MEDIA, LOCK_DIR_AGAINST, LOCK_DIR_BOTH, LOCK_DIR_NEG,
    LOCK_DIR_POS, LOCK_DIR_WITH, LOCK_ID_ALL, LOCK_SCALE_BLOCK, LOCK_SCALE_PASS, MAX_BUTTONS,
    OPT_BEARING, OPT_EMIT, OPT_IMPERFECT, OPT_MOVE_RIDE, OPT_NAME, OPT_RENDER, OPT_SPREAD,
    Q_FIRMWARE, RATE_CONFIDENT,
};
use crate::protocol::opcode::{
    CATCH_CLS_AXIS, CATCH_CLS_BTN, CATCH_CLS_KEY, CATCH_CLS_MEDIA, Q_TRANSFORMS, TF_F_FULL,
    TF_INVERT, TF_REMAP, TF_SCALE, TF_SWAP, TRANSFORM_MAX_ENTRIES,
};
use crate::protocol::opcode::{
    CLIP_CFG_F_FINALIZED, CLIP_CFG_F_LOOP, CLIP_CFG_F_RETAIN, CLIP_CFG_F_RIDE, CLIP_TRIG_MAX,
    CLK_RATE_NONE, PATCH_APPLY, PATCH_CLEAR, PATCH_MAX_ENTRIES, Q_PATCH_ENTRY, Q_PATCHES,
    Q_REWRITE, Q_REWRITE_ENTRY, REWRITE_MATCH_MAX, REWRITE_MAX_ENTRIES,
};
use crate::protocol::{DecodedFrame, FrameType, encode};
use crate::types::PatchSection;
use sha2::{Digest, Sha256};

use crate::transport::mock::MockTransport;
use crate::types::lock::blanket_scope;
use crate::types::{
    Axis, Bearing, BearingMode, Caps, CatchClass, CatchState, Class, ClipSettings, ClipState,
    ClipStatus, ClockDomain, DeviceInfo, DeviceKind, Direction, EmitPace, Health, ImperfectStatus,
    KbdCaps, LockEntry, LockScope, LockTarget, Locks, LogLevel, MouseCaps, Rate, RenderMode, Stats,
    Usage, Version,
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
    /// Whether the box has learned a profile. A real box arms this off native motion, so a mock
    /// starts unarmed and a test that needs the armed path says so.
    render_ready: bool,
    spread_percent: u16,
    /// The command period the box has learned off MOVE arrivals, in microseconds. 0 is a box that has
    /// not seen enough of them, which is where every session starts.
    spread_learned_us: u32,
    emit_force_hz: Option<u16>,
    advertised_hz: u16,
    clip: ClipStatus,
    clip_settings: ClipSettings,
    // The rewrite table the REWRITE frames build, modelled the way the box holds it (keyed rows, a
    // monotonic gen, a full flag) so the mock answers RESP(REWRITE)/RESP(REWRITE_ENTRY) like a box.
    rewrites: Vec<MockRewrite>,
    rewrite_gen: u8,
    rewrite_full: bool,
    // The patch store the PATCH frames build, plus its apply state. `pending` and `full` are not held
    // here: patches_resp_payload derives them from the store the way usbdev_pack_patches does.
    patches: Vec<MockPatch>,
    patch_applied: bool,
    patch_refused: bool,
    // The field-transform table the TRANSFORM frames build, modelled the way the box holds it (keyed
    // rows in installation order, a full flag). Ungated: unlike rewrites it is not cleared when the
    // imperfect opt-in goes off, because a transform is faithful and never needed it.
    transforms: Vec<MockTransform>,
    transform_full: bool,
    // The canned answer to a TRANSFER (status, IN data). The box answers 0xFC when the opt-in is off.
    transfer_reply: (u8, Vec<u8>),
    recorded: Vec<DecodedFrame>,
    respond: bool,
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

// One transform-table row the mock holds, in wire fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MockTransform {
    op: u8,
    sclass: u8,
    sid: u16,
    dclass: u8,
    did: u16,
    scale: i16,
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
            // A plain five-button mouse by default, so the lock table's button cap agrees with the
            // count `RESP(CAPS)` reports; a test wanting buttons past five or AC Pan sets its own caps.
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
            stats: Stats::from_payload(&[5, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0])
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
            rewrites: Vec::new(),
            rewrite_gen: 0,
            rewrite_full: false,
            patches: Vec::new(),
            patch_applied: false,
            patch_refused: false,
            transforms: Vec::new(),
            transform_full: false,
            transfer_reply: (0x00, Vec::new()),
            recorded: Vec::new(),
            respond: true,
        }
    }
}

// The box's lock table, modelled the way the firmware holds it so the mock answers `RESP(LOCKS)` the
// way a box would rather than echoing what the host sent. Mouse rows are X, Y, wheel, pan then the
// buttons; slots are POS, NEG, WITH, AGAINST.
const LOCK_TGT_BTN_BASE: usize = 4; // CTRL_LOCK_TGT_BTN_BASE: 4 axes (X, Y, wheel, pan) precede the buttons
const LOCK_TGT_COUNT: usize = LOCK_TGT_BTN_BASE + MAX_BUTTONS as usize; // 4 axes + 16 buttons
const LOCK_SLOT_WITH: usize = 2;
const SLOT_DIRS: [u8; 4] = [LOCK_DIR_POS, LOCK_DIR_NEG, LOCK_DIR_WITH, LOCK_DIR_AGAINST];
// CTRL_RESP_LOCKS_MAXN and INPUT_MEDIA_MAX: past either the box drops silently.
const RESP_LOCKS_MAXN: usize = 96;
const MEDIA_LOCK_MAX: usize = 8;
// The rest of ctrl_proto.h's reply bounds. Every one of these sits behind a public builder that
// takes a caller-supplied length, and the box truncates at each rather than refusing: it appends
// what fits and answers. Encoding past them writes a count byte that wrapped past 255, or a payload
// longer than a frame carries, and since the responder runs inside `write_all`, that `encode`
// failure unwinds back out of the caller's own query rather than answering it.
const NAME_MAX: usize = 32; // CTRL_NAME_MAX
const DEVICE_INFO_PRODUCT_MAX: usize = 127; // CTRL_DEVICE_INFO_PRODUCT_MAX
const CATCH_MAXN: usize = 32; // CTRL_CATCH_MAXN
const USAGE_EVENT_MAX: usize = 40; // CTRL_USAGE_EVENT_MAX
const CLIP_HELD_MAX: usize = USAGE_EVENT_MAX; // CTRL_CLIP_HELD_MAX, defined as CTRL_USAGE_EVENT_MAX
const TRAFFIC_DATA_MAX: usize = 180; // CTRL_TRAFFIC_DATA_MAX

#[derive(Debug, Clone)]
pub(crate) struct LockTable {
    mouse: [[u8; 4]; LOCK_TGT_COUNT],
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
    fn set_mouse(&mut self, target: usize, dir: u8, scale: u8) {
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
                // A button has no bearing, so a named relative direction on one is refused outright;
                // Both reaches the relative pair with a pass, never with the scale, since the two
                // multiply.
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

    // `n_buttons` is the clone's declared button count: a button blanket writes that many rows and a
    // button id past it is dropped, exactly as the firmware caps at `nbtn`.
    pub(crate) fn apply(&mut self, class: u8, id: u16, dir: u8, scale: u8, n_buttons: u8) {
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
                    // The blanket carries the two edge slots only, and honours the direction: a
                    // relative one names neither and is dropped.
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
                // A media usage is suppressed whole, so the direction byte is not read at all.
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

    // In vector mode one relative scale governs both axes, the lower of X's and Y's, so the
    // readback names that number on both axes instead of each axis's stored byte.
    fn reported(&self, t: usize, slot: usize, vector: bool) -> u8 {
        let sc = self.mouse[t][slot];
        if !vector || slot < LOCK_SLOT_WITH || t > Axis::Y.as_u16() as usize {
            return sc;
        }
        sc.min(self.mouse[1 - t][slot])
    }

    pub(crate) fn pack(&self, mode: BearingMode) -> Locks {
        let vector = mode == BearingMode::Vector;
        let mut out: Vec<LockEntry> = Vec::new();
        let mut push = |scope: LockScope, direction: u8, scale: u8| {
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
        // Media before granular keys: media is bounded at MEDIA_LOCK_MAX and granular keys are not,
        // so enumerating keys last is what keeps the unbounded class from crowding the bounded one out at
        // the entry cap. A media usage is suppressed whole, so the direction it reports is Both.
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
        // Granular keys last, on whatever is left of the cap. Past it they truncate silently (the
        // reply has nowhere to say so), which is why nothing bounded is enumerated after them.
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
        if p.len() < 5 {
            return;
        }
        let n_buttons = self.caps.mouse.n_buttons;
        self.table.apply(
            p[0],
            u16::from_le_bytes([p[1], p[2]]),
            p[3],
            p[4],
            n_buttons,
        );
    }

    // Apply a REWRITE frame the way the box would: keyed add/overwrite/remove, a monotonic gen, a
    // whole-table clear, and the caps that raise `full`. Dropped whole while the opt-in is off.
    fn apply_rewrite_frame(&mut self, p: &[u8]) {
        if !self.imperfect.allowed {
            return; // the box drops a REWRITE frame with the opt-in off
        }
        if p.len() < 9 {
            return;
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
        if mlen > REWRITE_MATCH_MAX || p.len() < 9 + 2 * mlen {
            return; // the box refuses an over-long or truncated match
        }
        let match_bytes = p[9..9 + mlen].to_vec();
        let mask = p[9 + mlen..9 + 2 * mlen].to_vec();
        let payload = p[9 + 2 * mlen..].to_vec();
        let key = (cls, id, dir, match_bytes.clone(), mask.clone());
        let pos = self.rewrites.iter().position(|r| r.key() == key);
        if state == 0 {
            if let Some(i) = pos {
                self.rewrites.remove(i);
                self.rewrite_gen = self.rewrite_gen.wrapping_add(1);
            }
            return;
        }
        match pos {
            Some(i) => {
                // An identical re-set does not bump gen (§3.14); the box's keepalive relies on it.
                let same = self.rewrites[i].action == action
                    && self.rewrites[i].offset == offset
                    && self.rewrites[i].payload == payload;
                if same {
                    return;
                }
                let hits = self.rewrites[i].hits;
                self.rewrites[i] = MockRewrite {
                    class: cls,
                    id,
                    dir,
                    action,
                    offset,
                    match_bytes,
                    mask,
                    payload,
                    hits,
                };
                self.rewrite_gen = self.rewrite_gen.wrapping_add(1);
            }
            None => {
                if self.rewrites.len() >= REWRITE_MAX_ENTRIES {
                    self.rewrite_full = true;
                    return;
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
            }
        }
    }

    // Apply a TRANSFORM frame (§3.15), modelled on transform_tab_set. Ungated: a transform is faithful,
    // so unlike REWRITE this runs whatever the imperfect opt-in. The refusals mirror the firmware: an
    // op at or above the count, a class pair the op cannot take, a scale of 0 on an INVERT, a field
    // neither map declares, and the eight-entry ceiling (which sets the full flag).
    fn apply_transform_frame(&mut self, p: &[u8]) {
        if p.len() < 10 {
            return;
        }
        let op = p[0];
        let sclass = p[1];
        let sid = u16::from_le_bytes([p[2], p[3]]);
        let dclass = p[4];
        let did = u16::from_le_bytes([p[5], p[6]]);
        let scale = i16::from_le_bytes([p[7], p[8]]);
        let state = p[9];
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
            }
            return;
        }
        // state 1: add or overwrite, after the same admissibility gauntlet the box runs.
        if op > TF_SCALE
            || !transform_pair_ok(op, sclass, sid, dclass, did)
            || (op == TF_INVERT && scale == 0)
            || !self.transform_field_present(sclass, sid)
            || !self.transform_field_present(dclass, did)
        {
            return;
        }
        match pos {
            Some(i) => {
                // The key matches: only the op and scale change, keeping the row's position.
                self.transforms[i].op = op;
                self.transforms[i].scale = scale;
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
                    scale,
                });
            }
        }
    }

    // Whether the bound clone declares this field, mirroring transform_field_present over RESP(CAPS):
    // an axis is present when its flag is set, a button when its id is under the declared count and the
    // box's ceiling, a key when a keyboard collection is bound, media when a consumer collection is.
    fn transform_field_present(&self, cls: u8, id: u16) -> bool {
        match cls {
            CATCH_CLS_AXIS => match id {
                0 => self.caps.mouse.has_x,
                1 => self.caps.mouse.has_y,
                2 => self.caps.mouse.has_wheel,
                3 => self.caps.mouse.pan,
                _ => false,
            },
            CATCH_CLS_BTN => id < self.caps.mouse.n_buttons as u16 && id < MAX_BUTTONS as u16,
            CATCH_CLS_KEY => self.caps.keyboard.n_keys > 0,
            CATCH_CLS_MEDIA => self.caps.keyboard.has_consumer,
            _ => false,
        }
    }

    // Apply a PATCH frame: APPLY (only under the opt-in), CLEAR, or a keyed store (kept whatever the
    // opt-in, as the box does; empty bytes removes the patch at that key).
    fn apply_patch_frame(&mut self, p: &[u8]) {
        let Some(&section) = p.first() else {
            return;
        };
        match section {
            PATCH_APPLY => {
                if self.imperfect.allowed {
                    self.patch_applied = true;
                    self.patch_refused = false;
                }
                return;
            }
            PATCH_CLEAR => {
                self.patches.clear();
                self.patch_applied = false;
                self.patch_refused = false;
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
            }
            return;
        }
        match pos {
            Some(i) => {
                self.patches[i] = MockPatch {
                    section,
                    cfg,
                    index,
                    offset,
                    bytes,
                }
            }
            None => {
                if self.patches.len() >= PATCH_MAX_ENTRIES {
                    return; // the box refuses a store past PATCH_MAX; full is derived from the count
                }
                self.patches.push(MockPatch {
                    section,
                    cfg,
                    index,
                    offset,
                    bytes,
                });
            }
        }
    }

    fn apply_option_frame(&mut self, p: &[u8]) {
        if p.is_empty() {
            return;
        }
        match (p.first().copied(), &p[1..]) {
            (Some(OPT_IMPERFECT), [allow, ..]) => {
                self.imperfect.allowed = *allow != 0;
                if !self.imperfect.allowed {
                    // Opt-off clears the rewrite table (usbdev_set_imperfect_allowed) so nothing in this
                    // layer rewrites while the clone is faithful-only; gen stays monotonic across the clear.
                    if !self.rewrites.is_empty() {
                        self.rewrites.clear();
                        self.rewrite_gen = self.rewrite_gen.wrapping_add(1);
                    }
                    self.rewrite_full = false;
                    // The clone re-presents without the opt-in, so a stored set stops being shown.
                    self.patch_applied = false;
                }
            }
            (Some(OPT_MOVE_RIDE), [lo, hi, ..]) => {
                self.move_ride_ms = u16::from_le_bytes([*lo, *hi])
            }
            (Some(OPT_EMIT), [mode, lo, hi, flo, fhi, ..]) => {
                // The box discards the whole command on a mode it does not know, force_hz included,
                // and answers nothing. Coercing here would model a box that does not exist.
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
    // usbdev_box_name_copy stops at CTRL_NAME_MAX, so a longer name reads back cut. Bytes, not
    // chars, as the box copies them: a split multi-byte char decodes lossily, which is what the box
    // would put on the wire too.
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
    p
}

fn locks_payload(l: &Locks) -> Vec<u8> {
    use crate::protocol::opcode::{LOCK_CLS_AXIS, LOCK_ID_ALL};
    use crate::types::{LockScope, LockTarget};
    // The box stops appending at RESP_LOCKS_MAXN and answers with what fit (ctrl_locks_append), so a
    // longer `Locks` truncates here. Encoding all of them would write a count byte that wrapped past
    // 255 and a payload no frame can carry, which fails the caller's query instead of answering it.
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
        p.push(e.scale);
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
    // `render` is not echoed here any more (it has its own option), but the rendered gate still
    // decides the resolved rate, so the pace reply still depends on it.
    // Mirror the firmware: Fixed clamps the echoed rate to 1..=1000 (0 -> 1000) and snaps resolved
    // to the 1 ms frame clock (1000/n); Learned/Interval echo 0 (no real device to resolve).
    let (mode, fixed_hz, mut resolved) = match pace {
        EmitPace::Learned => (0u8, 0u16, 0u16),
        EmitPace::Interval => (1, 0, 0),
        EmitPace::Fixed(h) => {
            let hz = if h == 0 { 1000 } else { h.min(1000) };
            let n = (((1_000_000u32 / hz as u32) + 500) / 1000).max(1);
            (2, hz, (1000 / n) as u16)
        }
    };
    // The texture rides its own option beside the pace, but only forces resolved to 1 kHz when the
    // pace resolved to no period of its own; a Fixed rate keeps its snapped value. That is the
    // firmware's condition, not "the pace is Learned": usbdev.c's emit_override_period returns 0 for
    // Interval too while no device is bound, which is the state this mock models. The box also gates
    // it on a profile having ARMED, not on the mode being set: until then it runs the paced fill and
    // reports 0. A mock that answered 1000 regardless would green-light host code that reads
    // resolved_hz as "the renderer is emitting".
    if render != RenderMode::Off && render_ready && resolved == 0 {
        resolved = 1000;
    }
    // The box resolves a forced rate to a bInterval in whole 1 ms frames and advertises 1000/n, so a
    // request that is not a divisor of 1000 comes back as something else. A naive echo would diverge.
    // A force only applies with the imperfect opt-in on; without it the clone still advertises its own.
    let (advertised, active) = match force_hz.filter(|hz| *hz != 0 && allowed) {
        None => (native_hz, false),
        Some(hz) => {
            // Mirror rate_force_binterval: a host rounds a full-speed interval down to a power of two,
            // so the box only ever advertises one of those. Echoing the request would diverge.
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

// The box resolves the interval from a command period it has learned off MOVE arrivals, and answers 0
// while it has none or the option is off. A mock that answered a span from the percent alone would
// model a friendlier box than the hardware and green-light host code reading it as "spreading".
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

fn clip_status_payload(c: &ClipStatus, cfg: &ClipSettings) -> Vec<u8> {
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
    p
}

// Which (op, class pair) a transform can take, mirroring transform_pair_ok in the firmware.
fn transform_pair_ok(op: u8, sc: u8, si: u16, dc: u8, di: u16) -> bool {
    match op {
        TF_INVERT | TF_SCALE => sc == CATCH_CLS_AXIS && dc == CATCH_CLS_AXIS && si == di,
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

// RESP(TRANSFORMS): [16][flags][n] then n × [op][sclass][sid u16][dclass][did u16][scale i16]. No state
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
        p.extend_from_slice(&t.scale.to_le_bytes());
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
    // pending and full are derived at pack time exactly as usbdev_pack_patches does, not held stickily:
    // pending = (n && !applied), mutually exclusive with applied; full = (n >= PATCH_MAX).
    let mut flags = 0u8;
    if st.patch_applied {
        flags |= 0x01;
    }
    if !st.patches.is_empty() && !st.patch_applied {
        flags |= 0x02;
    }
    if st.patch_refused {
        flags |= 0x04;
    }
    if st.patches.len() >= PATCH_MAX_ENTRIES {
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

/// A scriptable fake medius box for hardware-free tests (feature = `mock`).
#[derive(Clone, Debug)]
pub struct MockBox {
    state: Arc<Mutex<State>>,
    transport: Arc<MockTransport>,
}

// The box's side of an update session, so a transfer against the mock exercises the same sequencing
// the firmware does. A handler that just answered OK would let every deliberate break pass.
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

    // Returns `Some((status, arg))` only when the box owes an answer, exactly like the firmware:
    // a chunk inside an open window is written and not acknowledged.
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
    /// Create a mock box with default config that records commands and auto-answers `QUERY`.
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
            match ty {
                FrameType::Lock => st.apply_lock_frame(payload),
                FrameType::Option => st.apply_option_frame(payload),
                FrameType::Rewrite => st.apply_rewrite_frame(payload),
                FrameType::Patch => st.apply_patch_frame(payload),
                FrameType::Transform => st.apply_transform_frame(payload),
                // RESET clears every lock along with the injection, as input_reset does. The bearing
                // option is NVS-backed and survives it. The rewrite table clears too (§3.14).
                FrameType::Reset => {
                    st.table = LockTable::default();
                    if !st.rewrites.is_empty() {
                        st.rewrites.clear();
                        st.rewrite_gen = st.rewrite_gen.wrapping_add(1);
                    }
                    st.rewrite_full = false;
                    // The transform table clears on RESET too (§3.15); the patch store does not.
                    st.transforms.clear();
                    st.transform_full = false;
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
                            // A DATA acknowledgement answers a whole window, so the firmware gives it a
                            // rolling SEQ of its own rather than echoing the command's. Echoing it here
                            // would let a client that correlated on SEQ pass the mock and fail on the box.
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
                            // HEALTH is a u16 LE (proto 7); rewrite_on/patch_on/transform_on reflect live state.
                            let mut h = st.health;
                            h.rewrite_on |= !st.rewrites.is_empty();
                            h.patch_on |= st.patch_applied;
                            h.transform_on |= !st.transforms.is_empty();
                            let f = h.to_flags().to_le_bytes();
                            encode(FrameType::Resp, seq, &[1, f[0], f[1]]).expect("resp fits")
                        }
                        Some(2) => {
                            encode(FrameType::Resp, seq, &device_info_payload(&st.device_info))
                                .expect("resp fits")
                        }
                        Some(3) => {
                            encode(FrameType::Resp, seq, &caps_payload(st.caps)).expect("resp fits")
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
                            &clip_status_payload(&st.clip, &st.clip_settings),
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
            // Kept so a test can assert what the box ANSWERED, not just what the host asked.
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

    /// Set the keyboard half of the [`Caps`] answered to `QUERY(CAPS)`, marking the keyboard class change-driven.
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

    /// Pin the [`Locks`] answered to `QUERY(LOCKS)` (builder style), for a reply the mock's own lock
    /// table would never build. Without one it answers from that table, which the `LOCK` frames it
    /// receives maintain the way the box maintains its own.
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
    #[must_use]
    pub fn with_imperfect_status(self, imperfect: ImperfectStatus) -> Self {
        self.state.lock().imperfect = imperfect;
        self
    }

    /// Set the movement-riding window answered to `QUERY(OPTIONS, MOVE_RIDE)` (builder style); `None` = off.
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
    pub fn set_imperfect_status(&self, imperfect: ImperfectStatus) {
        self.state.lock().imperfect = imperfect;
    }

    /// Enable or disable the imperfect-clone opt-in the advanced control layer (§3.14) is gated on (builder
    /// style). A shorthand for scripting [`ImperfectStatus::allowed`] before a `raw`/`transfer`/`rewrite`.
    pub fn with_imperfect(self, allow: bool) -> Self {
        {
            let mut st = self.state.lock();
            st.imperfect.allowed = allow;
        }
        self
    }

    /// Set the canned `(status, IN data)` a `TRANSFER` is answered with while the opt-in is on
    /// (builder style). With the opt-in off the mock answers `0xFC` (refused) regardless.
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

    /// Set whether the mock reports a learned profile, which is what gates rendering on a real box.
    #[must_use]
    pub fn with_render_ready(self, ready: bool) -> Self {
        self.set_render_ready(ready);
        self
    }

    /// Update what `QUERY(OPTIONS, RENDER)` answers in place, like every other option's setter.
    pub fn set_render(&self, mode: RenderMode, full: bool) {
        let mut st = self.state.lock();
        st.render_mode = mode;
        st.render_full = full;
    }

    /// Update the learned-profile flag in place.
    pub fn set_render_ready(&self, ready: bool) {
        self.state.lock().render_ready = ready;
    }

    /// Set the command period the mock has learned, in microseconds (builder style). 0 is a box that
    /// has not seen enough `MOVE`s yet, which answers a span of 0 whatever the percent is.
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

    /// Set the rate the mock's clone advertises unforced, in Hz; 0 is the default and means no clone.
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

    /// Update the [`ClipStatus`] answered to `QUERY(CLIP)` in place (e.g. to simulate the ring draining).
    pub fn set_clip_status(&self, clip: ClipStatus) {
        self.state.lock().clip = clip;
    }

    /// Set the [`ClipSettings`] answered to `QUERY(CLIP)` (builder style).
    #[must_use]
    pub fn with_clip_settings(self, settings: ClipSettings) -> Self {
        self.state.lock().clip_settings = settings;
        self
    }

    /// Update the [`ClipSettings`] answered to `QUERY(CLIP)` in place.
    pub fn set_clip_settings(&self, settings: ClipSettings) {
        self.state.lock().clip_settings = settings;
    }

    /// Make the box unresponsive (builder style): it records commands but never answers a `QUERY`.
    #[must_use]
    pub fn silent(self) -> Self {
        self.state.lock().respond = false;
        self
    }

    /// Inject raw bytes into the host's inbound stream, exactly as if the box put them on the wire.
    pub fn push_raw(&self, bytes: &[u8]) {
        self.transport.push_bytes(bytes);
    }

    /// Push a `LOG` line as if the box emitted it; it surfaces on the device's `logs()` channel.
    pub fn push_log(&self, level: LogLevel, text: &str) {
        // The protocol names no bound on LOG text (emit_log_frame's 160-byte line buffer is one
        // emitter's, not the wire's), so the only bound to hold is the frame's own, less the level
        // byte. Bytes, as the box copies them; a split char decodes lossily.
        let n = text.len().min(crate::protocol::opcode::MAX_PAYLOAD - 1);
        let mut payload = Vec::with_capacity(1 + n);
        payload.push(level.as_u8());
        payload.extend_from_slice(&text.as_bytes()[..n]);
        self.transport.push_frame(FrameType::Log, 0, &payload);
    }

    /// Push a `MOTION_EVENT` as if the box emitted it; surfaces as [`CatchEvent::Motion`](crate::CatchEvent).
    /// `ts_us` is the raw wire timestamp, so a test can drive the `u32` wrap and the clock-restart case.
    /// The four axes are X, Y, wheel and AC Pan (`dpan`).
    pub fn push_motion(&self, seq: u8, ts_us: u32, dx: i16, dy: i16, dz: i16, dpan: i16) {
        self.transport.push_frame(
            FrameType::MotionEvent,
            seq,
            &motion_event_payload(ts_us, dx, dy, dz, dpan),
        );
    }

    /// Push a `USAGE_EVENT` (a held-usage snapshot); surfaces as [`CatchEvent::Usages`](crate::CatchEvent).
    /// Push a `TRAFFIC_EVENT` as if the box emitted it (surfaces as a `Traffic` catch event).
    /// `true_len` may exceed `bytes.len()`, which is how a snaplen-truncated capture looks.
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
        // ctrl_pack_traffic_event cuts the copy at CTRL_TRAFFIC_DATA_MAX; `true_len` still names the
        // packet's length before the cut, which is what makes a truncated capture self-describing.
        p.extend_from_slice(&bytes[..bytes.len().min(TRAFFIC_DATA_MAX)]);
        self.transport.push_frame(FrameType::TrafficEvent, seq, &p);
    }

    /// `ts_us` is the raw wire timestamp, as for [`push_motion`](Self::push_motion).
    /// A held-usage snapshot. `class` is carried in the frame rather than inferred, so a test can
    /// push the EMPTY snapshot (the release of the last held usage) and still say which class
    /// went quiet.
    /// `direction` is the edge that produced the snapshot: the subscribed set grew or shrank.
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

    /// Every frame the mock has answered with, decoded, in order. The recorded-command log says
    /// nothing about replies, and the reply `SEQ` is exactly what a client must not correlate on.
    pub fn replied_frames(&self) -> Vec<DecodedFrame> {
        let mut out = Vec::new();
        let mut dec = crate::protocol::FrameDecoder::new();
        for bytes in self.state.lock().replied.iter() {
            dec.feed(bytes, |f| out.push(f));
        }
        out
    }

    /// A snapshot copy of every command the host has sent so far, decoded, in order.
    pub fn recorded_frames(&self) -> Vec<DecodedFrame> {
        self.state.lock().recorded.clone()
    }

    /// The number of commands recorded so far.
    pub fn recorded(&self) -> usize {
        self.state.lock().recorded.len()
    }

    /// Whether the host has sent at least one frame of the given [`FrameType`].
    pub fn saw(&self, ty: FrameType) -> bool {
        self.state.lock().recorded.iter().any(|f| f.ty == ty)
    }

    /// Clear the recorded-command log (e.g. to assert only on commands after a setup phase).
    pub fn clear_recorded(&self) {
        self.state.lock().recorded.clear();
    }

    pub(crate) fn transport(&self) -> Arc<dyn crate::transport::Transport> {
        Arc::clone(&self.transport) as Arc<dyn crate::transport::Transport>
    }
}

impl crate::Device {
    /// Build a [`Device`](crate::Device) driven by a [`MockBox`], without running the handshake.
    pub fn with_mock(mock: MockBox) -> crate::Device {
        crate::Device::from_transport(mock.transport())
    }

    /// Build a [`Device`](crate::Device) over a [`MockBox`] and run the version handshake.
    pub fn open_mock(mock: MockBox) -> crate::Result<crate::Device> {
        crate::Device::open_transport(mock.transport())
    }
}
