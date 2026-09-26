use crate::error::{Error, Result};
use crate::link::reconcile::StoredRewrite;
use crate::protocol::command::rewrite_payload;
use crate::protocol::opcode::{
    MAX_PAYLOAD, Q_REWRITE, Q_REWRITE_ENTRY, REWRITE_MATCH_MAX, REWRITE_MAX_ENTRIES,
    REWRITE_PAYLOAD_POOL,
};
use crate::protocol::{FrameType, Resp, parse_resp};
use crate::types::rewrite::{REWRITE_CLEAR_ID, rewrite_entry_from_payload};
use crate::types::{Direction, RewriteAction, RewriteClass, RewriteRule, RewriteTable};

use super::Device;

impl Device {
    /// `REWRITE` (§3.14): add or overwrite one rewrite rule.
    ///
    /// The box's rewrite table matches traffic in flight and rewrites, answers, refuses or drops it
    /// per the rule's [`action`](RewriteRule::action). Keyed by `(class, id, direction, match, mask)`;
    /// setting an existing key overwrites it. Rules are session state, re-asserted after a reconnect,
    /// a device-chip restart or any session release the box counts, and kept by the keepalive like a
    /// [`lock`](Device::lock). The box clears them on control-PC silence, [`reset`](Device::reset), a
    /// device detach, a link drop, a re-clone, or the opt-in going off.
    ///
    /// A new rule past [`REWRITE_MAX_ENTRIES`](crate::REWRITE_MAX_ENTRIES) is
    /// [`Error::RewriteTableFull`](crate::Error::RewriteTableFull); a payload past what the other held
    /// rules leave of [`REWRITE_PAYLOAD_POOL`](crate::REWRITE_PAYLOAD_POOL) is
    /// [`Error::RewritePoolFull`](crate::Error::RewritePoolFull).
    ///
    /// Gated on [`allow_imperfect_clones`](Device::allow_imperfect_clones): with the opt-in off,
    /// [`Error::ImperfectRequired`](crate::Error::ImperfectRequired). `match` and `mask` must be one
    /// length ([`Error::RewriteMaskLength`](crate::Error::RewriteMaskLength)), `action` valid for
    /// `class` ([`Error::RewriteActionClass`](crate::Error::RewriteActionClass)), and `direction` not
    /// bearing-relative ([`Error::RelativeDirection`](crate::Error::RelativeDirection)).
    /// Fire-and-forget: [`query_rewrite`](Device::query_rewrite) confirms what the box holds.
    pub fn set_rewrite(&self, rule: &RewriteRule) -> Result<()> {
        validate_rule(rule)?;
        self.require_imperfect()?;
        self.set_rewrite_checked(rule)
    }

    // Capacity check and send under one re-assert guard, so two threads at the last slot cannot both
    // pass, and the async wrapper (with its own opt-in check) gets the check too. The guard is not
    // reentrant: taking it again in `set_rewrite_send_locked` would deadlock.
    pub(crate) fn set_rewrite_checked(&self, rule: &RewriteRule) -> Result<()> {
        let _serial = self.link.reassert_guard();
        {
            let d = self.link.desired().lock();
            let key = to_stored(rule).key();
            // Costed like the box: payload against the pool less what an overwrite frees, then the
            // entry count for a new key.
            let free = REWRITE_PAYLOAD_POOL.saturating_sub(d.rewrite_pool_used_except(&key));
            if rule.payload.len() > free {
                return Err(Error::RewritePoolFull {
                    len: rule.payload.len(),
                    free,
                    limit: REWRITE_PAYLOAD_POOL,
                });
            }
            if !d.holds_rewrite(&key) && d.rewrite_count() >= REWRITE_MAX_ENTRIES {
                return Err(Error::RewriteTableFull {
                    limit: REWRITE_MAX_ENTRIES,
                });
            }
        }
        self.set_rewrite_send_locked(rule)
    }

    // No opt-in pre-check, so the async wrapper can gate on the async query path. The caller holds
    // the re-assert guard, so a concurrent remove/clear cannot interleave. Recorded before the write
    // so a racing reconnect replays it; rolled back if the frame never went out.
    fn set_rewrite_send_locked(&self, rule: &RewriteRule) -> Result<()> {
        let stored = to_stored(rule);
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

    /// `REWRITE` remove (§3.14): drop the rule with this rule's `(class, id, direction, match, mask)`
    /// key; action and payload are ignored. A no-op on the box if no such rule is held.
    pub fn remove_rewrite(&self, rule: &RewriteRule) -> Result<()> {
        if rule.match_bytes.len() != rule.mask.len() {
            return Err(Error::RewriteMaskLength {
                match_len: rule.match_bytes.len(),
                mask_len: rule.mask.len(),
            });
        }
        let key = to_stored(rule).key();
        // Serialised against re-asserts, or a keepalive tick can re-send a stale add after the
        // remove and leave the rule live.
        let _serial = self.link.reassert_guard();
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

    /// `REWRITE` clear (§3.14): drop the whole table (the `class 0xFF, id 0xFFFF, state 0` blanket).
    /// Always clears the crate's held rules, whatever the opt-in.
    pub fn clear_rewrite(&self) -> Result<()> {
        // Serialised against re-asserts; the snapshot restores the rules if the send fails (the box
        // still holds them).
        let _serial = self.link.reassert_guard();
        let held = {
            let mut d = self.link.desired().lock();
            let held = d.held_rewrites();
            d.clear_rewrites();
            held
        };
        let sent = self.link.send(
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
        );
        if sent.is_err() {
            let mut d = self.link.desired().lock();
            for r in held {
                d.apply_rewrite(r);
            }
        }
        sent
    }

    /// `QUERY(REWRITE)` → [`RewriteTable`] (§4.17): table summary (flags, generation, a row per rule
    /// without match/mask/payload bytes). [`query_rewrite_entry`](Device::query_rewrite_entry) reads
    /// one rule in full.
    pub fn query_rewrite(&self) -> Result<RewriteTable> {
        let payload = self.link.query(Q_REWRITE)?;
        match parse_resp(&payload) {
            Some(Resp::Rewrite(t)) => Ok(t),
            _ => Err(Error::NoReply),
        }
    }

    /// `QUERY(REWRITE_ENTRY, index)` → [`RewriteRule`] (§4.17): one rule in full, in
    /// [`set_rewrite`](Device::set_rewrite)'s shape so it replays as a set. `index` is the row in the
    /// [`query_rewrite`](Device::query_rewrite) summary.
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
    if rule.match_bytes.len() > REWRITE_MATCH_MAX {
        return Err(Error::RewriteMatchTooLong {
            len: rule.match_bytes.len(),
            limit: REWRITE_MATCH_MAX,
        });
    }
    // The box rejects the bearing-relative pair by range and drops the frame.
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
    // The box refuses a rule its read-back reply cannot carry; that header is two bytes wider than
    // the command's, so a rule can fit its frame and still be refused.
    const ENTRY_HDR: usize = 11;
    let room = MAX_PAYLOAD - ENTRY_HDR - 2 * rule.match_bytes.len();
    if rule.payload.len() > room {
        return Err(Error::RewritePayloadTooLarge {
            action: rule.action,
            class: rule.class,
            len: rule.payload.len(),
            offset: rule.offset as usize,
            cap: room,
        });
    }
    // Mirrors the box's head caps (rewrite_tab.h): 64 bytes on a report surface, 8+2048 on control.
    const HEAD_REPORT: usize = 64;
    const HEAD_CONTROL: usize = 8 + 2048;
    let plen = rule.payload.len();
    let off = rule.offset as usize;
    let report_cap = if rule.class == RewriteClass::Control {
        HEAD_CONTROL
    } else {
        HEAD_REPORT
    };
    let (over, cap) = match rule.action {
        RewriteAction::Patch | RewriteAction::ReplyPatch => (off + plen > report_cap, report_cap),
        RewriteAction::Replace => (plen > report_cap, report_cap),
        RewriteAction::Answer | RewriteAction::ReplyReplace => (plen > HEAD_CONTROL, HEAD_CONTROL),
        _ => (false, 0),
    };
    if over {
        return Err(Error::RewritePayloadTooLarge {
            action: rule.action,
            class: rule.class,
            len: plen,
            offset: off,
            cap,
        });
    }
    Ok(())
}
