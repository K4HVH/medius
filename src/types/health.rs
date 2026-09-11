//! Decoded `RESP(HEALTH)` flags.

use crate::protocol::opcode::{
    H_CATCH_ON, H_CLONE_CFG, H_INJECT_ON, H_KBD_ATT, H_LINK_UP, H_LOCK_ON, H_MOUSE_ATT, H_PATCH_ON,
    H_RATE_CONFIDENT, H_REWRITE_ON, H_TRANSFORM_ON,
};

/// The decoded `RESP(HEALTH)` flags word.
///
/// `HEALTH` is a `u16` LE from `CTRL_PROTO_VER 7` (§4.2): bits 0-7 are the original byte and the
/// v3.4.0 advanced control layer opened the high byte. [`from_flags`](Self::from_flags) and
/// [`to_flags`](Self::to_flags) carry the whole word.
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
    /// A catch subscription is active; physical-input events are streaming.
    pub catch_on: bool,
    /// A keyboard is attached on the host chip, cloned and injectable.
    pub kbd_attached: bool,
    /// The rewrite-rule table (§3.14) is non-empty (v3.4.0).
    pub rewrite_on: bool,
    /// A descriptor-patch set (§3.14) is applied to the clone (v3.4.0).
    pub patch_on: bool,
    /// A field transform is active (reserved; the transforms feature owns this bit) (v3.4.0).
    pub transform_on: bool,
}

impl Health {
    /// Decode a `RESP(HEALTH)` flags word (§4.2). A box that reports only the low byte (an eight-bit
    /// caller passing a `u8` that widens here) still decodes bits 0-7; the high byte reads clear.
    pub fn from_flags(flags: u16) -> Self {
        let lo = flags as u8;
        Health {
            link_up: lo & H_LINK_UP != 0,
            mouse_attached: lo & H_MOUSE_ATT != 0,
            clone_configured: lo & H_CLONE_CFG != 0,
            injection_active: lo & H_INJECT_ON != 0,
            rate_confident: lo & H_RATE_CONFIDENT != 0,
            lock_on: lo & H_LOCK_ON != 0,
            catch_on: lo & H_CATCH_ON != 0,
            kbd_attached: lo & H_KBD_ATT != 0,
            rewrite_on: flags & H_REWRITE_ON != 0,
            patch_on: flags & H_PATCH_ON != 0,
            transform_on: flags & H_TRANSFORM_ON != 0,
        }
    }

    /// Re-encode this health view back to its flags word.
    pub fn to_flags(self) -> u16 {
        let mut flags = 0u16;
        if self.link_up {
            flags |= H_LINK_UP as u16;
        }
        if self.mouse_attached {
            flags |= H_MOUSE_ATT as u16;
        }
        if self.clone_configured {
            flags |= H_CLONE_CFG as u16;
        }
        if self.injection_active {
            flags |= H_INJECT_ON as u16;
        }
        if self.rate_confident {
            flags |= H_RATE_CONFIDENT as u16;
        }
        if self.lock_on {
            flags |= H_LOCK_ON as u16;
        }
        if self.catch_on {
            flags |= H_CATCH_ON as u16;
        }
        if self.kbd_attached {
            flags |= H_KBD_ATT as u16;
        }
        if self.rewrite_on {
            flags |= H_REWRITE_ON;
        }
        if self.patch_on {
            flags |= H_PATCH_ON;
        }
        if self.transform_on {
            flags |= H_TRANSFORM_ON;
        }
        flags
    }
}
