//! Discovered serial-port descriptor.

/// Information about one discovered serial port.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortInfo {
    /// The OS path used to open the port (`/dev/ttyACM0` on Linux, `COM3` on Windows).
    pub path: String,
    /// USB vendor id.
    pub vid: u16,
    /// USB product id.
    pub pid: u16,
    /// USB `iSerial` string of the control adapter, or `None` when it serves none.
    pub serial: Option<String>,
}
