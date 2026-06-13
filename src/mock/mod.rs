//! Scriptable fake box (feature = `mock`) for hardware-free testing.

use std::sync::Arc;

use parking_lot::Mutex;

use crate::protocol::{DecodedFrame, FrameType, encode};
use crate::transport::mock::MockTransport;
use crate::types::{Health, LogLevel, Version};

#[derive(Debug)]
struct State {
    version: Version,
    health: Health,
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
            recorded: Vec::new(),
            respond: true,
        }
    }
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
                        let v = st.version;
                        encode(
                            FrameType::Resp,
                            seq,
                            &[0, v.proto_ver, v.fw_major, v.fw_minor, v.fw_patch],
                        )
                        .expect("resp fits")
                    }
                    Some(1) => {
                        encode(FrameType::Resp, seq, &[1, st.health.to_flags()]).expect("resp fits")
                    }
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
