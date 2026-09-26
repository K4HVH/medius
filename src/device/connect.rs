use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use crate::error::{Error, Result};
use crate::link::reconnect::BoxIdentity;
use crate::protocol::opcode::{Q_CAPS, Q_STATS, Q_VERSION};
use crate::protocol::{PROTO_VER, Resp, parse_resp};
use crate::transport::Transport;
use crate::types::Version;

use super::Device;

const HANDSHAKE_ATTEMPTS: usize = 5;

const HANDSHAKE_ATTEMPT_TIMEOUT: Duration = Duration::from_millis(250);

impl Device {
    /// Open the box at serial `path` and run the version handshake.
    pub fn open(path: impl AsRef<Path>) -> Result<Device> {
        let path = path.as_ref();
        let serial = crate::transport::serial::SerialTransport::open(path)?;
        let device = Device::from_transport(Arc::new(serial));
        let version = device.handshake()?;
        let port_serial = crate::transport::scan::find_medius()
            .into_iter()
            .find(|p| Path::new(&p.path) == path)
            .and_then(|p| p.serial);
        device.link.set_identity(BoxIdentity {
            serial: port_serial,
            mac: version.mac,
        });
        Ok(device)
    }

    #[cfg_attr(not(feature = "mock"), allow(dead_code))]
    pub(crate) fn open_transport(transport: Arc<dyn Transport>) -> Result<Device> {
        let device = Device::from_transport(transport);
        device.handshake()?;
        Ok(device)
    }

    fn handshake(&self) -> Result<Version> {
        let _span =
            trace_span!(target: "medius::device", tracing::Level::INFO, "connect").entered();

        let version = self.read_version()?;
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
        // CAPS now, so a button blanket set before the caller's own caps() re-asserts every declared
        // button across a reconnect, not just the five named ones. Best-effort, bounded like a
        // handshake attempt; a box with no device reports zero, left uncached so the named buttons
        // apply until a real caps().
        if let Ok(payload) = self.link.query_timeout(Q_CAPS, HANDSHAKE_ATTEMPT_TIMEOUT)
            && let Some(Resp::Caps(caps)) = parse_resp(&payload)
            && caps.mouse.n_buttons > 0
        {
            self.link
                .desired()
                .lock()
                .note_declared_buttons(caps.mouse.n_buttons);
        }
        // Baseline session counter, so any later release is noticed.
        if let Ok(payload) = self.link.query_timeout(Q_STATS, HANDSHAKE_ATTEMPT_TIMEOUT)
            && let Some(Resp::Stats(stats)) = parse_resp(&payload)
        {
            self.link.restart_watch().note_session(stats.session);
        }
        Ok(version)
    }

    // Whatever protocol the box reports: discovery lists a box this build cannot speak to; the
    // handshake then checks the number.
    pub(crate) fn read_version(&self) -> Result<Version> {
        for _ in 0..HANDSHAKE_ATTEMPTS {
            match self
                .link
                .query_timeout(Q_VERSION, HANDSHAKE_ATTEMPT_TIMEOUT)
            {
                Ok(payload) => match parse_resp(&payload) {
                    Some(Resp::Version(v)) => return Ok(v),
                    _ => {
                        trace_event!(target: "medius::device", tracing::Level::DEBUG, "handshake: unparseable version reply, retrying");
                    }
                },
                Err(Error::QueryTimeout) => {
                    trace_event!(target: "medius::device", tracing::Level::DEBUG, "handshake: version probe timed out, retrying");
                }
                Err(e) => return Err(e),
            }
        }
        trace_event!(target: "medius::device", tracing::Level::WARN, attempts = HANDSHAKE_ATTEMPTS, "handshake: no reply to version query");
        Err(Error::NoReply)
    }

    /// Open the first box found by VID/PID, with the handshake.
    pub fn find() -> Result<Device> {
        let port = crate::transport::scan::find_medius()
            .into_iter()
            .next()
            .ok_or(Error::NotFound)?;
        Device::open(port.path)
    }
}
