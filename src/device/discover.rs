//! Multi-box discovery: enumerate connected medius boxes and open one by identity or clone kind.

use std::path::Path;
use std::sync::Arc;

use crate::error::{Error, Result};
use crate::protocol::PROTO_VER;
use crate::transport::Transport;
use crate::types::{DeviceInfo, DeviceKind, PortInfo, Version};

use super::Device;

/// One discovered medius box: its serial port, firmware version, and the device it currently clones.
#[derive(Debug, Clone)]
pub struct BoxInfo {
    /// The control serial port (path + USB ids + serial).
    pub port: PortInfo,
    /// Firmware version, control protocol, and the box's base MAC.
    pub version: Version,
    /// The cloned device's identity, kind, and product. `None` for a box whose
    /// [`proto_ver`](Version::proto_ver) is not [`PROTO_VER`](crate::PROTO_VER): this build reads
    /// nothing past its version, and opening it answers [`Error::BadProtoVer`]. Update that box's
    /// firmware from the dashboard at <https://medius.k4tech.net/dashboard>.
    pub device: Option<DeviceInfo>,
}

impl BoxInfo {
    /// The canonical, stable box id: the device MAC as 12 lowercase hex digits.
    pub fn id(&self) -> String {
        self.version.mac_hex()
    }

    /// The CH343 control-adapter serial, if it serves one.
    pub fn serial(&self) -> Option<&str> {
        self.port.serial.as_deref()
    }

    /// The box's human-readable name (its readable partner to [`id`](Self::id)), from `RESP(VERSION)`.
    pub fn name(&self) -> &str {
        &self.version.name
    }
}

fn probe(port: &PortInfo) -> Option<BoxInfo> {
    let serial = crate::transport::serial::SerialTransport::open(Path::new(&port.path)).ok()?;
    probe_transport(port, Arc::new(serial))
}

// The box behind one transport. It is listed on its version alone when it speaks another protocol,
// so a box that needs an update shows up as one.
pub(crate) fn probe_transport(port: &PortInfo, transport: Arc<dyn Transport>) -> Option<BoxInfo> {
    let device = Device::from_transport(transport);
    let version = device.read_version().ok()?;
    let info = if version.proto_ver == PROTO_VER {
        Some(device.device_info().ok()?)
    } else {
        None
    };
    Some(BoxInfo {
        port: port.clone(),
        version,
        device: info,
    })
}

// A box this build cannot speak to is refused with the protocol it reported, never skipped.
fn openable(b: &BoxInfo) -> Result<&BoxInfo> {
    match b.device {
        Some(_) => Ok(b),
        None => Err(Error::BadProtoVer {
            got: b.version.proto_ver,
        }),
    }
}

pub(crate) fn pick_by_id<'a>(boxes: &'a [BoxInfo], id: &str) -> Result<&'a BoxInfo> {
    let want: String = id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect();
    let found = boxes
        .iter()
        .find(|b| {
            b.id() == want
                || b.serial()
                    .is_some_and(|s| s.eq_ignore_ascii_case(id) || s.eq_ignore_ascii_case(&want))
        })
        .ok_or(Error::NotFound)?;
    openable(found)
}

// A box this build speaks to wins; one on another protocol is picked only when nothing else matches.
pub(crate) fn pick_where(boxes: &[BoxInfo], pred: impl Fn(&BoxInfo) -> bool) -> Result<&BoxInfo> {
    let found = boxes
        .iter()
        .find(|b| b.device.is_some() && pred(b))
        .or_else(|| boxes.iter().find(|b| pred(b)))
        .ok_or(Error::NotFound)?;
    openable(found)
}

// Whether a box may clone `kind`. A box on another protocol reports no clone, so it may.
pub(crate) fn may_clone(kind: DeviceKind) -> impl Fn(&BoxInfo) -> bool {
    move |b| b.device.as_ref().is_none_or(|d| d.kind == kind)
}

impl Device {
    /// Enumerate every connected medius box, reading each one's version and cloned-device info. A box
    /// on another control protocol is listed too, with [`device`](BoxInfo::device) `None`.
    pub fn list() -> Vec<BoxInfo> {
        crate::transport::scan::find_medius()
            .iter()
            .filter_map(probe)
            .collect()
    }

    /// Open the box whose identity matches `id`, either its device MAC or its CH343 serial.
    /// [`Error::BadProtoVer`] when that box speaks another control protocol, [`Error::NotFound`] when
    /// no box matches.
    pub fn open_by_id(id: &str) -> Result<Device> {
        Device::open(&pick_by_id(&Device::list(), id)?.port.path)
    }

    /// Open the first box whose clone is a mouse ([`DeviceKind::Mouse`]). A box on another control
    /// protocol reports no clone, so when no other box clones a mouse this answers
    /// [`Error::BadProtoVer`] for it.
    pub fn find_mouse_box() -> Result<Device> {
        Device::find_where(may_clone(DeviceKind::Mouse))
    }

    /// Open the first box whose clone is a keyboard ([`DeviceKind::Keyboard`]). A box on another
    /// control protocol reports no clone, so when no other box clones a keyboard this answers
    /// [`Error::BadProtoVer`] for it.
    pub fn find_keyboard_box() -> Result<Device> {
        Device::find_where(may_clone(DeviceKind::Keyboard))
    }

    /// Open the first discovered box that satisfies `pred`, preferring a box this build speaks to. A
    /// box on another control protocol ([`device`](BoxInfo::device) `None`) is chosen only when no
    /// other box satisfies `pred`, and answers [`Error::BadProtoVer`]. [`Error::NotFound`] if none
    /// match.
    pub fn find_where(pred: impl Fn(&BoxInfo) -> bool) -> Result<Device> {
        Device::open(&pick_where(&Device::list(), pred)?.port.path)
    }
}
