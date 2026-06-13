//! The decoded `RESP(VERSION)` payload — firmware/protocol identity.

use core::fmt;

/// The decoded `RESP(VERSION)` payload (§4.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_display() {
        let v = Version {
            proto_ver: 1,
            fw_major: 2,
            fw_minor: 3,
            fw_patch: 4,
        };
        assert_eq!(v.to_string(), "fw 2.3.4");
    }
}
