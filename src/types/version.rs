//! The decoded `RESP(VERSION)` payload — firmware/protocol identity.

use core::fmt;

/// The decoded `RESP(VERSION)` payload (§4.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Version {
    /// Protocol version, expected to be `1`.
    pub proto_ver: u8,
    pub fw_major: u8,
    pub fw_minor: u8,
    pub fw_patch: u8,
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "fw {}.{}.{}",
            self.fw_major, self.fw_minor, self.fw_patch
        )
    }
}
