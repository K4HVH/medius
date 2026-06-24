//! Scriptable fake box (feature = `mock`) for hardware-free testing.

use std::sync::Arc;

use parking_lot::Mutex;

use crate::protocol::opcode::{
    CAP_REPORT_ID, CAP_WHEEL, CAP_X, CAP_Y, CAPS_CD_KBD, CAPS_CD_MOUSE, KBC_CONSUMER, KBC_NKRO,
    KBC_REPORT_ID, KBC_SYSTEM, MI_HAS_BOS, MI_HAS_SERIAL, RATE_CONFIDENT,
};
use crate::protocol::{DecodedFrame, FrameType, encode};
use crate::transport::mock::MockTransport;
use crate::types::{
    Caps, CatchState, Health, KbdCaps, KeyboardEvent, Locks, LogLevel, MediaEvent, MouseCaps,
    MouseEvent, MouseInfo, Rate, Stats, Version,
};

#[derive(Debug)]
struct State {
    version: Version,
    health: Health,
    mouse_info: MouseInfo,
    caps: Caps,
    rate: Rate,
    stats: Stats,
    locks: Locks,
    catch: CatchState,
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
            },
            health: Health::from_flags(0),
            mouse_info: MouseInfo::from_payload(&[2, 0, 0, 0, 0, 0, 0, 0, 0, 0]).unwrap(),
            caps: Caps::default(),
            rate: Rate::from_payload(&[4, 0, 0, 0, 0, 0]).unwrap(),
            stats: Stats::from_payload(&[5, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0])
                .unwrap(),
            locks: Locks::from_payload(&[6, 0, 0]).unwrap(),
            catch: CatchState::from_payload(&[7, 0, 0, 0, 0, 0]).unwrap(),
            recorded: Vec::new(),
            respond: true,
        }
    }
}

fn mouse_info_payload(m: MouseInfo) -> Vec<u8> {
    let mut flags = 0u8;
    if m.has_serial {
        flags |= MI_HAS_SERIAL;
    }
    if m.has_bos {
        flags |= MI_HAS_BOS;
    }
    let mut p = vec![2u8];
    p.extend_from_slice(&m.vid.to_le_bytes());
    p.extend_from_slice(&m.pid.to_le_bytes());
    p.extend_from_slice(&m.bcd_device.to_le_bytes());
    p.extend_from_slice(&m.bcd_usb.to_le_bytes());
    p.push(flags);
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

fn locks_payload(l: Locks) -> Vec<u8> {
    let mut p = vec![6u8];
    p.extend_from_slice(&l.mask().to_le_bytes());
    p
}

fn catch_resp_payload(c: CatchState) -> Vec<u8> {
    let mut p = vec![7u8, c.mask.bits()];
    p.extend_from_slice(&c.dropped.to_le_bytes());
    p
}

fn kb_event_payload(e: &KeyboardEvent) -> Vec<u8> {
    let mut p = vec![e.modifiers, e.keys.len() as u8];
    p.extend(e.keys.iter().map(|k| k.usage()));
    p
}

fn cons_event_payload(e: &MediaEvent) -> Vec<u8> {
    let mut p = vec![e.keys.len() as u8];
    for k in &e.keys {
        p.extend_from_slice(&k.usage().to_le_bytes());
    }
    p
}

fn event_payload(r: MouseEvent) -> Vec<u8> {
    let mut p = Vec::with_capacity(7);
    p.push(r.buttons);
    p.extend_from_slice(&r.dx.to_le_bytes());
    p.extend_from_slice(&r.dy.to_le_bytes());
    p.extend_from_slice(&r.wheel.to_le_bytes());
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

        let transport =
            Arc::new(MockTransport::with_responder(move |ty, seq, payload| {
                let mut st = responder_state.lock();
                st.recorded.push(DecodedFrame {
                    ty,
                    seq,
                    payload: payload.to_vec(),
                });
                if ty == FrameType::Query && st.respond {
                    match payload.first().copied() {
                        Some(0) => {
                            let v = st.version;
                            encode(
                                FrameType::Resp,
                                seq,
                                &[0, v.proto_ver, v.fw_major, v.fw_minor, v.fw_patch],
                            )
                            .expect("resp fits")
                        }
                        Some(1) => encode(FrameType::Resp, seq, &[1, st.health.to_flags()])
                            .expect("resp fits"),
                        Some(2) => encode(FrameType::Resp, seq, &mouse_info_payload(st.mouse_info))
                            .expect("resp fits"),
                        Some(3) => encode(FrameType::Resp, seq, &caps_payload(st.caps))
                            .expect("resp fits"),
                        Some(4) => {
                            encode(FrameType::Resp, seq, &rate_payload(st.rate)).expect("resp fits")
                        }
                        Some(5) => encode(FrameType::Resp, seq, &stats_payload(st.stats))
                            .expect("resp fits"),
                        Some(6) => encode(FrameType::Resp, seq, &locks_payload(st.locks))
                            .expect("resp fits"),
                        Some(7) => encode(FrameType::Resp, seq, &catch_resp_payload(st.catch))
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

    /// Set the [`MouseInfo`] answered to `QUERY(MOUSE_INFO)` (builder style).
    #[must_use]
    pub fn with_mouse_info(self, mouse_info: MouseInfo) -> Self {
        self.state.lock().mouse_info = mouse_info;
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

    /// Set the keyboard half of the [`Caps`] answered to `QUERY(CAPS)` (builder style). Also marks the
    /// keyboard class change-driven, since a keyboard reports only on a key change.
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

    /// Update the configured [`Version`] in place (e.g. mid-test).
    pub fn set_version(&self, version: Version) {
        self.state.lock().version = version;
    }

    /// Update the configured [`Health`] in place (e.g. to simulate the mouse attaching).
    pub fn set_health(&self, health: Health) {
        self.state.lock().health = health;
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

    /// Push an `EVENT` (mouse snapshot) as if the box emitted it; it surfaces on a subscribed
    /// [`EventStream`](crate::EventStream) as [`CatchEvent::Mouse`](crate::CatchEvent). `seq` is the
    /// rolling event counter the host sees as the frame `SEQ`.
    pub fn push_event(&self, seq: u8, report: MouseEvent) {
        self.transport
            .push_frame(FrameType::Event, seq, &event_payload(report));
    }

    /// Push a `KB_EVENT` (keyboard snapshot); surfaces as [`CatchEvent::Keyboard`](crate::CatchEvent).
    pub fn push_kb_event(&self, seq: u8, event: &KeyboardEvent) {
        self.transport
            .push_frame(FrameType::KbEvent, seq, &kb_event_payload(event));
    }

    /// Push a `CONS_EVENT` (media snapshot); surfaces as [`CatchEvent::Media`](crate::CatchEvent).
    pub fn push_cons_event(&self, seq: u8, event: &MediaEvent) {
        self.transport
            .push_frame(FrameType::ConsEvent, seq, &cons_event_payload(event));
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
