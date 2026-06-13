//! Serial port discovery — enumerate USB serial ports and filter to the box by USB VID/PID.
//!
//! [`find_ports`] lists USB serial ports via `serialport::available_ports`; [`find_medius`] keeps the
//! CH343 (WCH) ones. Reconnect rescans by VID/PID, not a fixed path, so a re-enumerated device is
//! found again. The handshake is the final gate distinguishing a box from any other CH343 device.

use crate::types::PortInfo;

/// WCH (Jiangsu Qinheng) USB vendor id — the CH343 USB-serial bridge the medius box uses (§6).
pub(crate) const WCH_VID: u16 = 0x1A86;

/// The CH343 USB product id, confirmed on the medius board hardware (`idProduct = 55d3`).
pub(crate) const CH343_PID: u16 = 0x55D3;

/// Every USB serial port with its VID/PID (empty if enumeration fails or the platform is unsupported).
pub(crate) fn find_ports() -> Vec<PortInfo> {
    serialport::available_ports()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|p| match p.port_type {
            serialport::SerialPortType::UsbPort(usb) => Some(PortInfo {
                path: p.port_name,
                vid: usb.vid,
                pid: usb.pid,
            }),
            _ => None,
        })
        .collect()
}

/// Discover medius boxes: the USB serial ports filtered to the WCH vendor id and the CH343 product id.
pub fn find_medius() -> Vec<PortInfo> {
    find_ports()
        .into_iter()
        .filter(|p| p.vid == WCH_VID && p.pid == CH343_PID)
        .collect()
}
