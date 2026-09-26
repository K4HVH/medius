//! Discovered serial-port descriptor.

/// Discovered serial port.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortInfo {
    /// OS path that opens the port (`/dev/ttyACM0` on Linux, `COM3` on Windows).
    pub path: String,
    /// USB vendor id.
    pub vid: u16,
    /// USB product id.
    pub pid: u16,
    /// USB `iSerial` string of the control adapter, or `None` when it serves none.
    pub serial: Option<String>,
}
