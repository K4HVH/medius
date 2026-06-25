//! Decoded `RESP(CAPS)` — unified capabilities of the whole cloned device (§4.4).

use super::{KbdCaps, MouseCaps};
use crate::protocol::opcode::{
    CAPS_CD_KBD, CAPS_CD_MOUSE, CAP_REPORT_ID, CAP_WHEEL, CAP_X, CAP_Y, KBC_CONSUMER, KBC_NKRO,
    KBC_REPORT_ID, KBC_SYSTEM,
};

/// A semantic capability summary of the whole cloned device, mouse and keyboard, from one
/// [`caps()`](crate::Device::caps) query. Counts and booleans only — never raw HID bit offsets. A class
/// that is not present reads all-zero/false. Use it for feature detection: an `inject` for a usage the
/// device lacks is a silent no-op, so the counts tell you what is real.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Caps {
    /// Mouse capabilities (all-zero when no mouse is bound).
    pub mouse: MouseCaps,
    /// Keyboard capabilities (all-zero when no keyboard is bound).
    pub keyboard: KbdCaps,
    /// The mouse class is change-driven. Always `false`: mouse motion is continuous, so its
    /// [`Rate`](crate::Rate) carries a learned native cadence.
    pub mouse_change_driven: bool,
    /// The keyboard/media class is change-driven. `true` when a keyboard is bound: it reports only on a
    /// key change, so its [`Rate`](crate::Rate) has no continuous cadence (`native_hz` is structurally
    /// `None`, not not-yet-learned).
    pub kbd_change_driven: bool,
}

impl Caps {
    /// Whether a mouse interface is bound.
    pub fn has_mouse(&self) -> bool {
        self.mouse.n_buttons > 0 || self.mouse.has_x || self.mouse.has_y || self.mouse.has_wheel
    }

    /// Whether a keyboard interface is bound.
    pub fn has_keyboard(&self) -> bool {
        self.keyboard.n_keys > 0 || self.keyboard.has_consumer || self.keyboard.has_system
    }

    /// Whether the clone is a composite (multi-HID-interface) device.
    pub fn is_composite(&self) -> bool {
        self.mouse.n_hid > 1
    }

    /// Decode a `RESP(CAPS)` payload (§4.4):
    /// `[what][n_buttons][axis_flags][n_hid][n_keys][kbd_flags][change_driven]`.
    pub(crate) fn from_payload(p: &[u8]) -> Option<Caps> {
        if p.len() < 7 {
            return None;
        }
        let axis = p[2];
        let kf = p[5];
        let cd = p[6];
        Some(Caps {
            mouse: MouseCaps {
                n_buttons: p[1],
                has_x: axis & CAP_X != 0,
                has_y: axis & CAP_Y != 0,
                has_wheel: axis & CAP_WHEEL != 0,
                has_report_id: axis & CAP_REPORT_ID != 0,
                n_hid: p[3],
            },
            keyboard: KbdCaps {
                n_keys: p[4],
                nkro: kf & KBC_NKRO != 0,
                has_consumer: kf & KBC_CONSUMER != 0,
                has_system: kf & KBC_SYSTEM != 0,
                has_report_id: kf & KBC_REPORT_ID != 0,
            },
            mouse_change_driven: cd & CAPS_CD_MOUSE != 0,
            kbd_change_driven: cd & CAPS_CD_KBD != 0,
        })
    }
}
