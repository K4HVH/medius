//! Decoded value types — the typed surface over the raw wire bytes.
//!
//! `serde` derives use `snake_case` to match the wire doc / `medius.py` / CLI. The `as_*`/`from_*`
//! helpers map between the typed forms and the raw `u8` wire values in [`super::opcode`].

use core::fmt;

use super::opcode::{
    ACT_FORCEREL, ACT_PRESS, ACT_SOFTREL, BTN_LEFT, BTN_MIDDLE, BTN_RIGHT, BTN_SIDE1, BTN_SIDE2,
    H_CLONE_CFG, H_INJECT_ON, H_LINK_UP, H_MOUSE_ATT, LOG_DEBUG, LOG_ERROR, LOG_INFO, LOG_VERBOSE,
    LOG_WARN,
};

/// One of the five standard mouse buttons (§3.3).
///
/// `id`s bind at clone-time to the captured mouse's descriptor fields; a command for a button the
/// attached mouse lacks is a firmware no-op.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum Button {
    Left,
    Right,
    Middle,
    Side1,
    Side2,
}

impl Button {
    /// The wire `id` byte for this button (§3.3).
    pub fn as_id(self) -> u8 {
        match self {
            Button::Left => BTN_LEFT,
            Button::Right => BTN_RIGHT,
            Button::Middle => BTN_MIDDLE,
            Button::Side1 => BTN_SIDE1,
            Button::Side2 => BTN_SIDE2,
        }
    }

    /// Map a wire `id` byte to a [`Button`], or `None` for an unknown id.
    pub fn from_id(id: u8) -> Option<Self> {
        Some(match id {
            BTN_LEFT => Button::Left,
            BTN_RIGHT => Button::Right,
            BTN_MIDDLE => Button::Middle,
            BTN_SIDE1 => Button::Side1,
            BTN_SIDE2 => Button::Side2,
            _ => return None,
        })
    }
}

/// A button injection override action (§3.3); discriminants are the wire `action` byte.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum ButtonAction {
    /// Clear our injected press; defer to physical state.
    SoftRelease = ACT_SOFTREL,
    /// Force the button down regardless of physical state.
    Press = ACT_PRESS,
    /// Force the button up, masking a physical hold.
    ForceRelease = ACT_FORCEREL,
}

impl ButtonAction {
    /// The wire `action` byte for this action.
    pub fn as_u8(self) -> u8 {
        self as u8
    }

    /// Map a wire `action` byte to a [`ButtonAction`], or `None` for an unknown value.
    pub fn from_u8(v: u8) -> Option<Self> {
        Some(match v {
            ACT_SOFTREL => ButtonAction::SoftRelease,
            ACT_PRESS => ButtonAction::Press,
            ACT_FORCEREL => ButtonAction::ForceRelease,
            _ => return None,
        })
    }
}

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

/// A device `LOG` frame severity level (§4.3).
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
    Verbose,
}

impl LogLevel {
    /// The wire `level` byte for this level.
    pub fn as_u8(self) -> u8 {
        match self {
            LogLevel::Error => LOG_ERROR,
            LogLevel::Warn => LOG_WARN,
            LogLevel::Info => LOG_INFO,
            LogLevel::Debug => LOG_DEBUG,
            LogLevel::Verbose => LOG_VERBOSE,
        }
    }

    /// Map a wire `level` byte to a [`LogLevel`]; unknown levels fall back to `Info` (matching
    /// `medius.py`) so a forward-compat level never panics or loses the log text.
    pub fn from_u8(v: u8) -> Self {
        match v {
            LOG_ERROR => LogLevel::Error,
            LOG_WARN => LogLevel::Warn,
            LOG_INFO => LogLevel::Info,
            LOG_DEBUG => LogLevel::Debug,
            LOG_VERBOSE => LogLevel::Verbose,
            _ => LogLevel::Info,
        }
    }
}

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

/// The decoded `RESP(HEALTH)` flags byte (§4.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Health {
    /// Inter-chip link to the host chip is up.
    pub link_up: bool,
    /// A real mouse is attached on the host chip.
    pub mouse_attached: bool,
    /// The clone has been configured by the game PC.
    pub clone_configured: bool,
    /// Injection is currently active.
    pub injection_active: bool,
}

impl Health {
    /// Decode a `RESP(HEALTH)` flags byte (§4.2). Bits b4–b7 are unused in v1 and ignored.
    pub fn from_flags(flags: u8) -> Self {
        Health {
            link_up: flags & H_LINK_UP != 0,
            mouse_attached: flags & H_MOUSE_ATT != 0,
            clone_configured: flags & H_CLONE_CFG != 0,
            injection_active: flags & H_INJECT_ON != 0,
        }
    }

    /// Re-encode this health view back to its flags byte (inverse of [`Health::from_flags`]).
    pub fn to_flags(self) -> u8 {
        let mut flags = 0u8;
        if self.link_up {
            flags |= H_LINK_UP;
        }
        if self.mouse_attached {
            flags |= H_MOUSE_ATT;
        }
        if self.clone_configured {
            flags |= H_CLONE_CFG;
        }
        if self.injection_active {
            flags |= H_INJECT_ON;
        }
        flags
    }
}

/// A decoded `LOG` frame (§4.3): a severity level and its UTF-8 text.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct LogLine {
    pub level: LogLevel,
    /// Decoded lossily from UTF-8; not NUL-terminated on the wire.
    pub text: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn button_id_round_trips() {
        for (id, btn) in [
            (0u8, Button::Left),
            (1, Button::Right),
            (2, Button::Middle),
            (3, Button::Side1),
            (4, Button::Side2),
        ] {
            assert_eq!(Button::from_id(id), Some(btn));
            assert_eq!(btn.as_id(), id);
        }
        assert_eq!(Button::from_id(5), None);
        assert_eq!(Button::from_id(255), None);
    }

    #[test]
    fn button_action_round_trips() {
        assert_eq!(ButtonAction::SoftRelease.as_u8(), 0);
        assert_eq!(ButtonAction::Press.as_u8(), 1);
        assert_eq!(ButtonAction::ForceRelease.as_u8(), 2);
        assert_eq!(ButtonAction::from_u8(0), Some(ButtonAction::SoftRelease));
        assert_eq!(ButtonAction::from_u8(1), Some(ButtonAction::Press));
        assert_eq!(ButtonAction::from_u8(2), Some(ButtonAction::ForceRelease));
        assert_eq!(ButtonAction::from_u8(3), None);
    }

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

    #[test]
    fn log_level_from_u8() {
        assert_eq!(LogLevel::from_u8(0), LogLevel::Error);
        assert_eq!(LogLevel::from_u8(1), LogLevel::Warn);
        assert_eq!(LogLevel::from_u8(2), LogLevel::Info);
        assert_eq!(LogLevel::from_u8(3), LogLevel::Debug);
        assert_eq!(LogLevel::from_u8(4), LogLevel::Verbose);
        assert_eq!(LogLevel::from_u8(5), LogLevel::Info);
        assert_eq!(LogLevel::from_u8(255), LogLevel::Info);
    }

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

    #[test]
    fn health_from_flags_all_set() {
        let h = Health::from_flags(0x0F);
        assert!(h.link_up && h.mouse_attached && h.clone_configured && h.injection_active);
        assert_eq!(h.to_flags(), 0x0F);
    }

    #[test]
    fn health_from_flags_only_mouse() {
        let h = Health::from_flags(0x02);
        assert!(!h.link_up);
        assert!(h.mouse_attached);
        assert!(!h.clone_configured);
        assert!(!h.injection_active);
        assert_eq!(h.to_flags(), 0x02);
    }

    #[test]
    fn health_ignores_unused_high_bits() {
        // Bits b4–b7 are unused in v1 (§4.2) and must not leak into the decoded view.
        let h = Health::from_flags(0xF0);
        assert_eq!(h, Health::from_flags(0x00));
        assert_eq!(h.to_flags(), 0x00);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn serde_snake_case_round_trip() {
        // Enum variants serialize to the protocol's own snake_case vocabulary.
        assert_eq!(serde_json::to_string(&Button::Side1).unwrap(), "\"side1\"");
        assert_eq!(
            serde_json::to_string(&ButtonAction::ForceRelease).unwrap(),
            "\"force_release\""
        );
        assert_eq!(
            serde_json::from_str::<Button>("\"side1\"").unwrap(),
            Button::Side1
        );
        assert_eq!(
            serde_json::from_str::<ButtonAction>("\"force_release\"").unwrap(),
            ButtonAction::ForceRelease
        );

        let h = Health::from_flags(0x05);
        let j = serde_json::to_string(&h).unwrap();
        assert_eq!(serde_json::from_str::<Health>(&j).unwrap(), h);
    }
}
