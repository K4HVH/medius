//! `TRANSFER` (§3.14) vocabulary: the USB setup packet a host sends, and the device's answer.
//!
//! [`Device::transfer`](crate::Device::transfer) runs one control transfer against the real device on
//! the host chip and returns its actual answer, riding its own inter-chip link pair rather than the
//! game PC's EP0 proxy. It is single-outstanding: issue one and wait for the reply.

/// A USB control-transfer setup packet: the eight bytes of `bmRequestType`, `bRequest`, `wValue`,
/// `wIndex`, `wLength` (§9.3 of the USB spec), little-endian on the wire.
///
/// `length` is the transfer's data-stage length: for an IN request it is how many bytes to read back
/// (the device may return fewer); for an OUT request it is the length of the data you pass to
/// [`transfer`](crate::Device::transfer), which the box carries after the setup packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Setup {
    /// `bmRequestType`: direction (b7), type and recipient.
    pub request_type: u8,
    /// `bRequest`: the request code.
    pub request: u8,
    /// `wValue`: request-specific.
    pub value: u16,
    /// `wIndex`: request-specific (often an interface or endpoint).
    pub index: u16,
    /// `wLength`: the data-stage length (see the type docs).
    pub length: u16,
}

impl Setup {
    /// A setup packet from its five fields.
    pub fn new(request_type: u8, request: u8, value: u16, index: u16, length: u16) -> Setup {
        Setup {
            request_type,
            request,
            value,
            index,
            length,
        }
    }

    /// The eight setup bytes as they go on the wire (`<BBHHH>`, little-endian).
    pub fn to_bytes(self) -> [u8; 8] {
        let v = self.value.to_le_bytes();
        let i = self.index.to_le_bytes();
        let l = self.length.to_le_bytes();
        [
            self.request_type,
            self.request,
            v[0],
            v[1],
            i[0],
            i[1],
            l[0],
            l[1],
        ]
    }

    /// Whether this is a device-to-host (IN) request, from the direction bit of `bmRequestType`.
    pub fn is_in(self) -> bool {
        self.request_type & 0x80 != 0
    }
}

/// How a [`transfer`](crate::Device::transfer) ended, from the `status` byte of `TRANSFER_RESP` (§3.14).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TransferStatus {
    /// The transfer completed; any data is in [`TransferOutcome::data`].
    Ok,
    /// The device STALLed the request.
    Stall,
    /// The device NAKed to a timeout, or never answered.
    Nak,
    /// No device is attached on the host chip.
    NoDevice,
    /// The box refused before reaching the device: the opt-in is off, the request was malformed, or
    /// the data stage was larger than one control frame carries.
    Refused,
    /// A status byte this crate does not name (kept so a newer box's value is not lost).
    Other(u8),
}

impl TransferStatus {
    /// Map a wire `status` byte to a [`TransferStatus`].
    pub fn from_u8(v: u8) -> TransferStatus {
        match v {
            0x00 => TransferStatus::Ok,
            0xFD => TransferStatus::Stall,
            0xFE => TransferStatus::Nak,
            0xFF => TransferStatus::NoDevice,
            0xFC => TransferStatus::Refused,
            other => TransferStatus::Other(other),
        }
    }

    /// The wire `status` byte.
    pub fn as_u8(self) -> u8 {
        match self {
            TransferStatus::Ok => 0x00,
            TransferStatus::Stall => 0xFD,
            TransferStatus::Nak => 0xFE,
            TransferStatus::NoDevice => 0xFF,
            TransferStatus::Refused => 0xFC,
            TransferStatus::Other(v) => v,
        }
    }

    /// Whether the transfer completed.
    pub fn is_ok(self) -> bool {
        matches!(self, TransferStatus::Ok)
    }
}

/// The device's answer to a [`transfer`](crate::Device::transfer): its status and any IN data.
///
/// A [`TransferStatus`] other than [`Ok`](TransferStatus::Ok) is a real protocol outcome, not a link
/// error, so it is returned rather than raised; [`is_ok`](Self::is_ok) and [`data`](Self::data) read
/// it. The `Ok(_)` of the surrounding [`Result`](crate::Result) means the box answered at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransferOutcome {
    /// How the transfer ended.
    pub status: TransferStatus,
    /// The IN data the device returned (empty for an OUT transfer, a STALL, or no data).
    pub data: Vec<u8>,
}

impl TransferOutcome {
    /// Whether the transfer completed.
    pub fn is_ok(&self) -> bool {
        self.status.is_ok()
    }

    /// The returned data.
    pub fn data(&self) -> &[u8] {
        &self.data
    }

    /// The returned data if the transfer completed, else `None`.
    pub fn ok_data(self) -> Option<Vec<u8>> {
        self.status.is_ok().then_some(self.data)
    }
}
