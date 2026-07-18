//! The unified input vocabulary: a momentary [`Usage`] (`(class, id)`) and a relative [`Axis`].

use crate::protocol::opcode::{
    INJ_BTN, INJ_KEY, INJ_MEDIA, LOCK_AXIS_WHEEL, LOCK_AXIS_X, LOCK_AXIS_Y,
};
use crate::types::{Button, Key, MediaKey};

/// The class of a momentary [`Usage`]; the discriminant is the wire `class` byte (matches `INJECT`).
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Class {
    /// A mouse button (id = button id, 0=Left..4=Side2).
    Button = INJ_BTN,
    /// A keyboard key (id = HID keycode; 0xE0..0xE7 = modifier).
    Key = INJ_KEY,
    /// A media / Consumer usage (id = 16-bit Consumer usage).
    Media = INJ_MEDIA,
}

impl Class {
    /// The wire `class` byte.
    pub fn as_u8(self) -> u8 {
        self as u8
    }

    /// Map a wire `class` byte to a [`Class`], or `None` for an unknown value.
    pub fn from_u8(v: u8) -> Option<Class> {
        Some(match v {
            INJ_BTN => Class::Button,
            INJ_KEY => Class::Key,
            INJ_MEDIA => Class::Media,
            _ => return None,
        })
    }
}

/// A momentary input usage: a `(class, id)` pair (button, key, or media usage).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Usage {
    /// The input class.
    pub class: Class,
    /// The class-specific id (button id / HID keycode / 16-bit Consumer usage).
    pub id: u16,
}

impl Usage {
    /// A usage from an explicit class and id.
    pub const fn new(class: Class, id: u16) -> Usage {
        Usage { class, id }
    }

    /// The wire `(class, id)` this usage encodes to.
    pub fn class_id(self) -> (u8, u16) {
        (self.class as u8, self.id)
    }

    /// Append this usage's wire bytes `[class u8][id u16 LE]` to `out`.
    pub(crate) fn push_le(self, out: &mut Vec<u8>) {
        out.push(self.class as u8);
        out.extend_from_slice(&self.id.to_le_bytes());
    }

    /// Decode a length-prefixed usage list `[n u8]` then `n × [class u8][id u16 LE]`, `None` on a malformed buffer.
    pub(crate) fn decode_list(p: &[u8]) -> Option<Vec<Usage>> {
        let n = *p.first()? as usize;
        let mut out = Vec::with_capacity(n);
        for i in 0..n {
            let off = 1 + 3 * i;
            let class = Class::from_u8(*p.get(off)?)?;
            let id = u16::from_le_bytes([*p.get(off + 1)?, *p.get(off + 2)?]);
            out.push(Usage::new(class, id));
        }
        Some(out)
    }
}

impl From<Button> for Usage {
    fn from(b: Button) -> Usage {
        Usage::new(Class::Button, b.as_id() as u16)
    }
}
impl From<Key> for Usage {
    fn from(k: Key) -> Usage {
        Usage::new(Class::Key, k.usage() as u16)
    }
}
impl From<MediaKey> for Usage {
    fn from(m: MediaKey) -> Usage {
        Usage::new(Class::Media, m.usage())
    }
}

/// A relative axis: continuous, signed, no press/release edge.
#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Axis {
    /// The X cursor axis.
    X = LOCK_AXIS_X,
    /// The Y cursor axis.
    Y = LOCK_AXIS_Y,
    /// The wheel.
    Wheel = LOCK_AXIS_WHEEL,
}

impl Axis {
    /// The wire axis id (0=X, 1=Y, 2=wheel).
    pub fn as_u16(self) -> u16 {
        self as u16
    }
}
