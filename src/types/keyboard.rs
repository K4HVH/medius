//! Keyboard command vocabulary: a key by HID keycode, and the keyboard catch snapshot (v2.0.0).

/// A keyboard key, addressed by HID Usage (Keyboard/Keypad page, §3.10).
///
/// Construct from a raw usage with [`Key::new`], or use an associated constant. Usages `0xE0..=0xE7`
/// are the eight modifiers (the firmware folds them into the report's modifier byte); every other
/// usage is a regular key. This is a thin newtype, so any HID keycode is expressible — the constants
/// are just the common ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Key(pub u8);

impl Key {
    /// A key from a raw HID Keyboard/Keypad usage.
    pub const fn new(usage: u8) -> Key {
        Key(usage)
    }

    /// The HID usage byte.
    pub const fn usage(self) -> u8 {
        self.0
    }

    /// Whether this key is one of the eight modifiers (usages `0xE0..=0xE7`).
    pub const fn is_modifier(self) -> bool {
        self.0 >= 0xE0 && self.0 <= 0xE7
    }

    // Letters (0x04..=0x1D).
    pub const A: Key = Key(0x04);
    pub const B: Key = Key(0x05);
    pub const C: Key = Key(0x06);
    pub const D: Key = Key(0x07);
    pub const E: Key = Key(0x08);
    pub const F: Key = Key(0x09);
    pub const G: Key = Key(0x0A);
    pub const H: Key = Key(0x0B);
    pub const I: Key = Key(0x0C);
    pub const J: Key = Key(0x0D);
    pub const K: Key = Key(0x0E);
    pub const L: Key = Key(0x0F);
    pub const M: Key = Key(0x10);
    pub const N: Key = Key(0x11);
    pub const O: Key = Key(0x12);
    pub const P: Key = Key(0x13);
    pub const Q: Key = Key(0x14);
    pub const R: Key = Key(0x15);
    pub const S: Key = Key(0x16);
    pub const T: Key = Key(0x17);
    pub const U: Key = Key(0x18);
    pub const V: Key = Key(0x19);
    pub const W: Key = Key(0x1A);
    pub const X: Key = Key(0x1B);
    pub const Y: Key = Key(0x1C);
    pub const Z: Key = Key(0x1D);

    // Digit row (0x1E..=0x27), 1 through 0.
    pub const NUM1: Key = Key(0x1E);
    pub const NUM2: Key = Key(0x1F);
    pub const NUM3: Key = Key(0x20);
    pub const NUM4: Key = Key(0x21);
    pub const NUM5: Key = Key(0x22);
    pub const NUM6: Key = Key(0x23);
    pub const NUM7: Key = Key(0x24);
    pub const NUM8: Key = Key(0x25);
    pub const NUM9: Key = Key(0x26);
    pub const NUM0: Key = Key(0x27);

    // Common keys.
    pub const ENTER: Key = Key(0x28);
    pub const ESCAPE: Key = Key(0x29);
    pub const BACKSPACE: Key = Key(0x2A);
    pub const TAB: Key = Key(0x2B);
    pub const SPACE: Key = Key(0x2C);
    pub const CAPS_LOCK: Key = Key(0x39);
    pub const INSERT: Key = Key(0x49);
    pub const HOME: Key = Key(0x4A);
    pub const PAGE_UP: Key = Key(0x4B);
    pub const DELETE: Key = Key(0x4C);
    pub const END: Key = Key(0x4D);
    pub const PAGE_DOWN: Key = Key(0x4E);
    pub const RIGHT: Key = Key(0x4F);
    pub const LEFT: Key = Key(0x50);
    pub const DOWN: Key = Key(0x51);
    pub const UP: Key = Key(0x52);

    // Function row (0x3A..=0x45).
    pub const F1: Key = Key(0x3A);
    pub const F2: Key = Key(0x3B);
    pub const F3: Key = Key(0x3C);
    pub const F4: Key = Key(0x3D);
    pub const F5: Key = Key(0x3E);
    pub const F6: Key = Key(0x3F);
    pub const F7: Key = Key(0x40);
    pub const F8: Key = Key(0x41);
    pub const F9: Key = Key(0x42);
    pub const F10: Key = Key(0x43);
    pub const F11: Key = Key(0x44);
    pub const F12: Key = Key(0x45);

    // Modifiers (0xE0..=0xE7).
    pub const LEFT_CTRL: Key = Key(0xE0);
    pub const LEFT_SHIFT: Key = Key(0xE1);
    pub const LEFT_ALT: Key = Key(0xE2);
    pub const LEFT_GUI: Key = Key(0xE3);
    pub const RIGHT_CTRL: Key = Key(0xE4);
    pub const RIGHT_SHIFT: Key = Key(0xE5);
    pub const RIGHT_ALT: Key = Key(0xE6);
    pub const RIGHT_GUI: Key = Key(0xE7);
}

/// A keyboard catch snapshot — a `KB_EVENT` frame (§4.12, v2.0.0).
///
/// Carries the modifier bitmap and every currently-pressed key, so it is self-correcting: a dropped
/// frame is recovered by the next one. Diff successive snapshots for down/up edges, or use
/// [`is_pressed`](Self::is_pressed) for the current state of one key.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct KeyboardEvent {
    /// The modifier bitmap: bit `m` (0..8) is the modifier at usage `0xE0 + m`.
    pub modifiers: u8,
    /// Every currently-pressed non-modifier key, ascending by usage.
    pub keys: Vec<Key>,
}

impl KeyboardEvent {
    /// Decode a `KB_EVENT` payload: `[modifiers u8][n u8][keycode u8 × n]`.
    pub(crate) fn from_payload(p: &[u8]) -> Option<KeyboardEvent> {
        if p.len() < 2 {
            return None;
        }
        let n = p[1] as usize;
        if p.len() < 2 + n {
            return None; // truncated: fewer keycodes than the count claims
        }
        let keys = p[2..2 + n].iter().map(|&u| Key(u)).collect();
        Some(KeyboardEvent {
            modifiers: p[0],
            keys,
        })
    }

    /// Whether `key` is held in this snapshot — a modifier is read from the modifier bitmap, any other
    /// key from the pressed set.
    pub fn is_pressed(&self, key: Key) -> bool {
        if key.is_modifier() {
            self.modifiers & (1 << (key.0 - 0xE0)) != 0
        } else {
            self.keys.contains(&key)
        }
    }
}
