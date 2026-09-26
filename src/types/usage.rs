//! Input vocabulary: a momentary [`Usage`] (`(class, id)`) and a relative [`Axis`].

use crate::protocol::opcode::{
    INJ_BTN, INJ_KEY, INJ_MEDIA, LOCK_AXIS_PAN, LOCK_AXIS_WHEEL, LOCK_AXIS_X, LOCK_AXIS_Y,
};
use crate::types::{Button, Key, MediaKey};

/// Class of a momentary [`Usage`]; the discriminant is the wire `class` byte (as in `INJECT`).
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Class {
    /// Mouse button (id = button id, 0=Left..4=Side2).
    Button = INJ_BTN,
    /// Keyboard key (id = HID keycode; 0xE0..0xE7 = modifier).
    Key = INJ_KEY,
    /// Media / Consumer usage (id = 16-bit Consumer usage).
    Media = INJ_MEDIA,
}

impl Class {
    /// Wire `class` byte.
    pub fn as_u8(self) -> u8 {
        self as u8
    }

    /// Decodes a wire `class` byte; `None` if unknown.
    pub fn from_u8(v: u8) -> Option<Class> {
        Some(match v {
            INJ_BTN => Class::Button,
            INJ_KEY => Class::Key,
            INJ_MEDIA => Class::Media,
            _ => return None,
        })
    }
}

/// Momentary input usage: a `(class, id)` pair (button, key or media usage).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Usage {
    /// Input class.
    pub class: Class,
    /// Class-specific id (button id / HID keycode / 16-bit Consumer usage).
    pub id: u16,
}

impl Usage {
    /// Usage from an explicit class and id.
    pub const fn new(class: Class, id: u16) -> Usage {
        Usage { class, id }
    }

    /// Wire `(class, id)`.
    pub fn class_id(self) -> (u8, u16) {
        (self.class as u8, self.id)
    }

    // Appends `[class u8][id u16 LE]`.
    #[cfg(feature = "mock")]
    pub(crate) fn push_le(self, out: &mut Vec<u8>) {
        out.push(self.class as u8);
        out.extend_from_slice(&self.id.to_le_bytes());
    }

    // `[n u8]` then `n × [class u8][id u16 LE]`; `None` on a malformed buffer.
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

/// Relative axis: continuous, signed, no press/release edge.
#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Axis {
    /// X cursor axis.
    X = LOCK_AXIS_X,
    /// Y cursor axis.
    Y = LOCK_AXIS_Y,
    /// Wheel (vertical scroll).
    Wheel = LOCK_AXIS_WHEEL,
    /// AC Pan (horizontal scroll): signed and detented like the wheel, a full peer of it.
    Pan = LOCK_AXIS_PAN,
}

impl Axis {
    /// Every relative axis, in wire-id order: X, Y, wheel, pan.
    pub const ALL: [Axis; 4] = [Axis::X, Axis::Y, Axis::Wheel, Axis::Pan];

    /// Wire axis id (0=X, 1=Y, 2=wheel, 3=pan).
    pub fn as_u16(self) -> u16 {
        self as u16
    }

    /// Decodes a wire axis id; `None` for an id no axis names. Total over `u16`.
    pub fn from_u16(id: u16) -> Option<Axis> {
        Some(match id {
            LOCK_AXIS_X => Axis::X,
            LOCK_AXIS_Y => Axis::Y,
            LOCK_AXIS_WHEEL => Axis::Wheel,
            LOCK_AXIS_PAN => Axis::Pan,
            _ => return None,
        })
    }
}
