//! `REWRITE` (§3.14) vocabulary: rules, actions, classes, and decoded `RESP(REWRITE)` /
//! `RESP(REWRITE_ENTRY)`.
//!
//! The box's rewrite table matches traffic in flight and rewrites, answers, refuses or drops it. It
//! is gated on [`allow_imperfect_clones`](crate::Device::allow_imperfect_clones) and addressed in
//! `CATCH`'s `(class, id, direction)` space, in the write direction. A rule is session state,
//! re-asserted on reconnect like a lock or a catch subscription.

use crate::protocol::opcode::{
    CATCH_CLS_CONTROL, CATCH_CLS_EMIT, CATCH_CLS_HID_IN, CATCH_CLS_HID_OUT, CATCH_CLS_VEND_BULK,
    CATCH_CLS_VEND_INTR, CATCH_ID_ANY, RW_ANSWER, RW_DROP, RW_NAK, RW_PASS, RW_PATCH, RW_REPLACE,
    RW_REPLY_PATCH, RW_REPLY_REPLACE, RW_STALL,
};
use crate::types::Direction;

/// Traffic class a rewrite rule may address (§3.14).
///
/// `CATCH`'s traffic classes in the write direction, exactly the set the box rewrites; the
/// parsed-input classes (button, key, media, axis) and the bus class have no variant.
/// [`Any`](RewriteClass::Any) acts at every surface a packet crosses.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum RewriteClass {
    /// Native HID input report as it arrived; `id` is the interface number. On the bound mouse a
    /// motion rewrite here survives only on reports the host chip leaves alone; rewrite motion at
    /// `Emit`.
    HidIn = CATCH_CLS_HID_IN,
    /// Interrupt-OUT report the game PC wrote, relayed to the device; `id` is the endpoint number,
    /// `direction` [`OUT`](crate::Direction::OUT).
    HidOut = CATCH_CLS_HID_OUT,
    /// Interrupt traffic on a vendor interface; `id` is the endpoint number, `direction`
    /// [`IN`](crate::Direction::IN) or [`OUT`](crate::Direction::OUT).
    VendorInterrupt = CATCH_CLS_VEND_INTR,
    /// Bulk traffic on a vendor interface; `id` is the endpoint number, `direction`
    /// [`IN`](crate::Direction::IN) or [`OUT`](crate::Direction::OUT).
    VendorBulk = CATCH_CLS_VEND_BULK,
    /// Proxied control transfer; `id` is the endpoint number (0 = EP0). On EP0 only class and vendor
    /// requests reach a rule (change descriptors with `PATCH`); above EP0 every request does.
    Control = CATCH_CLS_CONTROL,
    /// Outgoing wire, after the renderer; `id` is the endpoint number, `direction`
    /// [`IN`](crate::Direction::IN). Matches injected and rendered frames besides relayed ones.
    Emit = CATCH_CLS_EMIT,
    /// Wire wildcard `0xFF`: ranked below every other rule, `id` not compared, `Pass`, `Patch` and
    /// `Replace` only. It acts at each surface, so a native report can hit it at `HidIn` and `Emit`.
    Any = 0xFF,
}

impl RewriteClass {
    /// Wire `class` byte.
    pub fn as_u8(self) -> u8 {
        self as u8
    }

    /// Decodes a wire `class` byte; `None` if not rewritable.
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

    /// Whether this is the control class, the only one that can answer or rewrite a device reply.
    pub fn is_control(self) -> bool {
        matches!(self, RewriteClass::Control)
    }
}

/// What the top-ranked matching rewrite rule does to the packet (§3.14).
///
/// A report class ([`HidIn`](RewriteClass::HidIn), [`HidOut`](RewriteClass::HidOut),
/// [`Emit`](RewriteClass::Emit), the vendor classes) takes [`Pass`](RewriteAction::Pass),
/// [`Drop`](RewriteAction::Drop), [`Patch`](RewriteAction::Patch) or
/// [`Replace`](RewriteAction::Replace); [`Any`](RewriteClass::Any) takes `Pass`, `Patch` and `Replace`
/// only. The control class adds [`Answer`](RewriteAction::Answer), [`Stall`](RewriteAction::Stall),
/// [`Nak`](RewriteAction::Nak) and the two reply rewrites.
/// [`is_valid_for`](RewriteAction::is_valid_for) mirrors the box's check.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub enum RewriteAction {
    /// Leaves the matched packet untouched (shadows a broader rule).
    #[default]
    Pass = RW_PASS,
    /// Report class: the packet is not delivered. An `Emit` drop mutes the wire; a `HidIn` drop
    /// removes the device's contribution while injection still emits.
    Drop = RW_DROP,
    /// Overwrite the payload bytes at `offset`, length preserved. On `Control`, an OUT request's data
    /// stage only.
    Patch = RW_PATCH,
    /// The packet becomes the payload. On `Control`, the payload overwrites the start of an OUT
    /// request's data stage and `wLength` is kept.
    Replace = RW_REPLACE,
    /// Control: answer from the payload without asking the device.
    Answer = RW_ANSWER,
    /// Control: protocol STALL.
    Stall = RW_STALL,
    /// Control: NAK on EP0 until the host times out; STALL on a control endpoint above 0.
    Nak = RW_NAK,
    /// Control IN: overwrite the device's reply at `offset`.
    ReplyPatch = RW_REPLY_PATCH,
    /// Control IN: replace the device's reply with the payload.
    ReplyReplace = RW_REPLY_REPLACE,
}

impl RewriteAction {
    /// Wire `action` byte.
    pub fn as_u8(self) -> u8 {
        self as u8
    }

    /// Decodes a wire `action` byte; `None` if unknown.
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

    /// Whether the rule must supply a payload for this action.
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
    /// on report surfaces only, `Answer`/`Stall`/`Nak`/the reply rewrites on control only.
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

/// Rewrite rule the host installs on the box.
///
/// Keyed by `(class, id, direction, match, mask)`: rules differing in any of those are separate
/// entries; setting an existing key overwrites it. `match` and `mask` are one length (the box compares
/// the packet head byte for byte under `mask`); an empty match takes every packet on the address.
/// `offset` is where [`Patch`](RewriteAction::Patch) and [`ReplyPatch`](RewriteAction::ReplyPatch)
/// write; other actions ignore it. `payload` is the bytes an action that
/// [carries one](RewriteAction::carries_payload) supplies.
///
/// ```no_run
/// # use medius::{Device, Direction, Result};
/// # use medius::{RewriteRule, RewriteClass, RewriteAction};
/// # fn main() -> Result<()> {
/// let device = Device::find()?;
/// device.allow_imperfect_clones(true)?;
/// // Mute the clone's wire on interrupt-IN endpoint 1 (Emit is an IN endpoint).
/// device.set_rewrite(&RewriteRule::new(RewriteClass::Emit, 1, Direction::IN, RewriteAction::Drop))?;
/// # Ok(()) }
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RewriteRule {
    /// Addressed class.
    pub class: RewriteClass,
    /// Interface or endpoint number within the class.
    pub id: u16,
    /// Matched flow: `Both`/`Positive`/`Negative` only (the box rejects the bearing-relative ones).
    /// For an endpoint class, the endpoint's [`IN`](crate::Direction::IN)/[`OUT`](crate::Direction::OUT).
    pub direction: Direction,
    /// What the rule does to a matched packet.
    pub action: RewriteAction,
    /// Where [`Patch`](RewriteAction::Patch)/[`ReplyPatch`](RewriteAction::ReplyPatch) write.
    pub offset: u16,
    /// Head bytes compared under [`mask`](RewriteRule::mask); empty matches every packet.
    pub match_bytes: Vec<u8>,
    /// Mask over [`match_bytes`](RewriteRule::match_bytes); same length.
    pub mask: Vec<u8>,
    /// Payload for an action that carries one.
    pub payload: Vec<u8>,
}

impl Default for RewriteClass {
    /// [`Emit`](RewriteClass::Emit): the outgoing wire, the most common target.
    fn default() -> Self {
        RewriteClass::Emit
    }
}

impl RewriteRule {
    /// Rule with no match, no payload and `offset` 0; add them with [`matching`](Self::matching) and
    /// [`with_payload`](Self::with_payload).
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

    /// Narrow to packets whose head equals `match_bytes` under `mask`. Both are one length; a longer
    /// packet matches on its head.
    pub fn matching(mut self, match_bytes: impl Into<Vec<u8>>, mask: impl Into<Vec<u8>>) -> Self {
        self.match_bytes = match_bytes.into();
        self.mask = mask.into();
        self
    }

    /// Write offset for [`Patch`](RewriteAction::Patch)/[`ReplyPatch`](RewriteAction::ReplyPatch).
    pub fn at_offset(mut self, offset: u16) -> Self {
        self.offset = offset;
        self
    }

    /// Payload for an action that [carries one](RewriteAction::carries_payload).
    pub fn with_payload(mut self, payload: impl Into<Vec<u8>>) -> Self {
        self.payload = payload.into();
        self
    }

    /// The rule's `(class, id, direction, match, mask)` table key.
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

/// `(class, id, direction, match, mask)` key of one table entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RewriteKey {
    /// Addressed class.
    pub class: RewriteClass,
    /// Address within the class.
    pub id: u16,
    /// Matched flow.
    pub direction: Direction,
    /// Head bytes matched on.
    pub match_bytes: Vec<u8>,
    /// Mask over the match.
    pub mask: Vec<u8>,
}

/// Row of the decoded [`RewriteTable`] summary (§4.17): a rule's address, action and live counters,
/// without match/mask/payload bytes. [`query_rewrite_entry`](crate::Device::query_rewrite_entry)
/// reads the full rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RewriteEntry {
    /// Addressed class.
    pub class: RewriteClass,
    /// Address within the class.
    pub id: u16,
    /// Matched flow.
    pub direction: Direction,
    /// What the rule does to a matched packet.
    pub action: RewriteAction,
    /// `match`/`mask` bytes compared.
    pub match_len: u8,
    /// Write offset for a patching action.
    pub offset: u16,
    /// Payload bytes carried.
    pub payload_len: u16,
    /// Packets matched as the top-ranked rule since install or last overwrite, a
    /// [`Pass`](RewriteAction::Pass) rule included (saturating).
    pub hits: u16,
}

/// Decoded `RESP(REWRITE)` (§4.17): the rewrite table summary.
///
/// `generation` bumps on a table change and on a clear of a non-empty table. A reset, detach, link
/// loss, re-clone or the opt-in going off empties the table and returns it to 0. The crate ignores
/// it: its keepalive re-sends every held rule.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RewriteTable {
    /// The box refused the last new rule or overwrite for room: all 32 entries in use, or the
    /// 2048-byte payload pool full. The next table change, or a clear, resets it.
    pub table_full: bool,
    /// Generation counter (see the type docs).
    pub generation: u8,
    /// One row per installed rule, in installation order (the box's order). The box selects a match
    /// most specific first, whatever its order here.
    pub entries: Vec<RewriteEntry>,
}

impl RewriteTable {
    /// `[what][flags u8][gen u8][n u8]` then `n` ×
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
            // A class or action a newer box added is skipped; the rest of the table still reads.
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

// `[what][index][cls][id u16][dir][state=1][action][off u16][mlen][match mlen][mask mlen][payload]`,
// decoded into the rule that replays it.
pub(crate) fn rewrite_entry_from_payload(p: &[u8]) -> Option<RewriteRule> {
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

// Whole-table clear sentinel (`class 0xFF, id 0xFFFF, state 0`).
pub(crate) const REWRITE_CLEAR_ID: u16 = CATCH_ID_ANY;
