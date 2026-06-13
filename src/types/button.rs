//! Button command vocabulary — the five mouse buttons and the three injection-override actions.

use crate::protocol::opcode::{
    ACT_FORCEREL, ACT_PRESS, ACT_SOFTREL, BTN_LEFT, BTN_MIDDLE, BTN_RIGHT, BTN_SIDE1, BTN_SIDE2,
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
    }
}
