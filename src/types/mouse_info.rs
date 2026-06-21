//! Decoded `RESP(MOUSE_INFO)` — the cloned mouse's USB identity (§4.3).

use core::fmt;

use crate::protocol::opcode::{MI_HAS_BOS, MI_HAS_SERIAL};

/// The cloned mouse's USB identity, read from its device descriptor. All fields are zero and both
/// flags false when no mouse is cloned. The control host cannot otherwise see this — the clone sits
/// on the game PC's bus, not the control link.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MouseInfo {
    /// `idVendor`.
    pub vid: u16,
    /// `idProduct`.
    pub pid: u16,
    /// `bcdDevice` (device release).
    pub bcd_device: u16,
    /// `bcdUSB` (e.g. `0x0200`, `0x0201`).
    pub bcd_usb: u16,
    /// The clone serves a serial string.
    pub has_serial: bool,
    /// The clone serves a BOS descriptor (`bcdUSB >= 0x0201`).
    pub has_bos: bool,
}

impl MouseInfo {
    /// Decode a `RESP(MOUSE_INFO)` payload (§4.3):
    /// `[what][vid u16][pid u16][bcd_device u16][bcd_usb u16][flags u8]`.
    pub(crate) fn from_payload(p: &[u8]) -> Option<Self> {
        if p.len() < 10 {
            return None;
        }
        let flags = p[9];
        Some(MouseInfo {
            vid: u16::from_le_bytes([p[1], p[2]]),
            pid: u16::from_le_bytes([p[3], p[4]]),
            bcd_device: u16::from_le_bytes([p[5], p[6]]),
            bcd_usb: u16::from_le_bytes([p[7], p[8]]),
            has_serial: flags & MI_HAS_SERIAL != 0,
            has_bos: flags & MI_HAS_BOS != 0,
        })
    }
}

impl fmt::Display for MouseInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04X}:{:04X}", self.vid, self.pid)
    }
}
