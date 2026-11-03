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

#[derive(Clone, Copy, PartialEq, Eq)]
enum Accept {
    // This build's protocol only.
    Current,
    // Any protocol from MIN_PROTO_VER on; below PROTO_VER, the update path only.
    Update,
}

impl Device {
    /// Open the box at serial `path` and run the version handshake.
    pub fn open(path: impl AsRef<Path>) -> Result<Device> {
        Device::open_accepting(path.as_ref(), Accept::Current)
    }

    /// Open the box at serial `path` to update it, on any control protocol from
    /// [`MIN_PROTO_VER`](crate::MIN_PROTO_VER) on.
    ///
    /// A box on this build's [`PROTO_VER`](crate::PROTO_VER) opens as [`open`](Self::open) would. On
    /// any other, the device is [update-only](Self::is_update_only): it answers
    /// [`query_version`](Self::query_version), [`firmware_info`](Self::firmware_info),
    /// [`stage_firmware`](Self::stage_firmware), [`activate_firmware`](Self::activate_firmware),
    /// [`abort_update`](Self::abort_update), [`update_firmware`](Self::update_firmware) and
    /// [`wait_firmware_confirmed`](Self::wait_firmware_confirmed), and every other call returns
    /// [`Error::BadProtoVer`]. It holds no state and sends nothing on its own. Once updated, open the
    /// box again to use it.
    pub fn open_for_update(path: impl AsRef<Path>) -> Result<Device> {
        Device::open_accepting(path.as_ref(), Accept::Update)
    }

    /// Whether this device reaches only the update path: the box speaks an older control protocol
    /// than this build. See [`open_for_update`](Self::open_for_update).
    pub fn is_update_only(&self) -> bool {
        self.link.update_only().is_some()
    }

    fn open_accepting(path: &Path, accept: Accept) -> Result<Device> {
        let serial = crate::transport::serial::SerialTransport::open(path)?;
        let device = Device::from_transport(Arc::new(serial));
        let version = device.handshake(accept)?;
        // The scan lists the port by its device name; `path` may be a link to it (/dev/serial/by-id). Without its
        // serial, a reconnect has to probe every box and sim on the PC, which takes seconds.
        let real = std::fs::canonicalize(path).ok();
        let port_serial = crate::transport::scan::find_medius()
            .into_iter()
            .find(|p| {
                Path::new(&p.path) == path
                    || real.is_some() && std::fs::canonicalize(&p.path).ok() == real
            })
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
        device.handshake(Accept::Current)?;
        Ok(device)
    }

    #[cfg_attr(not(feature = "mock"), allow(dead_code))]
    pub(crate) fn open_transport_for_update(transport: Arc<dyn Transport>) -> Result<Device> {
        let device = Device::from_transport(transport);
        device.handshake(Accept::Update)?;
        Ok(device)
    }

    fn handshake(&self, accept: Accept) -> Result<Version> {
        let _span =
            trace_span!(target: "medius::device", tracing::Level::INFO, "connect").entered();

        if accept == Accept::Update {
            self.link.open_for_update();
        }
        let version = self.read_version()?;
        if self.link.reaches_update_only(version.proto_ver) {
            trace_event!(
                target: "medius::device",
                tracing::Level::INFO,
                proto_ver = version.proto_ver,
                "connected, update only",
            );
            return Ok(version);
        }
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
        // CAPS now, so a button blanket set before the caller's first caps() re-asserts every
        // declared button on reconnect, not only the five named ones. Best-effort, with one
        // handshake attempt's timeout. A box with no device reports 0 buttons; that isn't cached,
        // so the five named buttons apply until a real caps().
        if let Ok(payload) = self.link.query_timeout(Q_CAPS, HANDSHAKE_ATTEMPT_TIMEOUT)
            && let Some(Resp::Caps(caps)) = parse_resp(&payload)
            && caps.mouse.n_buttons > 0
        {
            self.link
                .desired()
                .lock()
                .note_declared_buttons(caps.mouse.n_buttons);
        }
        // Baseline the session counter, so a later session release is noticed.
        if let Ok(payload) = self.link.query_timeout(Q_STATS, HANDSHAKE_ATTEMPT_TIMEOUT)
            && let Some(Resp::Stats(stats)) = parse_resp(&payload)
        {
            self.link.restart_watch().note_session(stats.session);
        }
        Ok(version)
    }

    // Accepts any protocol version: discovery lists boxes this build can't speak to, and the
    // handshake checks the number.
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
