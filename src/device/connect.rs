use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use crate::error::{Error, Result};
use crate::link::reconnect::BoxIdentity;
use crate::protocol::opcode::Q_VERSION;
use crate::protocol::{PROTO_VER, Resp, parse_resp};
use crate::transport::Transport;
use crate::types::Version;

use super::Device;

const HANDSHAKE_ATTEMPTS: usize = 5;

const HANDSHAKE_ATTEMPT_TIMEOUT: Duration = Duration::from_millis(250);

impl Device {
    /// Open the box at serial `path`, run the version handshake, and return a ready [`Device`]. The
    /// box's identity (CH343 serial + device MAC) is recorded so a later auto-reconnect re-finds this
    /// same box even if ports renumber.
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

        let mut version = None;
        for _ in 0..HANDSHAKE_ATTEMPTS {
            match self
                .link
                .query_timeout(Q_VERSION, HANDSHAKE_ATTEMPT_TIMEOUT)
            {
                Ok(payload) => match parse_resp(&payload) {
                    Some(Resp::Version(v)) => {
                        version = Some(v);
                        break;
                    }
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
        Ok(version)
    }

    /// Discover the first medius box by VID/PID, open it, and handshake.
    pub fn find() -> Result<Device> {
        let port = crate::transport::scan::find_medius()
            .into_iter()
            .next()
            .ok_or(Error::NotFound)?;
        Device::open(port.path)
    }
}
