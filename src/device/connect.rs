//! Connection setup — [`Device::open`], [`Device::find`], and the version handshake (§2.2, §6).
//!
//! [`Device::open`] adds the handshake on top of `from_transport`: send `QUERY(VERSION)`, require
//! `proto_ver == PROTO_VER`, else reject. [`Device::find`] scans ports by VID/PID and opens the first.

use std::sync::Arc;
use std::time::Duration;

use crate::error::{Error, Result};
use crate::protocol::opcode::Q_VERSION;
use crate::protocol::{PROTO_VER, Resp, parse_resp};
use crate::transport::Transport;

use super::Device;

/// VERSION probes before giving up. A box can drop the first frame after a fresh open (device-chip
/// UART resyncing / stale RX after enumeration) — observed ~1-in-12 on real hardware — so a few quick
/// retries make connect reliable without the native firmware's baud dance.
const HANDSHAKE_ATTEMPTS: usize = 5;

/// Per-attempt probe timeout; `HANDSHAKE_ATTEMPTS ×` this bounds total connect time (~1.25 s worst).
const HANDSHAKE_ATTEMPT_TIMEOUT: Duration = Duration::from_millis(250);

impl Device {
    /// Open the box at serial `path`, run the version handshake, and return a ready [`Device`].
    ///
    /// The handshake sends `QUERY(VERSION)` and requires the reported `proto_ver` to equal the
    /// library's supported protocol version (§2.2).
    ///
    /// # Errors
    /// - [`Error::Io`] if the port cannot be opened/configured.
    /// - [`Error::NoReply`] if the box never answers the version probe.
    /// - [`Error::BadProtoVer`] if it answers with an unsupported protocol version.
    #[cfg(target_os = "linux")]
    pub fn open(path: impl AsRef<std::path::Path>) -> Result<Device> {
        let serial = crate::transport::linux::LinuxSerial::open(path.as_ref())?;
        Self::open_transport(Arc::new(serial))
    }

    /// Open the box at serial `path` (Windows `COMn`); see the Linux [`open`](Device::open) for the
    /// handshake contract and errors.
    #[cfg(windows)]
    pub fn open(path: impl AsRef<std::path::Path>) -> Result<Device> {
        let serial = crate::transport::windows::WindowsSerial::open(path.as_ref())?;
        Self::open_transport(Arc::new(serial))
    }

    /// Build a device over an already-open transport and run the handshake. The transport seam shared by
    /// [`open`](Device::open) and the device tests.
    pub(crate) fn open_transport(transport: Arc<dyn Transport>) -> Result<Device> {
        let device = Device::from_transport(transport);
        device.handshake()?;
        Ok(device)
    }

    /// Send `QUERY(VERSION)` and validate the reported protocol version (§2.2).
    ///
    /// Probes up to [`HANDSHAKE_ATTEMPTS`] times because the box can drop the first frame after a fresh
    /// open; all-timeout surfaces as [`Error::NoReply`]. A wrong version is a hard
    /// [`Error::BadProtoVer`] (not retried — the box answered, it's just incompatible).
    fn handshake(&self) -> Result<()> {
        // `connect` span grouping the handshake's events (no-op without `tracing`).
        let _span =
            trace_span!(target: "medius::device", tracing::Level::INFO, "connect").entered();

        let mut version = None;
        for _ in 0..HANDSHAKE_ATTEMPTS {
            match self.query_timeout(Q_VERSION, HANDSHAKE_ATTEMPT_TIMEOUT) {
                Ok(payload) => match parse_resp(&payload) {
                    Some(Resp::Version(v)) => {
                        version = Some(v);
                        break;
                    }
                    // Answered, but not a parseable VERSION — treat like a dropped probe and retry.
                    _ => {
                        trace_event!(target: "medius::device", tracing::Level::DEBUG, "handshake: unparseable version reply, retrying");
                    }
                },
                // No reply this attempt — retry.
                Err(Error::QueryTimeout) => {
                    trace_event!(target: "medius::device", tracing::Level::DEBUG, "handshake: version probe timed out, retrying");
                }
                // A real transport/encode error is fatal — don't burn the remaining attempts.
                Err(e) => return Err(e),
            }
        }

        let Some(version) = version else {
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

    /// Discover the first medius box by VID/PID, open it, and handshake (§6).
    ///
    /// # Errors
    /// [`Error::NotFound`] if no port matches; otherwise the same errors as [`open`](Device::open).
    #[cfg(any(target_os = "linux", windows))]
    pub fn find() -> Result<Device> {
        let port = crate::transport::scan::find_medius()
            .into_iter()
            .next()
            .ok_or(Error::NotFound)?;
        Device::open(port.path)
    }
}
