//! `REBOOT_DL` target vocabulary.

/// A `REBOOT_DL` target; discriminants are the wire `target` byte.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RebootTarget {
    DeviceDownload = 0,
    HostDownload = 1,
    DeviceRun = 2,
    HostRun = 3,
}

impl RebootTarget {
    /// The wire `target` byte for this reboot target.
    pub fn as_u8(self) -> u8 {
        self as u8
    }

    /// Map a wire `target` byte to a [`RebootTarget`], or `None` if unknown.
    pub fn from_u8(v: u8) -> Option<Self> {
        Some(match v {
            0 => RebootTarget::DeviceDownload,
            1 => RebootTarget::HostDownload,
            2 => RebootTarget::DeviceRun,
            3 => RebootTarget::HostRun,
            _ => return None,
        })
    }
}
