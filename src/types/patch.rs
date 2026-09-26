//! `PATCH` (§3.14) vocabulary: descriptor patches, their sections, and decoded `RESP(PATCHES)` /
//! `RESP(PATCH_ENTRY)`.
//!
//! A patch overwrites bytes the clone presents at enumeration, persisted per device (VID:PID) in the
//! box's NVS. It is configuration: it survives a reconnect and clears on
//! [`clear_patch`](crate::Device::clear_patch) or [`factory_reset`](crate::Device::factory_reset).
//! The box stores a patch whatever the opt-in, and applies the set only under
//! [`allow_imperfect_clones`](crate::Device::allow_imperfect_clones).

use crate::protocol::opcode::{
    PATCH_SEC_BOS, PATCH_SEC_CONFIG, PATCH_SEC_DEVICE, PATCH_SEC_REPORT, PATCH_SEC_STRING,
};

/// Descriptor a patch overwrites (§3.14).
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub enum PatchSection {
    /// 18-byte device descriptor; `cfg`/`index` ignored.
    #[default]
    Device = PATCH_SEC_DEVICE,
    /// Configuration descriptor; `cfg` is the configuration index, from 0.
    Config = PATCH_SEC_CONFIG,
    /// Interface report descriptor; `cfg` is the configuration index, `index` the interface number.
    Report = PATCH_SEC_REPORT,
    /// String descriptor; `index` is the string index (not 0). Replaces the whole string: at most 127
    /// bytes, one UTF-16 code unit each, `offset` ignored.
    String = PATCH_SEC_STRING,
    /// BOS descriptor; `cfg`/`index` ignored.
    Bos = PATCH_SEC_BOS,
}

impl PatchSection {
    /// Wire `section` byte.
    pub fn as_u8(self) -> u8 {
        self as u8
    }

    /// Decodes a wire `section` byte; `None` if unknown, including the `APPLY` and `CLEAR` verbs.
    pub fn from_u8(v: u8) -> Option<PatchSection> {
        Some(match v {
            PATCH_SEC_DEVICE => PatchSection::Device,
            PATCH_SEC_CONFIG => PatchSection::Config,
            PATCH_SEC_REPORT => PatchSection::Report,
            PATCH_SEC_STRING => PatchSection::String,
            PATCH_SEC_BOS => PatchSection::Bos,
            _ => return None,
        })
    }
}

/// Descriptor patch the host installs on the box.
///
/// Keyed by `(section, cfg, index, offset)`: setting an existing key overwrites it and moves it to the
/// end of the set (setting the bytes it already holds changes nothing); empty
/// [`bytes`](Patch::bytes) removes the patch at that key. Every section but
/// [`String`](PatchSection::String) keeps the descriptor's byte count; a set failing the box's checks
/// is served unpatched ([`PatchSet::refused`]).
///
/// ```no_run
/// # use medius::{Device, Result};
/// # use medius::{Patch, PatchSection};
/// # fn main() -> Result<()> {
/// let device = Device::find()?;
/// // Overwrite idVendor in the device descriptor (offset 8, little-endian), then re-present.
/// device.set_patch(&Patch::new(PatchSection::Device, 8, [0x34, 0x12]))?;
/// device.allow_imperfect_clones(true)?;
/// device.apply_patch()?;
/// # Ok(()) }
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Patch {
    /// Target section.
    pub section: PatchSection,
    /// Configuration index for [`Config`](PatchSection::Config)/[`Report`](PatchSection::Report):
    /// `0` is the first configuration, not `bConfigurationValue`.
    pub cfg: u8,
    /// Interface or string index, for
    /// [`Report`](PatchSection::Report)/[`String`](PatchSection::String).
    pub index: u8,
    /// Byte offset in the descriptor where the overwrite starts.
    pub offset: u16,
    /// Overwrite bytes; empty removes the patch at this key.
    pub bytes: Vec<u8>,
}

impl Patch {
    /// Patch on a [`Device`](PatchSection::Device) or [`Bos`](PatchSection::Bos) section, which
    /// ignore `cfg`/`index`. The other sections use [`in_config`](Self::in_config),
    /// [`in_interface`](Self::in_interface) and [`in_string`](Self::in_string).
    pub fn new(section: PatchSection, offset: u16, bytes: impl Into<Vec<u8>>) -> Patch {
        Patch {
            section,
            cfg: 0,
            index: 0,
            offset,
            bytes: bytes.into(),
        }
    }

    /// [`Config`](PatchSection::Config) patch in configuration index `cfg` (`0` is the first).
    pub fn in_config(cfg: u8, offset: u16, bytes: impl Into<Vec<u8>>) -> Patch {
        Patch {
            section: PatchSection::Config,
            cfg,
            index: 0,
            offset,
            bytes: bytes.into(),
        }
    }

    /// [`Report`](PatchSection::Report) patch on `interface` in configuration index `cfg` (`0` is
    /// the first).
    pub fn in_interface(cfg: u8, interface: u8, offset: u16, bytes: impl Into<Vec<u8>>) -> Patch {
        Patch {
            section: PatchSection::Report,
            cfg,
            index: interface,
            offset,
            bytes: bytes.into(),
        }
    }

    /// [`String`](PatchSection::String) patch replacing the whole of string `index`.
    pub fn in_string(index: u8, bytes: impl Into<Vec<u8>>) -> Patch {
        Patch {
            section: PatchSection::String,
            cfg: 0,
            index,
            offset: 0,
            bytes: bytes.into(),
        }
    }

    /// The patch's `(section, cfg, index, offset)` store key.
    pub fn key(&self) -> PatchKey {
        PatchKey {
            section: self.section,
            cfg: self.cfg,
            index: self.index,
            offset: self.offset,
        }
    }
}

/// `(section, cfg, index, offset)` key of one stored patch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PatchKey {
    /// Descriptor section.
    pub section: PatchSection,
    /// Configuration index.
    pub cfg: u8,
    /// Interface or string index.
    pub index: u8,
    /// Byte offset in the descriptor.
    pub offset: u16,
}

/// Row of the decoded [`PatchSet`] summary (§4.17): a stored patch's key and length, without its
/// bytes. [`query_patch_entry`](crate::Device::query_patch_entry) reads the full patch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PatchEntry {
    /// Descriptor section.
    pub section: PatchSection,
    /// Configuration index.
    pub cfg: u8,
    /// Interface or string index.
    pub index: u8,
    /// Byte offset in the descriptor.
    pub offset: u16,
    /// Bytes overwritten.
    pub len: u16,
}

/// Decoded `RESP(PATCHES)` (§4.17): the stored patch set and its apply state.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PatchSet {
    /// The clone serves a non-empty patched set: the one it was presented with, which a later store
    /// leaves alone until the next presentation.
    pub applied: bool,
    /// The stored set differs from the served one in patches, bytes or order: not applied yet,
    /// changed or emptied since, refused, or held back with the opt-in off.
    pub pending: bool,
    /// The stored set failed a check at the last presentation and is unchanged since, so the device
    /// is served unpatched: a clone check, or a consistency check the unpatched descriptors pass (a
    /// descriptor's length or type fields, `bcdUSB` 0x0201+ with no BOS, a HID `wDescriptorLength`,
    /// an interrupt-IN `wMaxPacketSize` below the report). The box logs which. A set change, a clear,
    /// a passing presentation or a detach resets it.
    pub refused: bool,
    /// The box refused the last new patch or overwrite for room: 16 entries in use, or the 1024-byte
    /// pool full. The next set change, or a clear, resets it.
    pub table_full: bool,
    /// One row per stored patch.
    pub entries: Vec<PatchEntry>,
}

impl PatchSet {
    /// `[what][flags u8][n u8]` then `n` × `[section u8][cfg u8][index u8][offset u16][len u16]`.
    pub(crate) fn from_payload(p: &[u8]) -> Option<PatchSet> {
        if p.len() < 3 {
            return None;
        }
        let flags = p[1];
        let n = p[2] as usize;
        let mut entries = Vec::with_capacity(n);
        for i in 0..n {
            let o = 3 + 7 * i;
            let row = p.get(o..o + 7)?;
            // A section a newer box added is skipped; the rest reads.
            let Some(section) = PatchSection::from_u8(row[0]) else {
                continue;
            };
            entries.push(PatchEntry {
                section,
                cfg: row[1],
                index: row[2],
                offset: u16::from_le_bytes([row[3], row[4]]),
                len: u16::from_le_bytes([row[5], row[6]]),
            });
        }
        Some(PatchSet {
            applied: flags & 0x01 != 0,
            pending: flags & 0x02 != 0,
            refused: flags & 0x04 != 0,
            table_full: flags & 0x08 != 0,
            entries,
        })
    }
}

// `[what][list_index][section][cfg][index][offset u16][bytes]`, decoded into the patch that
// replays it.
pub(crate) fn patch_entry_from_payload(p: &[u8]) -> Option<Patch> {
    let hdr = p.get(0..7)?;
    let section = PatchSection::from_u8(hdr[2])?;
    Some(Patch {
        section,
        cfg: hdr[3],
        index: hdr[4],
        offset: u16::from_le_bytes([hdr[5], hdr[6]]),
        bytes: p.get(7..)?.to_vec(),
    })
}
