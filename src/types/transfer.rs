use crate::protocol::opcode::{
    XFER_BUSY, XFER_ERROR, XFER_NO_ANSWER, XFER_NO_DEVICE, XFER_OK, XFER_REFUSED, XFER_STAGE_DATA,
    XFER_STAGE_SETUP, XFER_STAGE_STATUS, XFER_STALL, XFER_SUSPENDED,
};

/// USB control-transfer setup packet, eight bytes little-endian on the wire.
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
    /// `wLength`: data-stage length.
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

/// How a [`transfer`](crate::Device::transfer) ended: `TRANSFER_RESP`'s `status` byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TransferStatus {
    /// Completed.
    Ok,
    /// The game PC held the pipe until the timeout, or took it from the transfer in `stage`.
    Busy,
    /// The game PC has suspended the clone, and with it the real device: nothing was sent.
    Suspended,
    /// Native packets were corrupt or overran the buffer, or the host chip had no free channel.
    Error,
    /// Never sent: the opt-in is off, `ep` is undeclared, or the box had no room for it.
    Refused,
    /// The real device STALLed, in `stage`.
    Stall,
    /// No answer: the real device NAKed until the timeout, or the host chip's reply was lost.
    NoAnswer,
    /// No device attached.
    NoDevice,
    /// Status byte this crate does not name.
    Other(u8),
}

impl TransferStatus {
    /// Decodes a wire `status` byte.
    pub fn from_u8(v: u8) -> TransferStatus {
        match v {
            XFER_OK => TransferStatus::Ok,
            XFER_BUSY => TransferStatus::Busy,
            XFER_SUSPENDED => TransferStatus::Suspended,
            XFER_ERROR => TransferStatus::Error,
            XFER_REFUSED => TransferStatus::Refused,
            XFER_STALL => TransferStatus::Stall,
            XFER_NO_ANSWER => TransferStatus::NoAnswer,
            XFER_NO_DEVICE => TransferStatus::NoDevice,
            other => TransferStatus::Other(other),
        }
    }

    /// Wire `status` byte.
    pub fn as_u8(self) -> u8 {
        match self {
            TransferStatus::Ok => XFER_OK,
            TransferStatus::Busy => XFER_BUSY,
            TransferStatus::Suspended => XFER_SUSPENDED,
            TransferStatus::Error => XFER_ERROR,
            TransferStatus::Refused => XFER_REFUSED,
            TransferStatus::Stall => XFER_STALL,
            TransferStatus::NoAnswer => XFER_NO_ANSWER,
            TransferStatus::NoDevice => XFER_NO_DEVICE,
            TransferStatus::Other(v) => v,
        }
    }

    /// Whether the transfer completed.
    pub fn is_ok(self) -> bool {
        matches!(self, TransferStatus::Ok)
    }
}

/// Where a [`transfer`](crate::Device::transfer) ended: `TRANSFER_RESP`'s `stage` byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TransferStage {
    /// The setup stage.
    Setup,
    /// The data stage.
    Data,
    /// The status stage, where a completed transfer ends.
    Status,
    /// Stage byte this crate does not name.
    Other(u8),
}

impl TransferStage {
    /// Decodes a wire `stage` byte.
    pub fn from_u8(v: u8) -> TransferStage {
        match v {
            XFER_STAGE_SETUP => TransferStage::Setup,
            XFER_STAGE_DATA => TransferStage::Data,
            XFER_STAGE_STATUS => TransferStage::Status,
            other => TransferStage::Other(other),
        }
    }

    /// Wire `stage` byte.
    pub fn as_u8(self) -> u8 {
        match self {
            TransferStage::Setup => XFER_STAGE_SETUP,
            TransferStage::Data => XFER_STAGE_DATA,
            TransferStage::Status => XFER_STAGE_STATUS,
            TransferStage::Other(v) => v,
        }
    }
}

/// Outcome of a [`transfer`](crate::Device::transfer): how and where it ended, and the native reply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransferOutcome {
    /// How the transfer ended.
    pub status: TransferStatus,
    /// Where the transfer ended.
    pub stage: TransferStage,
    /// The native reply as the box received it, partial data before a STALL included.
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
