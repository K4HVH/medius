//! `LED` control vocabulary (§3.7).

/// Status LED a `LED` command targets; discriminants are the wire `target` byte.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LedTarget {
    Device = 0,
    Host = 1,
    Both = 2,
}

impl LedTarget {
    /// Wire `target` byte.
    pub fn as_u8(self) -> u8 {
        self as u8
    }

    /// Decodes a wire `target` byte; `None` if unknown.
    pub fn from_u8(v: u8) -> Option<Self> {
        Some(match v {
            0 => LedTarget::Device,
            1 => LedTarget::Host,
            2 => LedTarget::Both,
            _ => return None,
        })
    }
}

/// LED state a `LED` command sets; `Auto` restores the box's status display.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LedMode {
    Auto = 0,
    Off = 1,
    Solid = 2,
    Blink = 3,
}

impl LedMode {
    /// Wire `mode` byte.
    pub fn as_u8(self) -> u8 {
        self as u8
    }

    /// Decodes a wire `mode` byte; `None` if unknown.
    pub fn from_u8(v: u8) -> Option<Self> {
        Some(match v {
            0 => LedMode::Auto,
            1 => LedMode::Off,
            2 => LedMode::Solid,
            3 => LedMode::Blink,
            _ => return None,
        })
    }
}
