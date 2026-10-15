//! Button command vocabulary.

use crate::protocol::opcode::{
    ACT_FORCEREL, ACT_PRESS, ACT_SOFTREL, BTN_LEFT, BTN_MIDDLE, BTN_RIGHT, BTN_SIDE1, BTN_SIDE2,
};

/// Mouse button by 0-based id (§3.3).
///
/// The five standard buttons have constants ([`Button::LEFT`] .. [`Button::SIDE2`]). Any id is
/// representable, since a gaming mouse often declares more buttons than it wires; the box drives ids up
/// to the clone's declared button count (`RESP(CAPS)`). Injecting a button the real device declares
/// but never produces is descriptor-faithful. Choosing a plausible button is the caller's job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Button(pub u8);

impl Button {
    /// Left button.
    pub const LEFT: Button = Button(BTN_LEFT);
    /// Right button.
    pub const RIGHT: Button = Button(BTN_RIGHT);
    /// Middle button.
    pub const MIDDLE: Button = Button(BTN_MIDDLE);
    /// First side button.
    pub const SIDE1: Button = Button(BTN_SIDE1);
    /// Second side button.
    pub const SIDE2: Button = Button(BTN_SIDE2);

    /// Button by 0-based id; the box caps it at the declared count.
    pub const fn new(id: u8) -> Button {
        Button(id)
    }

    /// Wire `id` byte (§3.3).
    pub const fn as_id(self) -> u8 {
        self.0
    }

    /// Decodes a wire `id` byte; every id is a valid button.
    pub const fn from_id(id: u8) -> Button {
        Button(id)
    }
}

/// Injection override action (§3.3); discriminants are the wire `action` byte.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Action {
    /// Clear the injected press; the native state applies.
    SoftRelease = ACT_SOFTREL,
    /// Force the button down regardless of native state.
    Press = ACT_PRESS,
    /// Force the button up, masking a hold on the real device.
    ForceRelease = ACT_FORCEREL,
}

impl Action {
    /// Wire `action` byte.
    pub fn as_u8(self) -> u8 {
        self as u8
    }

    /// Decodes a wire `action` byte; `None` if unknown.
    pub fn from_u8(v: u8) -> Option<Self> {
        Some(match v {
            ACT_SOFTREL => Action::SoftRelease,
            ACT_PRESS => Action::Press,
            ACT_FORCEREL => Action::ForceRelease,
            _ => return None,
        })
    }
}
