//! Keyboard capabilities: the keyboard half of the unified `RESP(CAPS)` (§4.4).

/// A semantic capability summary of the cloned keyboard; the keyboard half of [`Caps`](crate::Caps).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct KbdCaps {
    /// Keycode-array slots the report carries, or `0xFF` when the keyboard uses an NKRO bitmap.
    pub n_keys: u8,
    /// Keys are an NKRO bitmap (no rollover limit), rather than a fixed keycode array.
    pub nkro: bool,
    /// A Consumer (media-key) collection is present; media injection/catch is available.
    pub has_consumer: bool,
    /// A System-control collection is present (passthrough-only; not injectable).
    pub has_system: bool,
    /// The keyboard report sits behind a HID report ID.
    pub has_report_id: bool,
}
