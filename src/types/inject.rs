//! The two field-generic injection verbs (§3.1–3.2): `move` drives a relative axis, `inject` sets a
//! momentary usage. One verb per field kind, not one per device class.

use crate::types::{Button, Key, MediaKey};

/// A momentary usage to drive with the [`inject`](crate::Device::inject) verb — any input class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Input {
    Button(Button),
    Key(Key),
    Media(MediaKey),
}

impl From<Button> for Input {
    fn from(b: Button) -> Self {
        Input::Button(b)
    }
}
impl From<Key> for Input {
    fn from(k: Key) -> Self {
        Input::Key(k)
    }
}
impl From<MediaKey> for Input {
    fn from(m: MediaKey) -> Self {
        Input::Media(m)
    }
}

/// A relative axis to drive with the [`move_axis`](crate::Device::move_axis) verb.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Motion {
    Cursor { dx: i16, dy: i16 },
    Wheel(i16),
}
