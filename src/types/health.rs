//! The decoded `RESP(HEALTH)` flags — live box status.

use crate::protocol::opcode::{H_CLONE_CFG, H_INJECT_ON, H_LINK_UP, H_MOUSE_ATT};

/// The decoded `RESP(HEALTH)` flags byte (§4.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
