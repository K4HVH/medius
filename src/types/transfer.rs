//! `TRANSFER` (§3.14) vocabulary: the setup packet a host sends, and the device's reply.
//!
//! [`Device::transfer`](crate::Device::transfer) runs one control transfer against the real device on
//! the host chip and returns its reply, over its own inter-chip link pair, apart from the game PC's
//! EP0 proxy. One at a time: issue one and wait for the reply.

/// USB control-transfer setup packet: `bmRequestType`, `bRequest`, `wValue`, `wIndex`, `wLength`
/// (USB spec §9.3), eight bytes little-endian on the wire.
///
/// `length` is the data-stage length: for an IN request, the bytes to read back (the device may
/// return fewer); for an OUT request, the length of the data passed to
/// [`transfer`](crate::Device::transfer), which the box carries after the setup packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Setup {
    /// `bmRequestType`: direction (b7), type and recipient.
    pub request_type: u8,
    /// `bRequest`: request code.
    pub request: u8,
    /// `wValue`: request-specific.
    pub value: u16,
    /// `wIndex`: request-specific (often an interface or endpoint).
    pub index: u16,
    /// `wLength`: data-stage length (see the type docs).
    pub length: u16,
}

impl Setup {
    /// Setup packet from its five fields.
    pub fn new(request_type: u8, request: u8, value: u16, index: u16, length: u16) -> Setup {
        Setup {
            request_type,
            request,
            value,
            index,
            length,
        }
    }

    /// The eight wire bytes (`<BBHHH>`, little-endian).
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

    /// Whether this is a device-to-host (IN) request, per `bmRequestType`'s direction bit.
    pub fn is_in(self) -> bool {
        self.request_type & 0x80 != 0
    }
}

/// How a [`transfer`](crate::Device::transfer) ended: `TRANSFER_RESP`'s `status` byte (§3.14).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TransferStatus {
    /// Completed; any data is in [`TransferOutcome::data`].
    Ok,
    /// The device STALLed the request.
    Stall,
    /// No reply: the device NAKed past the timeout or failed on the bus, `ep` names a control
    /// endpoint the device does not declare, or the host chip did not reply in time.
    Nak,
    /// No device attached on the host chip.
    NoDevice,
    /// Refused by the box before reaching the device: the opt-in is off, the request is malformed or
    /// its data stage too large or short, or the host chip's control queue is full.
    Refused,
    /// Status byte this crate does not name, kept so a newer box's value is not lost.
    Other(u8),
}

impl TransferStatus {
    /// Decodes a wire `status` byte.
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

    /// Wire `status` byte.
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

/// Device's reply to a [`transfer`](crate::Device::transfer): status and any IN data.
///
/// A [`TransferStatus`] other than [`Ok`](TransferStatus::Ok) is a protocol outcome, not a link
/// error, so it is returned, not raised; [`is_ok`](Self::is_ok) and [`data`](Self::data) read it.
/// The surrounding [`Result`](crate::Result)'s `Ok(_)` means the box replied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransferOutcome {
    /// How the transfer ended.
    pub status: TransferStatus,
    /// IN data returned (empty for an OUT transfer, a STALL, or no data).
    pub data: Vec<u8>,
}

impl TransferOutcome {
    /// Whether the transfer completed.
    pub fn is_ok(&self) -> bool {
        self.status.is_ok()
    }

    /// Returned data.
    pub fn data(&self) -> &[u8] {
        &self.data
    }

    /// Returned data if the transfer completed, else `None`.
    pub fn ok_data(self) -> Option<Vec<u8>> {
        self.status.is_ok().then_some(self.data)
    }
}
