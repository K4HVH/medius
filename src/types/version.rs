//! Decoded `RESP(VERSION)` payload.

use core::fmt;

/// Decoded `RESP(VERSION)` payload.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Version {
    /// Protocol version; [`PROTO_VER`](crate::PROTO_VER) is the only value this build accepts.
    pub proto_ver: u8,
    pub fw_major: u8,
    pub fw_minor: u8,
    pub fw_patch: u8,
    /// Device chip's factory base MAC: a per-box identity that survives port renumbering.
    pub mac: [u8; 6],
    /// Box name, never empty (the firmware derives a `Medius-XXXX` default from the MAC).
    pub name: String,
}

impl Version {
    /// Base MAC as 12 lowercase hex digits, no separators; the canonical box id.
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
        write!(
            f,
            "fw {}.{}.{}",
            self.fw_major, self.fw_minor, self.fw_patch
        )
    }
}
