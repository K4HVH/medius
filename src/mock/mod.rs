//! Scriptable fake box (feature = `mock`) for hardware-free testing.

use std::sync::Arc;

use parking_lot::Mutex;

use crate::protocol::opcode::{
    CAP_REPORT_ID, CAP_WHEEL, CAP_X, CAP_Y, CAPS_CD_KBD, CAPS_CD_MOUSE, DI_HAS_BOS, DI_HAS_SERIAL,
    KBC_CONSUMER, KBC_NKRO, KBC_REPORT_ID, KBC_SYSTEM, OPT_EMIT, OPT_IMPERFECT, OPT_MOVE_RIDE,
    RATE_CONFIDENT,
};
use crate::protocol::opcode::{CLIP_CFG_F_FINALIZED, CLIP_CFG_F_LOOP, CLIP_CFG_F_RETAIN};
use crate::protocol::{DecodedFrame, FrameType, encode};
use crate::transport::mock::MockTransport;
use crate::types::lock::blanket_scope;
use crate::types::{
    Caps, CatchClass, CatchState, ClipSettings, ClipState, ClipStatus, ClockDomain, DeviceInfo,
    DeviceKind, EmitPace, Health, ImperfectStatus, KbdCaps, LockDirection, Locks, LogLevel,
    MouseCaps, Rate, Stats, Usage, Version,
};

#[derive(Debug)]
struct State {
    version: Version,
    health: Health,
    device_info: DeviceInfo,
    caps: Caps,
    rate: Rate,
    stats: Stats,
    locks: Locks,
    catch: CatchState,
    imperfect: ImperfectStatus,
    move_ride_ms: u16,
    emit_pace: EmitPace,
    clip: ClipStatus,
    clip_settings: ClipSettings,
    recorded: Vec<DecodedFrame>,
    respond: bool,
}

impl Default for State {
    fn default() -> Self {
        State {
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
            locks: Locks::from_payload(&[6, 0, 0]).unwrap(),
            catch: CatchState::from_payload(&[
                7, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xFF, 0xFF, 0,
            ])
            .unwrap(),
            imperfect: ImperfectStatus::default(),
            move_ride_ms: 0,
            emit_pace: EmitPace::Learned,
            clip: ClipStatus::default(),
            clip_settings: ClipSettings::default(),
            recorded: Vec::new(),
            respond: true,
        }
    }
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
    p.extend_from_slice(m.product.as_bytes());
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
    use crate::protocol::opcode::{LOCK_CLS_AXIS, LOCK_DIRBIT_NEG, LOCK_DIRBIT_POS, LOCK_ID_ALL};
    use crate::types::{LockScope, LockTarget};
    let entries = l.entries();
    let mut p = vec![6u8, entries.len() as u8];
    for e in entries {
        let (class, id) = match e.scope {
            LockScope::Blanket(class) => (class.as_u8(), LOCK_ID_ALL),
            LockScope::Target(LockTarget::Axis(a)) => (LOCK_CLS_AXIS, a.as_u16()),
            LockScope::Target(LockTarget::Usage(u)) => u.class_id(),
        };
        let dirbits = (if e.positive { LOCK_DIRBIT_POS } else { 0 })
            | (if e.negative { LOCK_DIRBIT_NEG } else { 0 });
        p.push(class);
        p.extend_from_slice(&id.to_le_bytes());
        p.push(dirbits);
    }
    p
}

fn catch_resp_payload(c: &CatchState) -> Vec<u8> {
    let mut p = vec![7u8, c.table_full as u8];
    p.extend_from_slice(&c.dropped.to_le_bytes());
    p.extend_from_slice(&c.clock.offset_us.to_le_bytes());
    p.extend_from_slice(&c.clock.rate_ppb.to_le_bytes());
    p.extend_from_slice(&c.clock.delay_us.to_le_bytes());
    // 0xFFFF is "no estimate", which a consumer must be able to tell from a zero-age one.
    let age = c
        .clock
        .age
        .map_or(u16::MAX, |d| d.as_millis().min(u16::MAX as u128 - 1) as u16);
    p.extend_from_slice(&age.to_le_bytes());
    p.push(c.entries.len() as u8);
    for e in &c.entries {
        let (class, id) = e.filter.wire();
        p.push(class);
        p.extend_from_slice(&id.to_le_bytes());
        p.push(e.filter.direction.as_u8());
        p.push(e.filter.snaplen);
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
    p.push(c.held.len() as u8);
    for u in &c.held {
        u.push_le(&mut p);
    }
    p.push(blanket_scope(&cfg.autolock));
    let flags = (if cfg.loop_ { CLIP_CFG_F_LOOP } else { 0 })
        | (if cfg.retain { CLIP_CFG_F_RETAIN } else { 0 })
        | (if cfg.finalized {
            CLIP_CFG_F_FINALIZED
        } else {
            0
        });
    p.push(flags);
    p.push(cfg.triggers.len() as u8);
    for t in &cfg.triggers {
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

fn usage_event_payload(ts_us: u32, usages: &[Usage]) -> Vec<u8> {
    let mut p = Vec::with_capacity(6 + 3 * usages.len());
    p.extend_from_slice(&ts_us.to_le_bytes());
    p.push(0); // clk: host chip, as for motion
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
            if ty == FrameType::Query && st.respond {
                match payload.first().copied() {
                    Some(0) => {
                        let v = &st.version;
                        let mut p = vec![0, v.proto_ver, v.fw_major, v.fw_minor, v.fw_patch];
                        p.extend_from_slice(&v.mac);
                        p.extend_from_slice(v.name.as_bytes());
                        encode(FrameType::Resp, seq, &p).expect("resp fits")
                    }
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
                        encode(FrameType::Resp, seq, &locks_payload(&st.locks)).expect("resp fits")
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
                        Some(OPT_EMIT) => {
                            encode(FrameType::Resp, seq, &options_emit_payload(st.emit_pace))
                                .expect("resp fits")
                        }
                        _ => Vec::new(),
                    },
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

    /// Set the [`Locks`] answered to `QUERY(LOCKS)` (builder style).
    #[must_use]
    pub fn with_locks(self, locks: Locks) -> Self {
        self.state.lock().locks = locks;
        self
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
        let mut payload = Vec::with_capacity(1 + text.len());
        payload.push(level.as_u8());
        payload.extend_from_slice(text.as_bytes());
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
        direction: LockDirection,
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
        p.extend_from_slice(bytes);
        self.transport.push_frame(FrameType::TrafficEvent, seq, &p);
    }

    /// `ts_us` is the raw wire timestamp, as for [`push_motion`](Self::push_motion).
    pub fn push_usages(&self, seq: u8, ts_us: u32, usages: &[Usage]) {
        self.transport.push_frame(
            FrameType::UsageEvent,
            seq,
            &usage_event_payload(ts_us, usages),
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
