//! `REBOOT_DL` target vocabulary.

/// A `REBOOT_DL` target (§3.6); discriminants are the wire `target` byte. `*Download` enters ROM
/// download mode (for flashing); `*Run` reboots to run the firmware.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
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

    /// Map a wire `target` byte to a [`RebootTarget`], or `None` for an unknown value.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reboot_target_round_trips() {
        for (v, t) in [
            (0u8, RebootTarget::DeviceDownload),
            (1, RebootTarget::HostDownload),
            (2, RebootTarget::DeviceRun),
            (3, RebootTarget::HostRun),
        ] {
            assert_eq!(RebootTarget::from_u8(v), Some(t));
            assert_eq!(t.as_u8(), v);
        }
        assert_eq!(RebootTarget::from_u8(4), None);
    }
}
