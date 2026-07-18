//! Multi-box discovery: enumerate connected medius boxes and open one by identity or clone kind.

use crate::error::{Error, Result};
use crate::types::{DeviceInfo, DeviceKind, PortInfo, Version};

use super::Device;

/// One discovered medius box: its serial port, firmware version, and the device it currently clones.
#[derive(Debug, Clone)]
pub struct BoxInfo {
    /// The control serial port (path + USB ids + serial).
    pub port: PortInfo,
    /// Firmware version and the box's base MAC.
    pub version: Version,
    /// The cloned device's identity, kind, and product.
    pub device: DeviceInfo,
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
    let device = Device::open(&port.path).ok()?;
    let version = device.query_version().ok()?;
    let info = device.device_info().ok()?;
    Some(BoxInfo {
        port: port.clone(),
        version,
        device: info,
    })
}

impl Device {
    /// Enumerate every connected medius box, reading each one's version and cloned-device info.
    pub fn list() -> Vec<BoxInfo> {
        crate::transport::scan::find_medius()
            .iter()
            .filter_map(probe)
            .collect()
    }

    /// Open the box whose identity matches `id`, either its device MAC or its CH343 serial.
    pub fn open_by_id(id: &str) -> Result<Device> {
        let want: String = id
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .flat_map(char::to_lowercase)
            .collect();
        let info = Device::list()
            .into_iter()
            .find(|b| {
                b.id() == want
                    || b.serial().is_some_and(|s| {
                        s.eq_ignore_ascii_case(id) || s.eq_ignore_ascii_case(&want)
                    })
            })
            .ok_or(Error::NotFound)?;
        Device::open(&info.port.path)
    }

    /// Open the first box whose clone is a mouse ([`DeviceKind::Mouse`]).
    pub fn find_mouse_box() -> Result<Device> {
        Device::find_where(|b| b.device.kind == DeviceKind::Mouse)
    }

    /// Open the first box whose clone is a keyboard ([`DeviceKind::Keyboard`]).
    pub fn find_keyboard_box() -> Result<Device> {
        Device::find_where(|b| b.device.kind == DeviceKind::Keyboard)
    }

    /// Open the first discovered box that satisfies `pred`. [`Error::NotFound`] if none match.
    pub fn find_where(pred: impl Fn(&BoxInfo) -> bool) -> Result<Device> {
        let info = Device::list()
            .into_iter()
            .find(|b| pred(b))
            .ok_or(Error::NotFound)?;
        Device::open(&info.port.path)
    }
}
