//! Public scriptable fake box (feature = `mock`) — hardware-free downstream testing (§10).
//!
//! [`MockBox`] is a configurable, scriptable stand-in for a real medius box, built on the in-memory
//! transport's responder seam. Downstream code (and this crate's own tests) can:
//!
//! - **configure** the `Version` / `Health` it answers queries with ([`MockBox::with_version`] /
//!   [`MockBox::with_health`]),
//! - **drive a real [`Device`](crate::Device)** over it via [`Device::with_mock`](crate::Device::with_mock) — the wrapper that reaches the
//!   private transport **without** exposing the private `Transport` trait,
//! - **record** every command the host sent, for assertions ([`MockBox::recorded`] /
//!   [`MockBox::recorded_frames`]),
//! - **push** `LOG` lines as if the box emitted them ([`MockBox::push_log`]).
//!
//! Like makcu's mock it matches **decoded** frames (semantic), not raw bytes. `MockBox` is cheap to
//! clone (it shares its state and transport via `Arc`), so a test can keep a handle for assertions
//! after the [`Device`](crate::Device) owns the transport.
//!
//! ```
//! # use medius::mock::MockBox;
//! # use medius::{Device, Button, Version};
//! let mock = MockBox::new().with_version(Version { proto_ver: 1, fw_major: 2, fw_minor: 3, fw_patch: 4 });
//! let device = Device::with_mock(mock.clone());
//! let v = device.query_version().unwrap();
//! assert_eq!((v.fw_major, v.fw_minor, v.fw_patch), (2, 3, 4));
//! device.press(Button::Left).unwrap();
//! // The press was recorded.
//! assert!(mock.recorded_frames().iter().any(|f| f.ty == medius::protocol::FrameType::Button));
//! ```

use std::sync::Arc;

use parking_lot::Mutex;

use crate::protocol::types::{Health, LogLevel, Version};
use crate::protocol::{DecodedFrame, FrameType, encode};
use crate::transport::mock::MockTransport;

/// One command the host sent the mock box, as a decoded `(ty, seq, payload)` triple — the recorded
/// form used for assertions.
pub type RecordedFrame = DecodedFrame;

/// The configurable, recorded state shared between a [`MockBox`] handle and the responder closure
/// installed in the transport.
#[derive(Debug)]
struct State {
    /// The `Version` answered to `QUERY(VERSION)`.
    version: Version,
    /// The `Health` answered to `QUERY(HEALTH)`.
    health: Health,
    /// Every decoded outbound frame the host sent, in order.
    recorded: Vec<DecodedFrame>,
}

impl Default for State {
    fn default() -> Self {
        State {
            // A sane default that passes the handshake (proto_ver == PROTO_VER).
            version: Version {
                proto_ver: crate::protocol::PROTO_VER,
                fw_major: 0,
                fw_minor: 0,
                fw_patch: 0,
            },
            // All-clear health by default.
            health: Health::from_flags(0),
            recorded: Vec::new(),
        }
    }
}

/// A scriptable fake medius box for hardware-free tests (feature = `mock`).
///
/// Build one with [`MockBox::new`] + the `with_*` setters, drive a [`Device`](crate::Device) over it
/// with [`Device::with_mock`](crate::Device::with_mock), then assert on [`recorded`](MockBox::recorded) /
/// [`recorded_frames`](MockBox::recorded_frames) and inject diagnostics with
/// [`push_log`](MockBox::push_log). See the [module docs](self) for the full model.
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
    /// Create a mock box with default config (proto_ver = [`PROTO_VER`](crate::protocol::PROTO_VER),
    /// fw 0.0.0, all-clear health) that records commands and auto-answers `QUERY`.
    pub fn new() -> Self {
        let state = Arc::new(Mutex::new(State::default()));
        let responder_state = Arc::clone(&state);

        // The responder is invoked on every decoded outbound frame: record it, and auto-answer a
        // QUERY from the configured Version/Health (echoing the request SEQ). Other commands record
        // only (fire-and-go, no reply) — exactly the real box's behaviour.
        let transport = Arc::new(MockTransport::with_responder(move |ty, seq, payload| {
            let mut st = responder_state.lock();
            st.recorded.push(DecodedFrame {
                ty,
                seq,
                payload: payload.to_vec(),
            });
            if ty == FrameType::Query {
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

    /// Set the [`Version`] the mock answers `QUERY(VERSION)` with (builder style).
    #[must_use]
    pub fn with_version(self, version: Version) -> Self {
        self.state.lock().version = version;
        self
    }

    /// Set the [`Health`] the mock answers `QUERY(HEALTH)` with (builder style).
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

    /// Push a `LOG` line as if the box emitted it; it surfaces on the device's
    /// [`logs()`](crate::Device::logs) channel (and, with `tracing`, as a host event).
    pub fn push_log(&self, level: LogLevel, text: &str) {
        let mut payload = Vec::with_capacity(1 + text.len());
        payload.push(level.as_u8());
        payload.extend_from_slice(text.as_bytes());
        self.transport.push_frame(FrameType::Log, 0, &payload);
    }

    /// A snapshot copy of every command the host has sent so far, decoded, in order.
    pub fn recorded_frames(&self) -> Vec<RecordedFrame> {
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

    /// The shared transport, as the crate-internal `Arc<dyn Transport>` the [`Device`](crate::Device) wraps. Kept
    /// `pub(crate)` so the private `Transport` trait is **not** exposed (the public seam is
    /// [`Device::with_mock`](crate::Device::with_mock)).
    pub(crate) fn transport(&self) -> Arc<dyn crate::transport::Transport> {
        Arc::clone(&self.transport) as Arc<dyn crate::transport::Transport>
    }
}

impl crate::Device {
    /// Build a [`Device`](crate::Device) driven by a [`MockBox`] (feature = `mock`).
    ///
    /// This is the public seam to run the real device stack against the fake box **without** exposing
    /// the private `Transport` trait — it wraps the mock's transport internally. No handshake is run
    /// (the mock is not a real link); call [`query_version`](crate::Device::query_version) explicitly
    /// if you want to exercise it.
    pub fn with_mock(mock: MockBox) -> crate::Device {
        crate::Device::from_transport(mock.transport())
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crate::protocol::FrameType;
    use crate::protocol::types::{Button, Health, LogLevel, Version};

    use super::*;

    /// Downstream-style test: configure a mock box, build a Device, run a query + a command, assert
    /// the recorded commands and the query result.
    #[test]
    fn downstream_flow_query_and_record() {
        let mock = MockBox::new()
            .with_version(Version {
                proto_ver: 1,
                fw_major: 5,
                fw_minor: 6,
                fw_patch: 7,
            })
            .with_health(Health::from_flags(0x0F));
        let device = crate::Device::with_mock(mock.clone());

        // query_version returns the configured version.
        let v = device.query_version().unwrap();
        assert_eq!((v.fw_major, v.fw_minor, v.fw_patch), (5, 6, 7));

        // query_health returns the configured health.
        let h = device.query_health().unwrap();
        assert!(h.link_up && h.mouse_attached && h.clone_configured && h.injection_active);

        // A press is recorded as a BUTTON frame.
        device.press(Button::Left).unwrap();
        let frames = mock.recorded_frames();
        assert!(frames.iter().any(|f| f.ty == FrameType::Query));
        let button = frames.iter().find(|f| f.ty == FrameType::Button).unwrap();
        assert_eq!(button.payload, vec![0, 1]); // press Left
        assert!(mock.saw(FrameType::Button));
    }

    #[test]
    fn pushed_log_reaches_logs_channel() {
        let mock = MockBox::new();
        let device = crate::Device::with_mock(mock.clone());
        let rx = device.logs();

        mock.push_log(LogLevel::Warn, "overheating");
        let line = rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(line.level, LogLevel::Warn);
        assert_eq!(line.text, "overheating");
    }

    #[test]
    fn clear_recorded_resets_the_log() {
        let mock = MockBox::new();
        let device = crate::Device::with_mock(mock.clone());
        device.move_rel(1, 1).unwrap();
        assert_eq!(mock.recorded(), 1);
        mock.clear_recorded();
        assert_eq!(mock.recorded(), 0);
        device.wheel(2).unwrap();
        assert_eq!(mock.recorded(), 1);
        assert!(mock.saw(FrameType::Wheel));
    }

    #[test]
    fn set_health_updates_subsequent_queries() {
        let mock = MockBox::new();
        let device = crate::Device::with_mock(mock.clone());
        assert!(!device.query_health().unwrap().mouse_attached);
        mock.set_health(Health::from_flags(0x02)); // mouse_attached
        assert!(device.query_health().unwrap().mouse_attached);
    }
}
