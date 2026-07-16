//! The unified input vocabulary: a momentary [`Usage`] (`(class, id)` — a button, key, or media usage,
//! all one shape) and a relative [`Axis`]. `INJECT`, `LOCK`, `CATCH`, and clip playback all speak these,
//! so a mouse button is addressed exactly like a key or a media usage. The friendly [`Button`], [`Key`],
//! and [`MediaKey`] types are constructors that convert into a `Usage`.

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

/// A momentary input usage: a `(class, id)` pair. A button, a key (modifiers are ids `0xE0..=0xE7`), and
/// a media usage are all one shape, so every verb that drives a momentary input takes a `Usage`. Build one
/// from a [`Button`], [`Key`], or [`MediaKey`] (they all `impl Into<Usage>`), or directly with [`Usage::new`].
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

/// A relative axis — the one genuinely mouse-hardware-specific input kind (continuous, signed, no
/// press/release edge). Driven by [`move_axis`](crate::Device::move_axis) and lockable by
/// [`lock_axis`](crate::Device::lock_axis).
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
