use crate::error::{Error, Result};
use crate::link::reconcile::StoredRewrite;
use crate::protocol::command::rewrite_payload;
use crate::protocol::opcode::{Q_REWRITE, Q_REWRITE_ENTRY};
use crate::protocol::{FrameType, Resp, parse_resp};
use crate::types::rewrite::{REWRITE_CLEAR_ID, rewrite_entry_from_payload};
use crate::types::{Direction, RewriteClass, RewriteRule, RewriteTable};

use super::Device;

impl Device {
    /// `REWRITE` (§3.14): install (add or overwrite) one rewrite rule.
    ///
    /// The on-box rewrite table matches traffic in flight and rewrites, answers, refuses or drops it
    /// per the rule's [`action`](RewriteRule::action). A rule is keyed by
    /// `(class, id, direction, match, mask)`; setting one whose key exists overwrites it. Rules are
    /// session state, re-asserted on reconnect and held alive by the keepalive exactly like a
    /// [`lock`](Device::lock) or a catch subscription, and cleared on control-PC silence,
    /// [`reset`](Device::reset), a re-clone, or the opt-in going off.
    ///
    /// Gated on [`allow_imperfect_clones`](Device::allow_imperfect_clones): with the opt-in off this
    /// returns [`Error::ImperfectRequired`](crate::Error::ImperfectRequired). The rule's `match` and
    /// `mask` must be the same length ([`Error::RewriteMaskLength`](crate::Error::RewriteMaskLength)),
    /// its `action` must be valid for its `class`
    /// ([`Error::RewriteActionClass`](crate::Error::RewriteActionClass)), and its `direction` must not
    /// be bearing-relative ([`Error::RelativeDirection`](crate::Error::RelativeDirection)). Delivery is
    /// fire-and-forget: [`query_rewrite`](Device::query_rewrite) confirms what the box actually holds.
    pub fn set_rewrite(&self, rule: &RewriteRule) -> Result<()> {
        validate_rule(rule)?;
        self.require_imperfect()?;
        self.set_rewrite_send(rule)
    }

    /// The validated `REWRITE` set with no opt-in pre-check, so the async wrapper can gate on the async
    /// query path. Records the rule for reconnect-replay, then rolls back if the frame never went out.
    pub(crate) fn set_rewrite_send(&self, rule: &RewriteRule) -> Result<()> {
        let stored = to_stored(rule);
        // Recorded before the write so a reconnect racing it still replays the rule, and rolled back
        // when the frame never went out (the lock pattern).
        let undo = self.link.desired().lock().apply_rewrite(stored);
        let sent = self.link.send(
            FrameType::Rewrite,
            &rewrite_payload(
                rule.class.as_u8(),
                rule.id,
                rule.direction.as_u8(),
                1,
                rule.action.as_u8(),
                rule.offset,
                &rule.match_bytes,
                &rule.mask,
                &rule.payload,
            ),
        );
        if sent.is_err() {
            self.link.desired().lock().restore_rewrite(undo);
        }
        sent
    }

    /// `REWRITE` remove (§3.14): drop the rule keyed by this rule's `(class, id, direction, match,
    /// mask)`. The rule's action and payload are ignored. A no-op on the box if no such rule is held.
    pub fn remove_rewrite(&self, rule: &RewriteRule) -> Result<()> {
        if rule.match_bytes.len() != rule.mask.len() {
            return Err(Error::RewriteMaskLength {
                match_len: rule.match_bytes.len(),
                mask_len: rule.mask.len(),
            });
        }
        let key = to_stored(rule).key();
        let undo = self.link.desired().lock().remove_rewrite(key);
        let sent = self.link.send(
            FrameType::Rewrite,
            &rewrite_payload(
                rule.class.as_u8(),
                rule.id,
                rule.direction.as_u8(),
                0,
                rule.action.as_u8(),
                rule.offset,
                &rule.match_bytes,
                &rule.mask,
                &[],
            ),
        );
        if sent.is_err() {
            self.link.desired().lock().restore_rewrite(undo);
        }
        sent
    }

    /// `REWRITE` clear (§3.14): drop the whole rewrite table (the `class 0xFF, id 0xFFFF, state 0`
    /// blanket). Always clears the crate's held rules, whatever the opt-in.
    pub fn clear_rewrite(&self) -> Result<()> {
        self.link.desired().lock().clear_rewrites();
        self.link.send(
            FrameType::Rewrite,
            &rewrite_payload(
                RewriteClass::Any.as_u8(),
                REWRITE_CLEAR_ID,
                Direction::Both.as_u8(),
                0,
                0,
                0,
                &[],
                &[],
                &[],
            ),
        )
    }

    /// `QUERY(REWRITE)` → [`RewriteTable`] (§4.17): the whole table's summary (flags, generation, and a
    /// row per rule without its match/mask/payload bytes). Read one rule in full with
    /// [`query_rewrite_entry`](Device::query_rewrite_entry).
    pub fn query_rewrite(&self) -> Result<RewriteTable> {
        let payload = self.link.query(Q_REWRITE)?;
        match parse_resp(&payload) {
            Some(Resp::Rewrite(t)) => Ok(t),
            _ => Err(Error::NoReply),
        }
    }

    /// `QUERY(REWRITE_ENTRY, index)` → [`RewriteRule`] (§4.17): one rule in full, in the shape
    /// [`set_rewrite`](Device::set_rewrite) takes, so a read rule replays as a set. `index` is the row
    /// in the [`query_rewrite`](Device::query_rewrite) summary.
    pub fn query_rewrite_entry(&self, index: u8) -> Result<RewriteRule> {
        let payload = self.link.query_indexed(Q_REWRITE_ENTRY, index)?;
        rewrite_entry_from_payload(&payload).ok_or(Error::NoReply)
    }
}

pub(crate) fn to_stored(rule: &RewriteRule) -> StoredRewrite {
    StoredRewrite {
        class: rule.class.as_u8(),
        id: rule.id,
        direction: rule.direction.as_u8(),
        action: rule.action.as_u8(),
        offset: rule.offset,
        match_bytes: rule.match_bytes.clone(),
        mask: rule.mask.clone(),
        payload: rule.payload.clone(),
    }
}

pub(crate) fn validate_rule(rule: &RewriteRule) -> Result<()> {
    if rule.match_bytes.len() != rule.mask.len() {
        return Err(Error::RewriteMaskLength {
            match_len: rule.match_bytes.len(),
            mask_len: rule.mask.len(),
        });
    }
    // REWRITE's direction byte is Both/Positive/Negative only; the box rejects the bearing-relative
    // pair by range, so surface it here rather than sending a frame the box drops.
    if rule.direction.is_relative() {
        return Err(Error::RelativeDirection {
            direction: rule.direction,
            what: "rewrite rule",
        });
    }
    if !rule.action.is_valid_for(rule.class) {
        return Err(Error::RewriteActionClass {
            action: rule.action,
            class: rule.class,
        });
    }
    Ok(())
}
