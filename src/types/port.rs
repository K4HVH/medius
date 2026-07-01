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
    /// The USB `iSerial` string of the control adapter, if it serves one — a fast, scan-time box
    /// identity (`None` when the adapter has no serial; fall back to the device MAC then).
    pub serial: Option<String>,
}
