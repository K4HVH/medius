//! `LED` control vocabulary (§3.7).

/// Which status LED a `LED` command targets; discriminants are the wire `target` byte.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LedTarget {
    Device = 0,
    Host = 1,
    Both = 2,
}

impl LedTarget {
    /// The wire `target` byte.
    pub fn as_u8(self) -> u8 {
        self as u8
    }

    /// Map a wire `target` byte to a [`LedTarget`], or `None` if unknown.
    pub fn from_u8(v: u8) -> Option<Self> {
        Some(match v {
            0 => LedTarget::Device,
            1 => LedTarget::Host,
            2 => LedTarget::Both,
            _ => return None,
        })
    }
}

/// What a `LED` command drives the LED to; `Auto` hands it back to the box's status display.
/// Discriminants are the wire `mode` byte.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LedMode {
    Auto = 0,
    Off = 1,
    Solid = 2,
    Blink = 3,
}

impl LedMode {
    /// The wire `mode` byte.
    pub fn as_u8(self) -> u8 {
        self as u8
    }

    /// Map a wire `mode` byte to a [`LedMode`], or `None` if unknown.
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
