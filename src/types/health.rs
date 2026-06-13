//! The decoded `RESP(HEALTH)` flags — live box status.

use crate::protocol::opcode::{H_CLONE_CFG, H_INJECT_ON, H_LINK_UP, H_MOUSE_ATT};

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

#[cfg(test)]
mod tests {
    use super::*;

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
    fn serde_round_trip() {
        let h = Health::from_flags(0x05);
        let j = serde_json::to_string(&h).unwrap();
        assert_eq!(serde_json::from_str::<Health>(&j).unwrap(), h);
    }
}
