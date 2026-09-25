//! `PATCH` (§3.14) vocabulary: a descriptor patch a host installs, the descriptor section it targets,
//! and the decoded `RESP(PATCHES)` / `RESP(PATCH_ENTRY)` readbacks.
//!
//! A patch overwrites bytes in what the clone presents at enumeration, persisted per device (VID:PID)
//! in the box's NVS. Unlike a rewrite rule, a patch is configuration, not session state: it survives a
//! reconnect and clears on [`clear_patch`](crate::Device::clear_patch) or
//! [`factory_reset`](crate::Device::factory_reset).
//! The box stores a patch whatever the opt-in, and applies the set only under
//! [`allow_imperfect_clones`](crate::Device::allow_imperfect_clones).

use crate::protocol::opcode::{
    PATCH_SEC_BOS, PATCH_SEC_CONFIG, PATCH_SEC_DEVICE, PATCH_SEC_REPORT, PATCH_SEC_STRING,
};

/// Which descriptor a patch overwrites (§3.14).
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub enum PatchSection {
    /// The 18-byte device descriptor. `cfg`/`index` are ignored.
    #[default]
    Device = PATCH_SEC_DEVICE,
    /// A configuration descriptor; `cfg` is the configuration index, counting from 0.
    Config = PATCH_SEC_CONFIG,
    /// An interface's report descriptor; `cfg` is the configuration index and `index` the interface number.
    Report = PATCH_SEC_REPORT,
    /// A string descriptor; `index` is the string index (not 0). The patch replaces the whole string: at
    /// most 127 bytes, one UTF-16 code unit each, `offset` ignored.
    String = PATCH_SEC_STRING,
    /// The BOS descriptor. `cfg`/`index` are ignored.
    Bos = PATCH_SEC_BOS,
}

impl PatchSection {
    /// The wire `section` byte.
    pub fn as_u8(self) -> u8 {
        self as u8
    }

    /// Map a wire `section` byte to a [`PatchSection`], or `None` for an unknown value (the `APPLY`
    /// and `CLEAR` engine verbs are not sections and decode to `None`).
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

/// A descriptor patch the host installs on the box.
///
/// A patch is keyed by `(section, cfg, index, offset)`: setting one whose key exists overwrites it and
/// moves it to the end of the set (the bytes it already holds change nothing), and a patch with empty
/// [`bytes`](Patch::bytes) removes the patch at that key. Every section but
/// [`String`](PatchSection::String) keeps the descriptor's byte count; a set that fails the box's
/// checks is served unpatched ([`PatchSet::refused`]).
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
    /// The descriptor section this patch targets.
    pub section: PatchSection,
    /// The configuration index for [`Config`](PatchSection::Config)/[`Report`](PatchSection::Report): `0` is
    /// the first configuration, not `bConfigurationValue`.
    pub cfg: u8,
    /// The interface or string index, for [`Report`](PatchSection::Report)/[`String`](PatchSection::String).
    pub index: u8,
    /// The byte offset within the descriptor the overwrite starts at.
    pub offset: u16,
    /// The overwrite bytes; empty removes the patch at this key.
    pub bytes: Vec<u8>,
}

impl Patch {
    /// A patch over a whole-descriptor section ([`Device`](PatchSection::Device)/[`Bos`](PatchSection::Bos)),
    /// where `cfg`/`index` are ignored. Use [`in_config`](Self::in_config) / [`in_interface`](Self::in_interface)
    /// / [`in_string`](Self::in_string) for the sections that take them.
    pub fn new(section: PatchSection, offset: u16, bytes: impl Into<Vec<u8>>) -> Patch {
        Patch {
            section,
            cfg: 0,
            index: 0,
            offset,
            bytes: bytes.into(),
        }
    }

    /// A [`Config`](PatchSection::Config) patch in configuration index `cfg` (`0` is the first).
    pub fn in_config(cfg: u8, offset: u16, bytes: impl Into<Vec<u8>>) -> Patch {
        Patch {
            section: PatchSection::Config,
            cfg,
            index: 0,
            offset,
            bytes: bytes.into(),
        }
    }

    /// A [`Report`](PatchSection::Report) patch on `interface` in configuration index `cfg` (`0` is the first).
    pub fn in_interface(cfg: u8, interface: u8, offset: u16, bytes: impl Into<Vec<u8>>) -> Patch {
        Patch {
            section: PatchSection::Report,
            cfg,
            index: interface,
            offset,
            bytes: bytes.into(),
        }
    }

    /// A [`String`](PatchSection::String) patch on string `index` (the whole string is replaced).
    pub fn in_string(index: u8, bytes: impl Into<Vec<u8>>) -> Patch {
        Patch {
            section: PatchSection::String,
            cfg: 0,
            index,
            offset: 0,
            bytes: bytes.into(),
        }
    }

    /// The `(section, cfg, index, offset)` key that identifies this patch in the store.
    pub fn key(&self) -> PatchKey {
        PatchKey {
            section: self.section,
            cfg: self.cfg,
            index: self.index,
            offset: self.offset,
        }
    }
}

/// The `(section, cfg, index, offset)` key that identifies one stored patch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PatchKey {
    /// The descriptor section.
    pub section: PatchSection,
    /// The configuration index.
    pub cfg: u8,
    /// The interface or string index.
    pub index: u8,
    /// The byte offset within the descriptor.
    pub offset: u16,
}

/// One row of the decoded [`PatchSet`] summary (§4.17): a stored patch's key and length, without its
/// bytes. Read the full patch with [`query_patch_entry`](crate::Device::query_patch_entry).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PatchEntry {
    /// The descriptor section.
    pub section: PatchSection,
    /// The configuration index.
    pub cfg: u8,
    /// The interface or string index.
    pub index: u8,
    /// The byte offset within the descriptor.
    pub offset: u16,
    /// How many bytes the patch overwrites.
    pub len: u16,
}

/// The decoded `RESP(PATCHES)` (§4.17): the stored patch set plus its apply state.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PatchSet {
    /// The clone serves a non-empty patched set: the one it was presented with, which a later store
    /// leaves alone until the clone is next presented.
    pub applied: bool,
    /// The stored set differs from the one the clone serves, in its patches, bytes or order: not applied
    /// yet, changed or emptied since, refused, or held back because the opt-in is off.
    pub pending: bool,
    /// The stored set failed a check when the clone was last presented with it and is unchanged since,
    /// so the device is served unpatched: a clone check, or a consistency check (a descriptor's length
    /// or type fields, `bcdUSB` 0x0201+ with no BOS, a HID `wDescriptorLength`, an interrupt-IN
    /// `wMaxPacketSize` below the report) that the unpatched descriptors pass. The box logs which. A set
    /// change, a clear, a presentation that passes or a detach resets it.
    pub refused: bool,
    /// Set when the box refused the last new patch or overwrite for room: 16 entries in use, or no
    /// space left in the 1024-byte pool. The next change to the set, or a clear, resets it.
    pub table_full: bool,
    /// One row per stored patch.
    pub entries: Vec<PatchEntry>,
}

impl PatchSet {
    /// Decode a `RESP(PATCHES)` payload (§4.17): `[what][flags u8][n u8]` then `n` ×
    /// `[section u8][cfg u8][index u8][offset u16][len u16]`.
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
            // A section a newer box added and this crate has no variant for is skipped; the rest reads.
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

/// Decode a `RESP(PATCH_ENTRY)` payload (§4.17) back into the [`Patch`] that replays it:
/// `[what][list_index][section][cfg][index][offset u16][bytes]`.
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
