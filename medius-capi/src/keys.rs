//! HID keyboard and Consumer usage constants, mirroring `medius::Key` / `medius::MediaKey`. Any raw
//! usage is also valid; these are the common ones.

use crate::ctypes::{MediusKey, MediusMediaKey};

// Letters.
pub const MEDIUS_KEY_A: MediusKey = 0x04;
pub const MEDIUS_KEY_B: MediusKey = 0x05;
pub const MEDIUS_KEY_C: MediusKey = 0x06;
pub const MEDIUS_KEY_D: MediusKey = 0x07;
pub const MEDIUS_KEY_E: MediusKey = 0x08;
pub const MEDIUS_KEY_F: MediusKey = 0x09;
pub const MEDIUS_KEY_G: MediusKey = 0x0A;
pub const MEDIUS_KEY_H: MediusKey = 0x0B;
pub const MEDIUS_KEY_I: MediusKey = 0x0C;
pub const MEDIUS_KEY_J: MediusKey = 0x0D;
pub const MEDIUS_KEY_K: MediusKey = 0x0E;
pub const MEDIUS_KEY_L: MediusKey = 0x0F;
pub const MEDIUS_KEY_M: MediusKey = 0x10;
pub const MEDIUS_KEY_N: MediusKey = 0x11;
pub const MEDIUS_KEY_O: MediusKey = 0x12;
pub const MEDIUS_KEY_P: MediusKey = 0x13;
pub const MEDIUS_KEY_Q: MediusKey = 0x14;
pub const MEDIUS_KEY_R: MediusKey = 0x15;
pub const MEDIUS_KEY_S: MediusKey = 0x16;
pub const MEDIUS_KEY_T: MediusKey = 0x17;
pub const MEDIUS_KEY_U: MediusKey = 0x18;
pub const MEDIUS_KEY_V: MediusKey = 0x19;
pub const MEDIUS_KEY_W: MediusKey = 0x1A;
pub const MEDIUS_KEY_X: MediusKey = 0x1B;
pub const MEDIUS_KEY_Y: MediusKey = 0x1C;
pub const MEDIUS_KEY_Z: MediusKey = 0x1D;

// Digit row, 1 through 0.
pub const MEDIUS_KEY_1: MediusKey = 0x1E;
pub const MEDIUS_KEY_2: MediusKey = 0x1F;
pub const MEDIUS_KEY_3: MediusKey = 0x20;
pub const MEDIUS_KEY_4: MediusKey = 0x21;
pub const MEDIUS_KEY_5: MediusKey = 0x22;
pub const MEDIUS_KEY_6: MediusKey = 0x23;
pub const MEDIUS_KEY_7: MediusKey = 0x24;
pub const MEDIUS_KEY_8: MediusKey = 0x25;
pub const MEDIUS_KEY_9: MediusKey = 0x26;
pub const MEDIUS_KEY_0: MediusKey = 0x27;

// Common keys.
pub const MEDIUS_KEY_ENTER: MediusKey = 0x28;
pub const MEDIUS_KEY_ESCAPE: MediusKey = 0x29;
pub const MEDIUS_KEY_BACKSPACE: MediusKey = 0x2A;
pub const MEDIUS_KEY_TAB: MediusKey = 0x2B;
pub const MEDIUS_KEY_SPACE: MediusKey = 0x2C;
pub const MEDIUS_KEY_CAPS_LOCK: MediusKey = 0x39;
pub const MEDIUS_KEY_INSERT: MediusKey = 0x49;
pub const MEDIUS_KEY_HOME: MediusKey = 0x4A;
pub const MEDIUS_KEY_PAGE_UP: MediusKey = 0x4B;
pub const MEDIUS_KEY_DELETE: MediusKey = 0x4C;
pub const MEDIUS_KEY_END: MediusKey = 0x4D;
pub const MEDIUS_KEY_PAGE_DOWN: MediusKey = 0x4E;
pub const MEDIUS_KEY_RIGHT: MediusKey = 0x4F;
pub const MEDIUS_KEY_LEFT: MediusKey = 0x50;
pub const MEDIUS_KEY_DOWN: MediusKey = 0x51;
pub const MEDIUS_KEY_UP: MediusKey = 0x52;

// Function row.
pub const MEDIUS_KEY_F1: MediusKey = 0x3A;
pub const MEDIUS_KEY_F2: MediusKey = 0x3B;
pub const MEDIUS_KEY_F3: MediusKey = 0x3C;
pub const MEDIUS_KEY_F4: MediusKey = 0x3D;
pub const MEDIUS_KEY_F5: MediusKey = 0x3E;
pub const MEDIUS_KEY_F6: MediusKey = 0x3F;
pub const MEDIUS_KEY_F7: MediusKey = 0x40;
pub const MEDIUS_KEY_F8: MediusKey = 0x41;
pub const MEDIUS_KEY_F9: MediusKey = 0x42;
pub const MEDIUS_KEY_F10: MediusKey = 0x43;
pub const MEDIUS_KEY_F11: MediusKey = 0x44;
pub const MEDIUS_KEY_F12: MediusKey = 0x45;

// Modifiers.
pub const MEDIUS_KEY_LEFT_CTRL: MediusKey = 0xE0;
pub const MEDIUS_KEY_LEFT_SHIFT: MediusKey = 0xE1;
pub const MEDIUS_KEY_LEFT_ALT: MediusKey = 0xE2;
pub const MEDIUS_KEY_LEFT_GUI: MediusKey = 0xE3;
pub const MEDIUS_KEY_RIGHT_CTRL: MediusKey = 0xE4;
pub const MEDIUS_KEY_RIGHT_SHIFT: MediusKey = 0xE5;
pub const MEDIUS_KEY_RIGHT_ALT: MediusKey = 0xE6;
pub const MEDIUS_KEY_RIGHT_GUI: MediusKey = 0xE7;

// Media (Consumer usages).
pub const MEDIUS_MEDIA_PLAY_PAUSE: MediusMediaKey = 0xCD;
pub const MEDIUS_MEDIA_STOP: MediusMediaKey = 0xB7;
pub const MEDIUS_MEDIA_NEXT_TRACK: MediusMediaKey = 0xB5;
pub const MEDIUS_MEDIA_PREV_TRACK: MediusMediaKey = 0xB6;
pub const MEDIUS_MEDIA_MUTE: MediusMediaKey = 0xE2;
pub const MEDIUS_MEDIA_VOLUME_UP: MediusMediaKey = 0xE9;
pub const MEDIUS_MEDIA_VOLUME_DOWN: MediusMediaKey = 0xEA;
pub const MEDIUS_MEDIA_PLAY: MediusMediaKey = 0xB0;
pub const MEDIUS_MEDIA_PAUSE: MediusMediaKey = 0xB1;
