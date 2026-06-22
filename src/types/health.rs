//! Decoded `RESP(HEALTH)` flags.

use crate::protocol::opcode::{
    H_CLONE_CFG, H_INJECT_ON, H_LINK_UP, H_LOCK_ON, H_MOUSE_ATT, H_RATE_CONFIDENT,
};

/// The decoded `RESP(HEALTH)` flags byte.
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
    /// The native-rate estimator window is full, so the [`Rate`](crate::Rate) value is trustworthy.
    pub rate_confident: bool,
    /// At least one lock is active.
    pub lock_on: bool,
}

impl Health {
    /// Decode a `RESP(HEALTH)` flags byte.
    pub fn from_flags(flags: u8) -> Self {
        Health {
            link_up: flags & H_LINK_UP != 0,
            mouse_attached: flags & H_MOUSE_ATT != 0,
            clone_configured: flags & H_CLONE_CFG != 0,
            injection_active: flags & H_INJECT_ON != 0,
            rate_confident: flags & H_RATE_CONFIDENT != 0,
            lock_on: flags & H_LOCK_ON != 0,
        }
    }

    /// Re-encode this health view back to its flags byte.
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
        if self.rate_confident {
            flags |= H_RATE_CONFIDENT;
        }
        if self.lock_on {
            flags |= H_LOCK_ON;
        }
        flags
    }
}
