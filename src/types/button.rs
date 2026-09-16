//! Button command vocabulary.

use crate::protocol::opcode::{
    ACT_FORCEREL, ACT_PRESS, ACT_SOFTREL, BTN_LEFT, BTN_MIDDLE, BTN_RIGHT, BTN_SIDE1, BTN_SIDE2,
};

/// A mouse button, addressed by its 0-based id (§3.3).
///
/// The five standard buttons have named constants ([`Button::LEFT`] .. [`Button::SIDE2`]); a gaming
/// mouse commonly declares more than it wires, so any id is representable and the box drives it up to
/// the clone's declared button count (`RESP(CAPS)`). A button the device declares but never itself
/// produces is a descriptor-faithful injection; shaping which button is natural is the caller's job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Button(pub u8);

impl Button {
    /// The left button.
    pub const LEFT: Button = Button(BTN_LEFT);
    /// The right button.
    pub const RIGHT: Button = Button(BTN_RIGHT);
    /// The middle button.
    pub const MIDDLE: Button = Button(BTN_MIDDLE);
    /// The first side button.
    pub const SIDE1: Button = Button(BTN_SIDE1);
    /// The second side button.
    pub const SIDE2: Button = Button(BTN_SIDE2);

    /// A button from its 0-based id. Any id is representable; the box caps it at the declared count.
    pub const fn new(id: u8) -> Button {
        Button(id)
    }

    /// The wire `id` byte for this button (§3.3).
    pub const fn as_id(self) -> u8 {
        self.0
    }

    /// Map a wire `id` byte to a [`Button`]. Total: every id is a valid button.
    pub const fn from_id(id: u8) -> Button {
        Button(id)
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
