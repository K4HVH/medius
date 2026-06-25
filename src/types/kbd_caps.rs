//! Keyboard capabilities — the keyboard half of the unified `RESP(CAPS)` (§4.4).

/// A semantic capability summary of the cloned keyboard. Counts and booleans only — never raw HID bit
/// offsets. All fields are zero/false when no keyboard interface is bound (check
/// [`Health::kbd_attached`](crate::Health::kbd_attached) first); the keyboard half of
/// [`Caps`](crate::Caps). Use it for feature detection: a media key on a board with no Consumer
/// collection is a silent no-op, so [`has_consumer`](Self::has_consumer) tells you whether media
/// injection is real.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct KbdCaps {
    /// Keycode-array slots the report carries, or `0xFF` when the keyboard uses an NKRO bitmap.
    pub n_keys: u8,
    /// Keys are an NKRO bitmap (no rollover limit), rather than a fixed keycode array.
    pub nkro: bool,
    /// A Consumer (media-key) collection is present — media injection/catch is available.
    pub has_consumer: bool,
    /// A System-control collection is present (passthrough-only; not injectable).
    pub has_system: bool,
    /// The keyboard report sits behind a HID report ID.
    pub has_report_id: bool,
}
