//! Discovered serial-port descriptor.

/// Information about one discovered serial port.
///
/// Produced by [`find_medius`](crate::find_medius); `path` is the OS path used to open the port.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PortInfo {
    /// The OS path used to open the port (`/dev/ttyACM0` on Linux, `COM3` on Windows).
    pub path: String,
    /// USB vendor id.
    pub vid: u16,
    /// USB product id.
    pub pid: u16,
}
