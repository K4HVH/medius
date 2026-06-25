//! Mouse capabilities — the mouse half of the unified `RESP(CAPS)` (§4.4).

/// A semantic capability summary of the emulated mouse, parsed from its HID report descriptor. Counts
/// and booleans only — never raw HID bit offsets or field widths. All fields are zero/false when no
/// relative-axis mouse interface is bound; the mouse half of [`Caps`](crate::Caps). Use it for feature
/// detection: an `inject` for a button the mouse lacks is a silent no-op, so [`MouseCaps::n_buttons`]
/// tells you which ids are real.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct MouseCaps {
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

impl MouseCaps {
    /// Whether the clone is a composite (multi-HID-interface) device.
    pub fn is_composite(&self) -> bool {
        self.n_hid > 1
    }
}
