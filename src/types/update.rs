//! Firmware update state: which chip, which slot, and how a staged image is answered (§3.13, §4.16).

use core::fmt;

use crate::protocol::opcode::Q_FIRMWARE;

/// Which chip an update addresses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UpdateTarget {
    /// The PC-facing chip: the clone, the control protocol, injection.
    Device,
    /// The device-facing chip, reachable only through the inter-chip link.
    Host,
}

impl UpdateTarget {
    pub fn as_u8(self) -> u8 {
        match self {
            UpdateTarget::Device => 0,
            UpdateTarget::Host => 1,
        }
    }

    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(UpdateTarget::Device),
            1 => Some(UpdateTarget::Host),
            _ => None,
        }
    }
}

impl fmt::Display for UpdateTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            UpdateTarget::Device => "device",
            UpdateTarget::Host => "host",
        })
    }
}

/// What the bootloader thinks of the image a chip is running.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ImageState {
    New,
    /// Booted but not yet confirmed. This is the window a failed image is reverted in.
    PendingVerify,
    Valid,
    Invalid,
    Aborted,
    Unknown(u8),
}

impl ImageState {
    pub fn from_u8(v: u8) -> Self {
        match v {
            0 => ImageState::New,
            1 => ImageState::PendingVerify,
            2 => ImageState::Valid,
            3 => ImageState::Invalid,
            4 => ImageState::Aborted,
            other => ImageState::Unknown(other),
        }
    }

    /// True while the chip has not confirmed the image it booted. A chip in this state refuses to
    /// open another update, because the one it is running might still be reverted.
    pub fn is_pending(self) -> bool {
        matches!(self, ImageState::PendingVerify)
    }
}

impl fmt::Display for ImageState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ImageState::New => f.write_str("new"),
            ImageState::PendingVerify => f.write_str("pending-verify"),
            ImageState::Valid => f.write_str("valid"),
            ImageState::Invalid => f.write_str("invalid"),
            ImageState::Aborted => f.write_str("aborted"),
            ImageState::Unknown(v) => write!(f, "unknown({v})"),
        }
    }
}

/// One chip's firmware version and which of its two app slots it booted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ChipFirmware {
    pub major: u8,
    pub minor: u8,
    pub patch: u8,
    /// 0 = `ota_0`, 1 = `ota_1`.
    pub slot: u8,
    pub state: ImageState,
}

impl fmt::Display for ChipFirmware {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}.{}.{} on ota_{} ({})",
            self.major, self.minor, self.patch, self.slot, self.state
        )
    }
}

/// The decoded `RESP(FIRMWARE)` payload (§4.16): the only place the host chip's version appears.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FirmwareInfo {
    pub device: ChipFirmware,
    /// `None` when the host chip has not answered over the inter-chip link.
    pub host: Option<ChipFirmware>,
    /// Usable bytes in a spare slot; the same on both chips.
    pub slot_size: u32,
    pub device_staged: bool,
    pub host_staged: bool,
}

impl FirmwareInfo {
    pub const PAYLOAD_LEN: usize = 17;

    /// Decode `RESP(FIRMWARE)`; `None` for anything shorter than the fixed 17 bytes.
    pub fn from_payload(p: &[u8]) -> Option<Self> {
        if p.len() < Self::PAYLOAD_LEN || p[0] != Q_FIRMWARE {
            return None;
        }
        let host_present = p[6] != 0;
        Some(FirmwareInfo {
            device: ChipFirmware {
                major: p[1],
                minor: p[2],
                patch: p[3],
                slot: p[4],
                state: ImageState::from_u8(p[5]),
            },
            host: host_present.then(|| ChipFirmware {
                major: p[7],
                minor: p[8],
                patch: p[9],
                slot: p[10],
                state: ImageState::from_u8(p[11]),
            }),
            slot_size: u32::from_le_bytes([p[12], p[13], p[14], p[15]]),
            device_staged: p[16] & 0x01 != 0,
            host_staged: p[16] & 0x02 != 0,
        })
    }

    /// True while either chip is still on probation, which is when an update is refused.
    pub fn any_pending(&self) -> bool {
        self.device.state.is_pending() || self.host.is_some_and(|h| h.state.is_pending())
    }
}

/// What the box answered one `UPDATE` op with (§4.16).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct UpdateStatus(pub u8);

impl UpdateStatus {
    pub const OK: UpdateStatus = UpdateStatus(0x00);
    pub const READY: UpdateStatus = UpdateStatus(0x01);
    pub const ACK: UpdateStatus = UpdateStatus(0x02);
    pub const STAGED: UpdateStatus = UpdateStatus(0x03);
    pub const BUSY: UpdateStatus = UpdateStatus(0x10);
    pub const NO_SLOT: UpdateStatus = UpdateStatus(0x11);
    pub const TOO_BIG: UpdateStatus = UpdateStatus(0x12);
    pub const SEQ_GAP: UpdateStatus = UpdateStatus(0x13);
    pub const WRITE_FAILED: UpdateStatus = UpdateStatus(0x14);
    pub const BAD_SHA: UpdateStatus = UpdateStatus(0x15);
    pub const BAD_IMAGE: UpdateStatus = UpdateStatus(0x16);
    pub const LINK_DOWN: UpdateStatus = UpdateStatus(0x17);
    pub const TIMEOUT: UpdateStatus = UpdateStatus(0x18);
    pub const NOTHING_STAGED: UpdateStatus = UpdateStatus(0x19);
    pub const BAD_STATE: UpdateStatus = UpdateStatus(0x1A);
    pub const ON_PROBATION: UpdateStatus = UpdateStatus(0x1B);
    /// Refused before the slot was touched. Unlike [`UpdateStatus::WRITE_FAILED`], which comes after
    /// the erase, whatever was already staged survives this and is still bootable.
    pub const UNTOUCHED: UpdateStatus = UpdateStatus(0x1C);

    pub fn name(self) -> &'static str {
        match self {
            UpdateStatus::OK => "ok",
            UpdateStatus::READY => "ready",
            UpdateStatus::ACK => "ack",
            UpdateStatus::STAGED => "staged",
            UpdateStatus::BUSY => "busy",
            UpdateStatus::NO_SLOT => "no-slot",
            UpdateStatus::TOO_BIG => "too-big",
            UpdateStatus::SEQ_GAP => "seq-gap",
            UpdateStatus::WRITE_FAILED => "write-failed",
            UpdateStatus::BAD_SHA => "bad-sha",
            UpdateStatus::BAD_IMAGE => "bad-image",
            UpdateStatus::LINK_DOWN => "link-down",
            UpdateStatus::TIMEOUT => "timeout",
            UpdateStatus::NOTHING_STAGED => "nothing-staged",
            UpdateStatus::BAD_STATE => "bad-state",
            UpdateStatus::ON_PROBATION => "on-probation",
            UpdateStatus::UNTOUCHED => "untouched",
            _ => "unknown",
        }
    }
}

impl fmt::Display for UpdateStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} (0x{:02X})", self.name(), self.0)
    }
}

/// How far a staged transfer has got, for a progress callback.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct UpdateProgress {
    pub target: UpdateTarget,
    pub sent: usize,
    pub total: usize,
}

impl UpdateProgress {
    pub fn percent(&self) -> u8 {
        if self.total == 0 {
            return 100;
        }
        ((self.sent as u64 * 100) / self.total as u64) as u8
    }
}

use crate::protocol::opcode::{
    OTA_OP_ABORT, OTA_OP_ACTIVATE, OTA_OP_BEGIN, OTA_OP_DATA, OTA_OP_END,
};

/// What the box was being asked to do, for an error message. The op number alone means nothing to
/// whoever is holding the box.
pub(crate) fn update_doing(op: u8) -> &'static str {
    match op {
        OTA_OP_BEGIN => "Starting the firmware transfer",
        OTA_OP_DATA => "Sending the firmware",
        OTA_OP_END => "Finishing the firmware transfer",
        OTA_OP_ABORT => "Cancelling the firmware transfer",
        OTA_OP_ACTIVATE => "Activating the new firmware",
        _ => "The firmware update",
    }
}

/// What a refusal means and what to do about it. A status name and a bare number read as a code dump;
/// the point of an error is that the reader knows what to do next.
pub(crate) fn update_reason(op: u8, status: UpdateStatus, arg: u32) -> String {
    match status {
        UpdateStatus::BUSY => "the box already has an update open on that chip.".into(),
        UpdateStatus::NO_SLOT => "this box still has the single-slot firmware layout, so it cannot be \
             updated over this port. It needs one flash over ROM download first."
            .into(),
        UpdateStatus::TOO_BIG => format!("the image does not fit. A slot holds {arg} bytes."),
        UpdateStatus::SEQ_GAP => {
            format!("a chunk went missing, so the box dropped the transfer. It was expecting chunk {arg}.")
        }
        UpdateStatus::WRITE_FAILED => format!("the box could not write to its flash (error {arg})."),
        UpdateStatus::BAD_SHA => "the image arrived corrupted: its digest did not match what was \
             declared. Try again."
            .into(),
        UpdateStatus::BAD_IMAGE => format!("those bytes are not a bootable image (error {arg})."),
        UpdateStatus::LINK_DOWN => {
            "the box cannot reach its mouse-side chip over the inter-chip link.".into()
        }
        UpdateStatus::TIMEOUT if op == OTA_OP_ACTIVATE => "the mouse-side chip did not come back \
             after committing its firmware. Power cycle the box, then check which slot each chip is \
             running before trying again."
            .into(),
        UpdateStatus::TIMEOUT => "the box stopped answering partway through and dropped the transfer \
             after ten seconds of silence."
            .into(),
        UpdateStatus::NOTHING_STAGED => "there is nothing staged to activate.".into(),
        UpdateStatus::BAD_STATE => {
            format!("the box was not expecting that step; it wanted op {arg}.")
        }
        UpdateStatus::ON_PROBATION => "a chip is still proving the firmware it just booted. Wait a \
             few seconds and try again."
            .into(),
        UpdateStatus::UNTOUCHED => "the box refused before writing anything, so whatever was already \
             staged is untouched."
            .into(),
        other => format!("the box answered {other} (arg {arg})."),
    }
}
