//! Serial port discovery: enumerate USB serial ports and filter to the box by USB VID/PID.

use crate::types::PortInfo;

pub(crate) const WCH_VID: u16 = 0x1A86;

pub(crate) const CH343_PID: u16 = 0x55D3;

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

/// Discover medius boxes: USB serial ports filtered to the WCH vendor id and CH343 product id.
pub fn find_medius() -> Vec<PortInfo> {
    find_ports()
        .into_iter()
        .filter(|p| p.vid == WCH_VID && p.pid == CH343_PID)
        .collect()
}
