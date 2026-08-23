//! Scriptable fake box (feature = `mock`) for hardware-free testing.

use std::sync::Arc;

use parking_lot::Mutex;

use crate::protocol::opcode::{
    BTN_COUNT, CAP_REPORT_ID, CAP_WHEEL, CAP_X, CAP_Y, CAPS_CD_KBD, CAPS_CD_MOUSE, DI_HAS_BOS,
    DI_HAS_SERIAL, KBC_CONSUMER, KBC_NKRO, KBC_REPORT_ID, KBC_SYSTEM, LOCK_AXIS_WHEEL,
    LOCK_CLS_AXIS, LOCK_CLS_BTN, LOCK_CLS_KEY, LOCK_CLS_MEDIA, LOCK_DIR_AGAINST, LOCK_DIR_BOTH,
    LOCK_DIR_NEG, LOCK_DIR_POS, LOCK_DIR_WITH, LOCK_ID_ALL, LOCK_SCALE_BLOCK, LOCK_SCALE_PASS,
    OPT_BEARING, OPT_EMIT, OPT_IMPERFECT, OPT_MOVE_RIDE, OPT_NAME, Q_FIRMWARE, RATE_CONFIDENT,
};
use crate::protocol::opcode::{
    CLIP_CFG_F_FINALIZED, CLIP_CFG_F_LOOP, CLIP_CFG_F_RETAIN, CLIP_CFG_F_RIDE, CLIP_TRIG_MAX,
    CLK_RATE_NONE,
};
use crate::protocol::{DecodedFrame, FrameType, encode};
use sha2::{Digest, Sha256};

use crate::transport::mock::MockTransport;
use crate::types::lock::blanket_scope;
use crate::types::{
    Axis, Bearing, BearingMode, Caps, CatchClass, CatchState, Class, ClipSettings, ClipState,
    ClipStatus, ClockDomain, DeviceInfo, DeviceKind, Direction, EmitPace, Health, ImperfectStatus,
    KbdCaps, LockEntry, LockScope, LockTarget, Locks, LogLevel, MouseCaps, Rate, Stats, Usage,
    Version,
};

#[derive(Debug)]
struct State {
    update: MockUpdate,
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
    clip: ClipStatus,
    clip_settings: ClipSettings,
    recorded: Vec<DecodedFrame>,
    respond: bool,
}

impl Default for State {
    fn default() -> Self {
        State {
            update: MockUpdate::default(),
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
            caps: Caps::default(),
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
            clip: ClipStatus::default(),
            clip_settings: ClipSettings::default(),
            recorded: Vec::new(),
            respond: true,
        }
    }
}

// The box's lock table, modelled the way the firmware holds it so the mock answers `RESP(LOCKS)` the
// way a box would rather than echoing what the host sent. Mouse rows are X, Y, wheel then the five
// buttons; slots are POS, NEG, WITH, AGAINST.
const LOCK_TGT_BTN_BASE: usize = 3;
const LOCK_TGT_COUNT: usize = 8;
const LOCK_SLOT_WITH: usize = 2;
const SLOT_DIRS: [u8; 4] = [LOCK_DIR_POS, LOCK_DIR_NEG, LOCK_DIR_WITH, LOCK_DIR_AGAINST];
// CTRL_RESP_LOCKS_MAXN and INPUT_MEDIA_MAX: past either the box drops silently.
const RESP_LOCKS_MAXN: usize = 96;
const MEDIA_LOCK_MAX: usize = 8;
// The rest of ctrl_proto.h's reply bounds. Every one of these sits behind a public builder that
// takes a caller-supplied length, and the box truncates at each rather than refusing: it appends
// what fits and answers. Encoding past them writes a count byte that wrapped past 255, or a payload
// longer than a frame carries -- and since the responder runs inside `write_all`, that `encode`
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
            // One bit is all a button carries, so the box stores the block or pass it will render.
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

    pub(crate) fn apply(&mut self, class: u8, id: u16, dir: u8, scale: u8) {
        let on = scale < LOCK_SCALE_PASS;
        match class {
            LOCK_CLS_AXIS => {
                if id == LOCK_ID_ALL {
                    for t in 0..=LOCK_AXIS_WHEEL as usize {
                        self.set_mouse(t, dir, scale);
                    }
                } else if id <= LOCK_AXIS_WHEEL {
                    self.set_mouse(id as usize, dir, scale);
                }
            }
            LOCK_CLS_BTN => {
                if id == LOCK_ID_ALL {
                    for b in 0..BTN_COUNT as usize {
                        self.set_mouse(LOCK_TGT_BTN_BASE + b, dir, scale);
                    }
                } else if id < BTN_COUNT as u16 {
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

    // In vector mode one relative scale governs the whole aim, the lower of X's and Y's, so the
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
                        _ => Axis::Wheel,
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
        // so enumerating keys last is what keeps the unbounded class from starving the bounded one at
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
        // Granular keys last, on whatever is left of the cap. Past it they truncate silently -- the
        // reply has nowhere to say so -- which is why nothing bounded is enumerated after them.
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
        self.table
            .apply(p[0], u16::from_le_bytes([p[1], p[2]]), p[3], p[4]);
    }

    fn apply_option_frame(&mut self, p: &[u8]) {
        match (p.first().copied(), &p[1..]) {
            (Some(OPT_IMPERFECT), [allow, ..]) => self.imperfect.allowed = *allow != 0,
            (Some(OPT_MOVE_RIDE), [lo, hi, ..]) => {
                self.move_ride_ms = u16::from_le_bytes([*lo, *hi])
            }
            (Some(OPT_EMIT), [mode, lo, hi, ..]) => {
                let hz = u16::from_le_bytes([*lo, *hi]);
                self.emit_pace = match mode {
                    1 => EmitPace::Interval,
                    2 => EmitPace::Fixed(hz),
                    _ => EmitPace::Learned,
                };
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

fn options_emit_payload(pace: EmitPace) -> Vec<u8> {
    // Mirror the firmware: Fixed clamps the echoed rate to 1..=1000 (0 -> 1000) and snaps resolved
    // to the 1 ms frame clock (1000/n); Learned/Interval echo 0 (no real device to resolve).
    let (mode, fixed_hz, resolved) = match pace {
        EmitPace::Learned => (0u8, 0u16, 0u16),
        EmitPace::Interval => (1, 0, 0),
        EmitPace::Fixed(h) => {
            let hz = if h == 0 { 1000 } else { h.min(1000) };
            let n = (((1_000_000u32 / hz as u32) + 500) / 1000).max(1);
            (2, hz, (1000 / n) as u16)
        }
    };
    let mut p = vec![9u8, OPT_EMIT, mode];
    p.extend_from_slice(&fixed_hz.to_le_bytes());
    p.extend_from_slice(&resolved.to_le_bytes());
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

fn motion_event_payload(ts_us: u32, dx: i16, dy: i16, dz: i16) -> Vec<u8> {
    let mut p = Vec::with_capacity(11);
    p.extend_from_slice(&ts_us.to_le_bytes());
    p.push(0); // clk: a motion event only exists for a real device's report

    p.extend_from_slice(&dx.to_le_bytes());
    p.extend_from_slice(&dy.to_le_bytes());
    p.extend_from_slice(&dz.to_le_bytes());
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

/// The box's side of an update session, so a transfer against the mock exercises the same sequencing
/// the firmware does. A handler that just answered OK would let every deliberate break pass.
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

    /// Returns `Some((status, arg))` only when the box owes an answer, exactly like the firmware:
    /// a chunk inside an open window is written and not acknowledged.
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
                // RESET clears every lock along with the injection, as input_reset does. The bearing
                // option is NVS-backed and survives it.
                FrameType::Reset => st.table = LockTable::default(),
                _ => {}
            }
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
                return match answer {
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
            if ty == FrameType::Query && st.respond {
                match payload.first().copied() {
                    Some(0) => encode(FrameType::Resp, seq, &version_payload(&st.version))
                        .expect("resp fits"),
                    Some(1) => {
                        encode(FrameType::Resp, seq, &[1, st.health.to_flags()]).expect("resp fits")
                    }
                    Some(2) => encode(FrameType::Resp, seq, &device_info_payload(&st.device_info))
                        .expect("resp fits"),
                    Some(3) => {
                        encode(FrameType::Resp, seq, &caps_payload(st.caps)).expect("resp fits")
                    }
                    Some(4) => {
                        encode(FrameType::Resp, seq, &rate_payload(st.rate)).expect("resp fits")
                    }
                    Some(5) => {
                        encode(FrameType::Resp, seq, &stats_payload(st.stats)).expect("resp fits")
                    }
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
                        Some(OPT_EMIT) => {
                            encode(FrameType::Resp, seq, &options_emit_payload(st.emit_pace))
                                .expect("resp fits")
                        }
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
                    _ => Vec::new(),
                }
            } else {
                Vec::new()
            }
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
        // The protocol names no bound on LOG text -- emit_log_frame's 160-byte line buffer is one
        // emitter's, not the wire's -- so the only bound to hold is the frame's own, less the level
        // byte. Bytes, as the box copies them; a split char decodes lossily.
        let n = text.len().min(crate::protocol::opcode::MAX_PAYLOAD - 1);
        let mut payload = Vec::with_capacity(1 + n);
        payload.push(level.as_u8());
        payload.extend_from_slice(&text.as_bytes()[..n]);
        self.transport.push_frame(FrameType::Log, 0, &payload);
    }

    /// Push a `MOTION_EVENT` as if the box emitted it; surfaces as [`CatchEvent::Motion`](crate::CatchEvent).
    /// `ts_us` is the raw wire timestamp, so a test can drive the `u32` wrap and the clock-restart case.
    pub fn push_motion(&self, seq: u8, ts_us: u32, dx: i16, dy: i16, dz: i16) {
        self.transport.push_frame(
            FrameType::MotionEvent,
            seq,
            &motion_event_payload(ts_us, dx, dy, dz),
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
    /// push the EMPTY snapshot -- the release of the last held usage -- and still say which class
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
