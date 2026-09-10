//! `REWRITE` (§3.14) vocabulary: the rule a host installs, its action and class, and the decoded
//! `RESP(REWRITE)` / `RESP(REWRITE_ENTRY)` readbacks.
//!
//! The on-box rewrite table matches traffic in flight and rewrites, answers, refuses or drops it. It
//! is gated on [`allow_imperfect_clones`](crate::Device::allow_imperfect_clones) and addressed in the
//! same `(class, id, direction)` space `CATCH` uses, in the write direction. A rule is session state,
//! re-asserted on reconnect exactly like a lock or a catch subscription.

use crate::protocol::opcode::{
    CATCH_CLS_CONTROL, CATCH_CLS_EMIT, CATCH_CLS_HID_IN, CATCH_CLS_HID_OUT, CATCH_CLS_VEND_BULK,
    CATCH_CLS_VEND_INTR, CATCH_ID_ANY, RW_ANSWER, RW_DROP, RW_NAK, RW_PASS, RW_PATCH, RW_REPLACE,
    RW_REPLY_PATCH, RW_REPLY_REPLACE, RW_STALL,
};
use crate::types::Direction;

/// A traffic class a rewrite rule may address (§3.14).
///
/// These are `CATCH`'s traffic classes in the write direction, the exact set the box will rewrite:
/// the parsed-input classes (button, key, media, axis) and the bus class are not rewritable and have
/// no variant here. [`Any`](RewriteClass::Any) matches every rewritable class at once.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum RewriteClass {
    /// The device's HID input report, before the renderer; `id` is the interface number.
    HidIn = CATCH_CLS_HID_IN,
    /// An interrupt-OUT report the game PC wrote, relayed to the device; `id` is the endpoint address.
    HidOut = CATCH_CLS_HID_OUT,
    /// Interrupt traffic on a vendor interface; `id` is the endpoint address.
    VendorInterrupt = CATCH_CLS_VEND_INTR,
    /// Bulk traffic on a vendor interface; `id` is the endpoint address.
    VendorBulk = CATCH_CLS_VEND_BULK,
    /// A proxied control transfer; `id` is the endpoint number (0 = EP0). The one class that may
    /// `Answer`/`Stall`/`Nak` or rewrite the device's reply.
    Control = CATCH_CLS_CONTROL,
    /// The outgoing wire, after the renderer; `id` is the endpoint address. Catches injected and
    /// rendered frames as well as relayed ones.
    Emit = CATCH_CLS_EMIT,
    /// Every rewritable class at once (the wire wildcard `0xFF`).
    Any = 0xFF,
}

impl RewriteClass {
    /// The wire `class` byte.
    pub fn as_u8(self) -> u8 {
        self as u8
    }

    /// Map a wire `class` byte to a [`RewriteClass`], or `None` for one that is not rewritable.
    pub fn from_u8(v: u8) -> Option<RewriteClass> {
        Some(match v {
            CATCH_CLS_HID_IN => RewriteClass::HidIn,
            CATCH_CLS_HID_OUT => RewriteClass::HidOut,
            CATCH_CLS_VEND_INTR => RewriteClass::VendorInterrupt,
            CATCH_CLS_VEND_BULK => RewriteClass::VendorBulk,
            CATCH_CLS_CONTROL => RewriteClass::Control,
            CATCH_CLS_EMIT => RewriteClass::Emit,
            0xFF => RewriteClass::Any,
            _ => return None,
        })
    }

    /// Whether this is the control class, the only one that may answer or rewrite a device reply.
    pub fn is_control(self) -> bool {
        matches!(self, RewriteClass::Control)
    }
}

/// What the winning rewrite rule does to a matched packet (§3.14).
///
/// A report class ([`HidIn`](RewriteClass::HidIn), [`HidOut`](RewriteClass::HidOut),
/// [`Emit`](RewriteClass::Emit), the vendor classes) may [`Pass`](RewriteAction::Pass),
/// [`Drop`](RewriteAction::Drop), [`Patch`](RewriteAction::Patch) or [`Replace`](RewriteAction::Replace).
/// The control class adds [`Answer`](RewriteAction::Answer), [`Stall`](RewriteAction::Stall),
/// [`Nak`](RewriteAction::Nak) and the two reply rewrites. [`is_valid_for`](RewriteAction::is_valid_for)
/// mirrors the box's own admissibility check.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub enum RewriteAction {
    /// Matched a rule but leaves the packet untouched (a shadow over a broader rule).
    #[default]
    Pass = RW_PASS,
    /// Report class: the packet is not delivered. An `Emit` drop mutes the wire; a `HidIn` drop drops
    /// the device's contribution while injection still emits.
    Drop = RW_DROP,
    /// Overwrite the payload bytes at `offset`, length preserved.
    Patch = RW_PATCH,
    /// The packet becomes the payload.
    Replace = RW_REPLACE,
    /// Control: answer from the payload without asking the device.
    Answer = RW_ANSWER,
    /// Control: protocol STALL.
    Stall = RW_STALL,
    /// Control: NAK to a timeout.
    Nak = RW_NAK,
    /// Control IN: overwrite the device's reply at `offset`.
    ReplyPatch = RW_REPLY_PATCH,
    /// Control IN: replace the device's reply with the payload.
    ReplyReplace = RW_REPLY_REPLACE,
}

impl RewriteAction {
    /// The wire `action` byte.
    pub fn as_u8(self) -> u8 {
        self as u8
    }

    /// Map a wire `action` byte to a [`RewriteAction`], or `None` for an unknown value.
    pub fn from_u8(v: u8) -> Option<RewriteAction> {
        Some(match v {
            RW_PASS => RewriteAction::Pass,
            RW_DROP => RewriteAction::Drop,
            RW_PATCH => RewriteAction::Patch,
            RW_REPLACE => RewriteAction::Replace,
            RW_ANSWER => RewriteAction::Answer,
            RW_STALL => RewriteAction::Stall,
            RW_NAK => RewriteAction::Nak,
            RW_REPLY_PATCH => RewriteAction::ReplyPatch,
            RW_REPLY_REPLACE => RewriteAction::ReplyReplace,
            _ => return None,
        })
    }

    /// Whether this action carries a payload the rule must supply.
    pub fn carries_payload(self) -> bool {
        matches!(
            self,
            RewriteAction::Patch
                | RewriteAction::Replace
                | RewriteAction::Answer
                | RewriteAction::ReplyPatch
                | RewriteAction::ReplyReplace
        )
    }

    /// Whether this action is admissible on `class`, mirroring the box's `rewrite_action_ok`: `Drop`
    /// is a report surface only, and `Answer`/`Stall`/`Nak`/the reply rewrites are control-only.
    pub fn is_valid_for(self, class: RewriteClass) -> bool {
        let ctl = class.is_control();
        let any = matches!(class, RewriteClass::Any);
        match self {
            RewriteAction::Pass | RewriteAction::Patch | RewriteAction::Replace => true,
            RewriteAction::Drop => !ctl && !any,
            RewriteAction::Answer
            | RewriteAction::Stall
            | RewriteAction::Nak
            | RewriteAction::ReplyPatch
            | RewriteAction::ReplyReplace => ctl,
        }
    }
}

/// A rewrite rule the host installs on the box.
///
/// A rule is keyed by `(class, id, direction, match, mask)`: two rules that differ in any of those are
/// separate table entries; setting one whose key already exists overwrites it. `match` and `mask` must
/// be the same length (the box compares the packet head byte-for-byte under `mask`); an empty match
/// matches every packet on the address. `offset` is where [`Patch`](RewriteAction::Patch) and
/// [`ReplyPatch`](RewriteAction::ReplyPatch) write; other actions ignore it. `payload` is the bytes an
/// action that [carries one](RewriteAction::carries_payload) supplies.
///
/// ```no_run
/// # use medius::{Device, Direction, Result};
/// # use medius::{RewriteRule, RewriteClass, RewriteAction};
/// # fn main() -> Result<()> {
/// let device = Device::find()?;
/// device.allow_imperfect_clones(true)?;
/// // Mute the clone's own wire on the interrupt-IN endpoint.
/// device.set_rewrite(&RewriteRule::new(RewriteClass::Emit, 0x81, Direction::Both, RewriteAction::Drop))?;
/// # Ok(()) }
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RewriteRule {
    /// The traffic class the rule addresses.
    pub class: RewriteClass,
    /// The address within the class: an interface number, endpoint address, or endpoint number.
    pub id: u16,
    /// The flow the rule matches. `REWRITE` uses `Both`/`Positive`/`Negative` only; the bearing-relative
    /// directions are rejected by the box.
    pub direction: Direction,
    /// What the rule does to a matched packet.
    pub action: RewriteAction,
    /// Where [`Patch`](RewriteAction::Patch)/[`ReplyPatch`](RewriteAction::ReplyPatch) write.
    pub offset: u16,
    /// The head bytes compared under [`mask`](RewriteRule::mask); empty matches every packet.
    pub match_bytes: Vec<u8>,
    /// The mask over [`match_bytes`](RewriteRule::match_bytes); same length.
    pub mask: Vec<u8>,
    /// The bytes an action that carries a payload supplies.
    pub payload: Vec<u8>,
}

impl Default for RewriteClass {
    /// [`Emit`](RewriteClass::Emit): the outgoing wire, the class a rule most often addresses.
    fn default() -> Self {
        RewriteClass::Emit
    }
}

impl RewriteRule {
    /// A rule with no match, no payload and `offset` 0. Add a masked match with
    /// [`matching`](Self::matching) and a payload with [`with_payload`](Self::with_payload).
    pub fn new(
        class: RewriteClass,
        id: u16,
        direction: Direction,
        action: RewriteAction,
    ) -> RewriteRule {
        RewriteRule {
            class,
            id,
            direction,
            action,
            ..RewriteRule::default()
        }
    }

    /// Narrow the rule to packets whose head compares equal to `match_bytes` under `mask`. Both slices
    /// must be the same length; a longer packet still matches on its head.
    pub fn matching(mut self, match_bytes: impl Into<Vec<u8>>, mask: impl Into<Vec<u8>>) -> Self {
        self.match_bytes = match_bytes.into();
        self.mask = mask.into();
        self
    }

    /// Set the write offset for [`Patch`](RewriteAction::Patch)/[`ReplyPatch`](RewriteAction::ReplyPatch).
    pub fn at_offset(mut self, offset: u16) -> Self {
        self.offset = offset;
        self
    }

    /// Supply the payload for an action that [carries one](RewriteAction::carries_payload).
    pub fn with_payload(mut self, payload: impl Into<Vec<u8>>) -> Self {
        self.payload = payload.into();
        self
    }

    /// The `(class, id, direction, match, mask)` key that identifies this rule in the table.
    pub fn key(&self) -> RewriteKey {
        RewriteKey {
            class: self.class,
            id: self.id,
            direction: self.direction,
            match_bytes: self.match_bytes.clone(),
            mask: self.mask.clone(),
        }
    }
}

/// The `(class, id, direction, match, mask)` key that identifies one table entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RewriteKey {
    /// The traffic class the rule addresses.
    pub class: RewriteClass,
    /// The address within the class.
    pub id: u16,
    /// The flow the rule matches.
    pub direction: Direction,
    /// The head bytes the rule matched on.
    pub match_bytes: Vec<u8>,
    /// The mask over the match.
    pub mask: Vec<u8>,
}

/// One row of the decoded [`RewriteTable`] summary (§4.17): the rule's address, action and live
/// counters, without the match/mask/payload bytes. Read the full rule with
/// [`query_rewrite_entry`](crate::Device::query_rewrite_entry).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RewriteEntry {
    /// The traffic class the rule addresses.
    pub class: RewriteClass,
    /// The address within the class.
    pub id: u16,
    /// The flow the rule matches.
    pub direction: Direction,
    /// What the rule does to a matched packet.
    pub action: RewriteAction,
    /// How many `match`/`mask` bytes the rule compares.
    pub match_len: u8,
    /// The write offset for a patching action.
    pub offset: u16,
    /// How many payload bytes the rule carries.
    pub payload_len: u16,
    /// How many packets the rule has matched since it was installed (saturating).
    pub hits: u16,
}

/// The decoded `RESP(REWRITE)` (§4.17): the whole rewrite table's summary.
///
/// `generation` bumps only on a change that alters the table, so a host that holds a last-seen value
/// re-sends its rules only when the box's diverges (a device blip or re-clone can clear the table
/// while the control link stays up). The crate does this for you; the field is exposed for a host that
/// runs its own reconcile.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RewriteTable {
    /// The table is full: a further rule was, or would be, refused.
    pub table_full: bool,
    /// The table's generation counter (see the type docs).
    pub generation: u8,
    /// One row per installed rule, in installation order (the order the box holds them). The
    /// most-specific-first ordering is how the box *selects* a match, not how it lists the table here.
    pub entries: Vec<RewriteEntry>,
}

impl RewriteTable {
    /// Decode a `RESP(REWRITE)` payload (§4.17): `[what][flags u8][gen u8][n u8]` then `n` ×
    /// `[cls u8][id u16][dir u8][action u8][mlen u8][off u16][plen u16][hits u16]`.
    pub(crate) fn from_payload(p: &[u8]) -> Option<RewriteTable> {
        if p.len() < 4 {
            return None;
        }
        let table_full = p[1] & 0x01 != 0;
        let generation = p[2];
        let n = p[3] as usize;
        let mut entries = Vec::with_capacity(n);
        for i in 0..n {
            let o = 4 + 12 * i;
            let row = p.get(o..o + 12)?;
            // A byte the crate does not have an enum for (a class or action a newer box added) is
            // skipped rather than aborting the whole decode: the rest of the table still reads.
            let (Some(class), Some(action)) = (
                RewriteClass::from_u8(row[0]),
                RewriteAction::from_u8(row[4]),
            ) else {
                continue;
            };
            let Some(direction) = Direction::from_u8(row[3]) else {
                continue;
            };
            entries.push(RewriteEntry {
                class,
                id: u16::from_le_bytes([row[1], row[2]]),
                direction,
                action,
                match_len: row[5],
                offset: u16::from_le_bytes([row[6], row[7]]),
                payload_len: u16::from_le_bytes([row[8], row[9]]),
                hits: u16::from_le_bytes([row[10], row[11]]),
            });
        }
        Some(RewriteTable {
            table_full,
            generation,
            entries,
        })
    }
}

/// Decode a `RESP(REWRITE_ENTRY)` payload (§4.17) back into the [`RewriteRule`] that replays it:
/// `[what][index][cls][id u16][dir][state=1][action][off u16][mlen][match mlen][mask mlen][payload]`.
pub(crate) fn rewrite_entry_from_payload(p: &[u8]) -> Option<RewriteRule> {
    // what + index + cls + id(2) + dir + state + action + off(2) + mlen = 11 bytes of header.
    let hdr = p.get(0..11)?;
    let class = RewriteClass::from_u8(hdr[2])?;
    let id = u16::from_le_bytes([hdr[3], hdr[4]]);
    let direction = Direction::from_u8(hdr[5])?;
    let action = RewriteAction::from_u8(hdr[7])?;
    let offset = u16::from_le_bytes([hdr[8], hdr[9]]);
    let mlen = hdr[10] as usize;
    let match_bytes = p.get(11..11 + mlen)?.to_vec();
    let mask = p.get(11 + mlen..11 + 2 * mlen)?.to_vec();
    let payload = p.get(11 + 2 * mlen..)?.to_vec();
    Some(RewriteRule {
        class,
        id,
        direction,
        action,
        offset,
        match_bytes,
        mask,
        payload,
    })
}

/// The wire sentinels the whole-table clear uses (`class 0xFF, id 0xFFFF, state 0`), so a caller
/// reading the encoders can see the clear is not a real rule.
pub(crate) const REWRITE_CLEAR_ID: u16 = CATCH_ID_ANY;
