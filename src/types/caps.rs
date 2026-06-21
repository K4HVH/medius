//! Decoded `RESP(CAPS)` — semantic capabilities of the emulated mouse (§4.4).

use crate::protocol::opcode::{CAP_REPORT_ID, CAP_WHEEL, CAP_X, CAP_Y};

/// A semantic capability summary of the emulated mouse, parsed from its HID report descriptor.
/// Counts and booleans only — never raw HID bit offsets or field widths. All fields are zero/false
/// when no relative-axis mouse interface is bound. Use it for feature detection: a `BUTTON` for a
/// button the mouse lacks is a silent no-op, so [`Caps::n_buttons`] tells you which ids are real.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Caps {
    /// Number of buttons the mouse report carries.
    pub n_buttons: u8,
    /// Relative X axis present.
    pub has_x: bool,
    /// Relative Y axis present.
    pub has_y: bool,
    /// Wheel present.
    pub has_wheel: bool,
    /// The mouse report sits behind a HID report ID.
    pub has_report_id: bool,
    /// Number of cloned HID interfaces (`> 1` = composite).
    pub n_hid: u8,
}

impl Caps {
    /// Whether the clone is a composite (multi-HID-interface) device.
    pub fn is_composite(&self) -> bool {
        self.n_hid > 1
    }

    /// Decode a `RESP(CAPS)` payload (§4.4): `[what][n_buttons u8][axis_flags u8][n_hid u8]`.
    pub(crate) fn from_payload(p: &[u8]) -> Option<Self> {
        if p.len() < 4 {
            return None;
        }
        let axis = p[2];
        Some(Caps {
            n_buttons: p[1],
            has_x: axis & CAP_X != 0,
            has_y: axis & CAP_Y != 0,
            has_wheel: axis & CAP_WHEEL != 0,
            has_report_id: axis & CAP_REPORT_ID != 0,
            n_hid: p[3],
        })
    }
}
