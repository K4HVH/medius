//! Decoded `RESP(KBD_CAPS)` — semantic capabilities of the cloned keyboard (§4.11, v1.7.0).

use crate::protocol::opcode::{KBC_CONSUMER, KBC_NKRO, KBC_REPORT_ID, KBC_SYSTEM};

/// A semantic capability summary of the cloned keyboard. Counts and booleans only — never raw HID bit
/// offsets. All fields are zero/false when no keyboard interface is bound (check
/// [`Health::kbd_attached`](crate::Health::kbd_attached) first). Use it for feature detection: a media
/// key on a board with no Consumer collection is a silent no-op, so [`has_consumer`](Self::has_consumer)
/// tells you whether media injection is real.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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

impl KbdCaps {
    /// Decode a `RESP(KBD_CAPS)` payload (§4.11): `[what][n_keys u8][flags u8]`.
    pub(crate) fn from_payload(p: &[u8]) -> Option<KbdCaps> {
        if p.len() < 3 {
            return None;
        }
        let flags = p[2];
        Some(KbdCaps {
            n_keys: p[1],
            nkro: flags & KBC_NKRO != 0,
            has_consumer: flags & KBC_CONSUMER != 0,
            has_system: flags & KBC_SYSTEM != 0,
            has_report_id: flags & KBC_REPORT_ID != 0,
        })
    }
}
