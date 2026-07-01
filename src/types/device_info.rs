//! Decoded `RESP(DEVICE_INFO)` — the cloned device's USB identity, kind, and product (§4.3).

use core::fmt;

use crate::protocol::opcode::{DI_HAS_BOS, DI_HAS_SERIAL};

/// The cloned device's primary kind, from its Boot-interface `bInterfaceProtocol`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum DeviceKind {
    /// No Boot interface, or nothing cloned yet.
    #[default]
    Unknown,
    Keyboard,
    Mouse,
}

impl DeviceKind {
    pub(crate) fn from_u8(v: u8) -> DeviceKind {
        match v {
            1 => DeviceKind::Keyboard,
            2 => DeviceKind::Mouse,
            _ => DeviceKind::Unknown,
        }
    }
}

impl fmt::Display for DeviceKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            DeviceKind::Unknown => "unknown",
            DeviceKind::Keyboard => "keyboard",
            DeviceKind::Mouse => "mouse",
        })
    }
}

/// The cloned device's USB identity, read from its descriptors. All numeric fields are zero, both
/// flags false, `kind` [`DeviceKind::Unknown`] and `product` empty when nothing is cloned. The
/// control host cannot otherwise see this — the clone sits on the game PC's bus, not the control link.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct DeviceInfo {
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
    /// The cloned device's primary kind (Boot-interface protocol).
    pub kind: DeviceKind,
    /// The cloned device's `iProduct` string (empty if it serves none).
    pub product: String,
}

impl DeviceInfo {
    /// Decode a `RESP(DEVICE_INFO)` payload (§4.3):
    /// `[what][vid u16][pid u16][bcd_device u16][bcd_usb u16][flags u8][primary_kind u8][product UTF-8…]`.
    pub(crate) fn from_payload(p: &[u8]) -> Option<Self> {
        if p.len() < 11 {
            return None;
        }
        let flags = p[9];
        Some(DeviceInfo {
            vid: u16::from_le_bytes([p[1], p[2]]),
            pid: u16::from_le_bytes([p[3], p[4]]),
            bcd_device: u16::from_le_bytes([p[5], p[6]]),
            bcd_usb: u16::from_le_bytes([p[7], p[8]]),
            has_serial: flags & DI_HAS_SERIAL != 0,
            has_bos: flags & DI_HAS_BOS != 0,
            kind: DeviceKind::from_u8(p[10]),
            product: String::from_utf8_lossy(&p[11..]).into_owned(),
        })
    }
}

impl fmt::Display for DeviceInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.product.is_empty() {
            write!(f, "{:04X}:{:04X}", self.vid, self.pid)
        } else {
            write!(f, "{:04X}:{:04X} {}", self.vid, self.pid, self.product)
        }
    }
}
