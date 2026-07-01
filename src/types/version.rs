//! The decoded `RESP(VERSION)` payload.

use core::fmt;

/// The decoded `RESP(VERSION)` payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Version {
    /// Protocol version, expected to be `2`.
    pub proto_ver: u8,
    pub fw_major: u8,
    pub fw_minor: u8,
    pub fw_patch: u8,
    /// The device chip's factory base MAC — a stable per-box identity that survives port renumbering.
    pub mac: [u8; 6],
}

impl Version {
    /// The base MAC as 12 lowercase hex digits, no separators — the canonical box id.
    pub fn mac_hex(&self) -> String {
        let mut s = String::with_capacity(12);
        for b in self.mac {
            s.push(char::from_digit((b >> 4) as u32, 16).unwrap());
            s.push(char::from_digit((b & 0xF) as u32, 16).unwrap());
        }
        s
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "fw {}.{}.{}", self.fw_major, self.fw_minor, self.fw_patch)
    }
}
