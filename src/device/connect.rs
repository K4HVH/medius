//! Connection setup — [`Device::open`], [`Device::find`], and the version handshake (§2.2, §6).
//!
//! [`Device::from_transport`] (in [`super`]) builds the device and spawns the reader **without** a
//! handshake — the seam for tests and for these constructors. [`Device::open`] adds the real
//! handshake: send `QUERY(VERSION)`, require `proto_ver == PROTO_VER`, else reject. [`Device::find`]
//! scans candidate ports by VID/PID and opens the first match.

use std::sync::Arc;
use std::time::Duration;

use crate::config::ConnectOptions;
use crate::error::{Error, Result};
use crate::protocol::opcode::Q_VERSION;
use crate::protocol::{PROTO_VER, Resp, parse_resp};
use crate::transport::Transport;

use super::Device;

/// Handshake VERSION probes before giving up. A box can drop the very first frame after a fresh open
/// (the device-chip UART resyncing / stale RX after enumeration), so a single-shot probe fails
/// intermittently on reconnect — observed ~1-in-12 on real hardware. A few quick retries make connect
/// reliable without the native firmware's baud dance.
const HANDSHAKE_ATTEMPTS: usize = 5;

/// Per-attempt VERSION-probe timeout during the handshake. `HANDSHAKE_ATTEMPTS × this` bounds total
/// connect time (~1.25 s worst case) while each retry re-sends a fresh `QUERY(VERSION)`.
const HANDSHAKE_ATTEMPT_TIMEOUT: Duration = Duration::from_millis(250);

impl Device {
    /// Open the box at an explicit serial `path`, perform the version handshake, and return a ready
    /// [`Device`] (default [`ConnectOptions`]).
    ///
    /// The handshake sends `QUERY(VERSION)` and requires the reported `proto_ver` to equal
    /// [`PROTO_VER`](crate::protocol::PROTO_VER) (§2.2). A silent box surfaces as
    /// [`Error::NoReply`]; a wrong version as [`Error::BadProtoVer`].
    ///
    /// # Errors
    /// - [`Error::Io`] if the port cannot be opened/configured.
    /// - [`Error::NoReply`] if the box never answers the version probe.
    /// - [`Error::BadProtoVer`] if it answers with an unsupported protocol version.
    #[cfg(target_os = "linux")]
    pub fn open(path: impl AsRef<std::path::Path>) -> Result<Device> {
        Self::open_with(path, &ConnectOptions::default())
    }

    /// Open the box at an explicit serial `path` (Windows `COMn`), perform the handshake, and return
    /// a ready [`Device`] (default [`ConnectOptions`]). See the Linux [`open`](Device::open) for the
    /// handshake contract.
    #[cfg(windows)]
    pub fn open(path: impl AsRef<std::path::Path>) -> Result<Device> {
        Self::open_with(path, &ConnectOptions::default())
    }

    /// As [`open`](Device::open) but configured by `opts` (query timeout, keepalive cadence) — the
    /// [`ConnectOptions`]-driven constructor (§10).
    #[cfg(target_os = "linux")]
    pub fn open_with(path: impl AsRef<std::path::Path>, opts: &ConnectOptions) -> Result<Device> {
        let serial = crate::transport::linux::LinuxSerial::open(path.as_ref())?;
        Self::open_transport_with(Arc::new(serial), opts)
    }

    /// As [`open`](Device::open) (Windows) but configured by `opts` (§10).
    #[cfg(windows)]
    pub fn open_with(path: impl AsRef<std::path::Path>, opts: &ConnectOptions) -> Result<Device> {
        let serial = crate::transport::windows::WindowsSerial::open(path.as_ref())?;
        Self::open_transport_with(Arc::new(serial), opts)
    }

    /// Build a device over an already-open transport **and** run the handshake (default
    /// [`ConnectOptions`]). The general form is [`open_transport_with`](Device::open_transport_with);
    /// this no-options convenience is used by the device tests.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn open_transport(transport: Arc<dyn Transport>) -> Result<Device> {
        Self::open_transport_with(transport, &ConnectOptions::default())
    }

    /// As [`open_transport`](Device::open_transport) but configured by `opts` (§10).
    pub(crate) fn open_transport_with(
        transport: Arc<dyn Transport>,
        opts: &ConnectOptions,
    ) -> Result<Device> {
        let device = Device::from_transport_with(transport, opts);
        device.handshake()?;
        Ok(device)
    }

    /// Send `QUERY(VERSION)` and validate the reported protocol version (§2.2).
    ///
    /// Probes up to [`HANDSHAKE_ATTEMPTS`] times (each [`HANDSHAKE_ATTEMPT_TIMEOUT`]) because the box
    /// can drop the first frame after a fresh open; a timeout on every attempt surfaces as
    /// [`Error::NoReply`]. A parseable reply with the wrong protocol version is a hard
    /// [`Error::BadProtoVer`] (not retried — the box answered, it's just incompatible).
    fn handshake(&self) -> Result<()> {
        // A `connect` span groups the handshake's query/transport events (no-op without `tracing`).
        let _span =
            trace_span!(target: "medius::device", tracing::Level::INFO, "connect").entered();

        let mut version = None;
        for attempt in 0..HANDSHAKE_ATTEMPTS {
            match self.query_timeout(Q_VERSION, HANDSHAKE_ATTEMPT_TIMEOUT) {
                Ok(payload) => match parse_resp(&payload) {
                    Some(Resp::Version(v)) => {
                        version = Some(v);
                        break;
                    }
                    // Answered, but not a parseable VERSION — treat like a dropped probe and retry.
                    _ => {
                        trace_event!(target: "medius::device", tracing::Level::DEBUG, attempt, "handshake: unparseable version reply, retrying");
                    }
                },
                // No reply this attempt — retry (the first frame after open is the usual casualty).
                Err(Error::QueryTimeout) => {
                    trace_event!(target: "medius::device", tracing::Level::DEBUG, attempt, "handshake: version probe timed out, retrying");
                }
                // A real transport/encode error is fatal — don't burn the remaining attempts on it.
                Err(e) => return Err(e),
            }
        }

        let Some(version) = version else {
            // Every probe timed out — there is no (responsive) box at the other end.
            trace_event!(target: "medius::device", tracing::Level::WARN, attempts = HANDSHAKE_ATTEMPTS, "handshake: no reply to version query");
            return Err(Error::NoReply);
        };
        if version.proto_ver != PROTO_VER {
            trace_event!(
                target: "medius::device",
                tracing::Level::WARN,
                got = version.proto_ver,
                expected = PROTO_VER,
                "handshake: unsupported protocol version",
            );
            return Err(Error::BadProtoVer {
                got: version.proto_ver,
            });
        }
        // Connect succeeded — INFO with the firmware/proto fields (§10).
        trace_event!(
            target: "medius::device",
            tracing::Level::INFO,
            proto_ver = version.proto_ver,
            fw_major = version.fw_major,
            fw_minor = version.fw_minor,
            fw_patch = version.fw_patch,
            "connected",
        );
        Ok(())
    }

    /// Discover the first medius box by VID/PID, open it, and handshake (§6, default
    /// [`ConnectOptions`]).
    ///
    /// # Errors
    /// [`Error::NotFound`] if no candidate port matches; otherwise the same errors as
    /// [`open`](Device::open).
    #[cfg(any(target_os = "linux", windows))]
    pub fn find() -> Result<Device> {
        Self::find_with(&ConnectOptions::default())
    }

    /// As [`find`](Device::find) but configured by `opts` (§10).
    #[cfg(any(target_os = "linux", windows))]
    pub fn find_with(opts: &ConnectOptions) -> Result<Device> {
        let port = crate::transport::scan::find_medius()
            .into_iter()
            .next()
            .ok_or(Error::NotFound)?;
        Device::open_with(port.path, opts)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use crate::error::Error;
    use crate::protocol::{FrameType, encode};
    use crate::transport::mock::MockTransport;

    use super::Device;

    /// The handshake must survive the box dropping its first frame(s) after open — it retries the
    /// VERSION probe. Here the mock ignores the first two `QUERY(VERSION)` frames, then answers; the
    /// handshake must still connect (reproduces the ~1-in-12 real-hardware reopen flake).
    #[test]
    fn handshake_retries_past_dropped_first_frames() {
        let calls = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&calls);
        let mock = Arc::new(MockTransport::with_responder(move |ty, seq, payload| {
            if ty == FrameType::Query && payload.first() == Some(&0) {
                // Drop the first two VERSION probes (return nothing); answer the third onward.
                if counter.fetch_add(1, Ordering::SeqCst) < 2 {
                    return Vec::new();
                }
                return encode(FrameType::Resp, seq, &[0, 1, 0, 1, 0]).unwrap();
            }
            Vec::new()
        }));

        let device = Device::open_transport(mock)
            .expect("handshake should retry past the dropped first frames and connect");
        assert_eq!(device.query_version().unwrap().proto_ver, 1);
        // At least 3 probes were sent (2 dropped + 1 answered).
        assert!(calls.load(Ordering::SeqCst) >= 3);
    }

    /// A genuinely silent box still fails fast-ish with `NoReply` after exhausting the retries.
    #[test]
    fn handshake_gives_up_with_no_reply_when_silent() {
        let mock = Arc::new(MockTransport::new()); // never answers
        let err = Device::open_transport(mock).unwrap_err();
        assert!(matches!(err, Error::NoReply), "got {err:?}");
    }
}
