//! Button command vocabulary.

use crate::protocol::opcode::{
    ACT_FORCEREL, ACT_PRESS, ACT_SOFTREL, BTN_LEFT, BTN_MIDDLE, BTN_RIGHT, BTN_SIDE1, BTN_SIDE2,
};

/// One of the five standard mouse buttons (§3.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
pub enum Action {
    /// Clear our injected press; defer to physical state.
    SoftRelease = ACT_SOFTREL,
    /// Force the button down regardless of physical state.
    Press = ACT_PRESS,
    /// Force the button up, masking a physical hold.
    ForceRelease = ACT_FORCEREL,
}

impl Action {
    /// The wire `action` byte for this action.
    pub fn as_u8(self) -> u8 {
        self as u8
    }

    /// Map a wire `action` byte to a [`Action`], or `None` for an unknown value.
    pub fn from_u8(v: u8) -> Option<Self> {
        Some(match v {
            ACT_SOFTREL => Action::SoftRelease,
            ACT_PRESS => Action::Press,
            ACT_FORCEREL => Action::ForceRelease,
            _ => return None,
        })
    }
}
