//! Connection setup — [`Device::open`], [`Device::find`], and the version handshake (§2.2, §6).
//!
//! [`Device::from_transport`] (in [`super`]) builds the device and spawns the reader **without** a
//! handshake — the seam for tests and for these constructors. [`Device::open`] adds the real
//! handshake: send `QUERY(VERSION)`, require `proto_ver == PROTO_VER`, else reject. [`Device::find`]
//! scans candidate ports by VID/PID and opens the first match.

use std::sync::Arc;

use crate::error::{Error, Result};
use crate::protocol::PROTO_VER;
use crate::transport::Transport;

use super::Device;

impl Device {
    /// Open the box at an explicit serial `path`, perform the version handshake, and return a ready
    /// [`Device`].
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
        let serial = crate::transport::linux::LinuxSerial::open(path.as_ref())?;
        Self::open_transport(Arc::new(serial))
    }

    /// Open the box at an explicit serial `path` (Windows `COMn`), perform the handshake, and return
    /// a ready [`Device`]. See the Linux [`open`](Device::open) for the handshake contract.
    #[cfg(windows)]
    pub fn open(path: impl AsRef<std::path::Path>) -> Result<Device> {
        let serial = crate::transport::windows::WindowsSerial::open(path.as_ref())?;
        Self::open_transport(Arc::new(serial))
    }

    /// Build a device over an already-open transport **and** run the handshake.
    ///
    /// Shared by [`open`](Device::open) and [`reconnect`](Device::reconnect); the test seam is
    /// [`from_transport`](Device::from_transport) (no handshake).
    pub(crate) fn open_transport(transport: Arc<dyn Transport>) -> Result<Device> {
        let device = Device::from_transport(transport);
        device.handshake()?;
        Ok(device)
    }

    /// Send `QUERY(VERSION)` and validate the reported protocol version (§2.2).
    fn handshake(&self) -> Result<()> {
        let version = match self.query_version() {
            Ok(v) => v,
            // A handshake timeout means "no box at the other end" — surface the dedicated NoReply,
            // not the generic query timeout.
            Err(Error::QueryTimeout) => return Err(Error::NoReply),
            Err(e) => return Err(e),
        };
        if version.proto_ver != PROTO_VER {
            return Err(Error::BadProtoVer {
                got: version.proto_ver,
            });
        }
        Ok(())
    }

    /// Discover the first medius box by VID/PID, open it, and handshake (§6).
    ///
    /// # Errors
    /// [`Error::NotFound`] if no candidate port matches; otherwise the same errors as
    /// [`open`](Device::open).
    #[cfg(any(target_os = "linux", windows))]
    pub fn find() -> Result<Device> {
        let port = crate::transport::scan::find_medius()
            .into_iter()
            .next()
            .ok_or(Error::NotFound)?;
        Device::open(port.path)
    }
}
