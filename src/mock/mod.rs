//! Public scriptable fake box (feature = `mock`) — hardware-free downstream testing (§10).
//!
//! [`MockBox`] is a configurable stand-in for a real medius box, built on the in-memory transport's
//! responder seam. It answers `QUERY` from a configurable `Version`/`Health`, records every command
//! the host sent, and can push `LOG` lines. Matching is on **decoded** frames (semantic), not raw
//! bytes. Cheap to clone (shares state and transport via `Arc`), so a test can keep a handle for
//! assertions after the `Device` owns the transport. Drive a real `Device` over it with
//! [`Device::with_mock`](crate::Device::with_mock) (or [`open_mock`](crate::Device::open_mock) to run
//! the handshake too); see `src/tests/behavior.rs` for worked usage.

use std::sync::Arc;

use parking_lot::Mutex;

use crate::protocol::{DecodedFrame, FrameType, encode};
use crate::transport::mock::MockTransport;
use crate::types::{Health, LogLevel, Version};

/// Configurable, recorded state shared between a [`MockBox`] handle and the responder closure.
#[derive(Debug)]
struct State {
    version: Version,
    health: Health,
    recorded: Vec<DecodedFrame>,
    /// When `false`, the box records commands but never answers a `QUERY` — simulating a hung/crashed
    /// box, so a query times out. Toggled by [`MockBox::silent`].
    respond: bool,
}

impl Default for State {
    fn default() -> Self {
        State {
            // Default proto_ver == PROTO_VER so the handshake passes.
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

/// A scriptable fake medius box for hardware-free tests (feature = `mock`). See the [module docs](self).
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
    /// Create a mock box with default config (proto_ver = the library's supported protocol version,
    /// fw 0.0.0, all-clear health) that records commands and auto-answers `QUERY`.
    pub fn new() -> Self {
        let state = Arc::new(Mutex::new(State::default()));
        let responder_state = Arc::clone(&state);

        // Record every outbound frame; auto-answer QUERY from the configured Version/Health (echoing
        // the SEQ). Other commands are fire-and-go with no reply, matching the real box.
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
                        // RESP(VERSION): [what=0][proto_ver][fw_major][fw_minor][fw_patch]
                        let v = st.version;
                        encode(
                            FrameType::Resp,
                            seq,
                            &[0, v.proto_ver, v.fw_major, v.fw_minor, v.fw_patch],
                        )
                        .expect("resp fits")
                    }
                    Some(1) => {
                        // RESP(HEALTH): [what=1][flags]
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

    /// Make the box **unresponsive** (builder style): it still records commands but never answers a
    /// `QUERY`, so a query against it times out — useful for simulating a hung/crashed box.
    #[must_use]
    pub fn silent(self) -> Self {
        self.state.lock().respond = false;
        self
    }

    /// Inject **raw bytes** into the host's inbound stream, exactly as if the box put them on the wire —
    /// including malformed, truncated, or garbage data. The device's reader decodes them like any other
    /// input (dropping bad-CRC frames, resyncing past garbage), so this drives robustness tests.
    pub fn push_raw(&self, bytes: &[u8]) {
        self.transport.push_bytes(bytes);
    }

    /// Push a `LOG` line as if the box emitted it; it surfaces on the device's
    /// [`logs()`](crate::Device::logs) channel (and, with `tracing`, as a host event).
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

    /// The shared transport as `Arc<dyn Transport>`. `pub(crate)` so the private `Transport` trait
    /// stays hidden; the public seam is [`Device::with_mock`](crate::Device::with_mock).
    pub(crate) fn transport(&self) -> Arc<dyn crate::transport::Transport> {
        Arc::clone(&self.transport) as Arc<dyn crate::transport::Transport>
    }
}

impl crate::Device {
    /// Build a [`Device`](crate::Device) driven by a [`MockBox`] (feature = `mock`).
    ///
    /// The public seam to run the real device stack against the fake box without exposing the private
    /// `Transport` trait. No handshake is run; call
    /// [`query_version`](crate::Device::query_version) explicitly to exercise it.
    pub fn with_mock(mock: MockBox) -> crate::Device {
        crate::Device::from_transport(mock.transport())
    }

    /// Build a [`Device`](crate::Device) over a [`MockBox`] **and run the version handshake** — the
    /// mock counterpart of [`open`](crate::Device::open)/[`find`](crate::Device::find), so the
    /// handshake path (version validation, retry, reject) is testable hardware-free.
    ///
    /// # Errors
    /// Same as [`open`](crate::Device::open): [`NoReply`](crate::Error::NoReply) if the mock is
    /// [`silent`](MockBox::silent), [`BadProtoVer`](crate::Error::BadProtoVer) if its version's
    /// `proto_ver` is unsupported.
    pub fn open_mock(mock: MockBox) -> crate::Result<crate::Device> {
        crate::Device::open_transport(mock.transport())
    }
}
