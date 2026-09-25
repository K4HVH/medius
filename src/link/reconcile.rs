use std::collections::BTreeMap;
use std::collections::btree_map::Entry;

use crate::device::clip::{bind_packet_payload, bind_payload};
use crate::link::catch::FilterSet;
use crate::protocol::FrameType;
use crate::protocol::command::clip_set_payload;
use crate::protocol::opcode::{
    BTN_COUNT, CLIP_SET_AUTOLOCK, CLIP_SET_LOOP, CLIP_SET_RETAIN, CLIP_SET_RIDE, LOCK_CLS_AXIS,
    LOCK_CLS_BTN, LOCK_CLS_MEDIA, LOCK_DIR_AGAINST, LOCK_DIR_BOTH, LOCK_DIR_NEG, LOCK_DIR_POS,
    LOCK_DIR_WITH, LOCK_ID_ALL, LOCK_SCALE_BLOCK, LOCK_SCALE_PASS, MAX_BUTTONS,
};
use crate::types::lock::blanket_scope;
use crate::types::{
    Action, Class, ClipPacketTrigger, ClipSettings, ClipState, ClipStatus, ClipTrigger, Usage,
};

/// A lock the host wants held, keyed by its wire fields so a reapply is exact and idempotent.
pub(crate) type LockKey = (u8, u16, u8);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Override {
    #[default]
    None,
    Press,
    Force,
}

impl Override {
    pub(crate) fn as_action(self) -> Option<Action> {
        match self {
            Override::None => None,
            Override::Press => Some(Action::Press),
            Override::Force => Some(Action::ForceRelease),
        }
    }

    fn applied(action: Action) -> Override {
        match action {
            Action::Press => Override::Press,
            Action::ForceRelease => Override::Force,
            Action::SoftRelease => Override::None,
        }
    }
}

const SLOT_DIRS: [u8; 4] = [LOCK_DIR_POS, LOCK_DIR_NEG, LOCK_DIR_WITH, LOCK_DIR_AGAINST];

// One row of the box's lock table: the scale each of the four slots holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Slots([i16; 4]);

impl Default for Slots {
    fn default() -> Slots {
        Slots([LOCK_SCALE_PASS; 4])
    }
}

impl Slots {
    fn write(&mut self, dir: u8, scale: i16) {
        if dir == LOCK_DIR_BOTH {
            self.0 = [scale, scale, LOCK_SCALE_PASS, LOCK_SCALE_PASS];
        } else if let Some(i) = SLOT_DIRS.iter().position(|&d| d == dir) {
            self.0[i] = scale;
        }
    }

    fn is_clear(self) -> bool {
        self.0.iter().all(|&s| s == LOCK_SCALE_PASS)
    }

    // The fewest LOCK commands that rebuild this row on a box holding nothing.
    fn commands(self) -> Vec<(u8, i16)> {
        let [p, n, w, a] = self.0;
        if p == n && w == LOCK_SCALE_PASS && a == LOCK_SCALE_PASS {
            return if p == LOCK_SCALE_PASS {
                Vec::new()
            } else {
                vec![(LOCK_DIR_BOTH, p)]
            };
        }
        (0..4)
            .filter(|&i| self.0[i] != LOCK_SCALE_PASS)
            .map(|i| (SLOT_DIRS[i], self.0[i]))
            .collect()
    }
}

/// What [`DesiredState::apply_lock`] overwrote, enough to put it back when the frame never went out.
#[derive(Debug)]
pub(crate) struct LockUndo {
    rows: Vec<((u8, u16), Option<Slots>)>,
    media_order: Vec<u16>,
}

/// A rewrite rule the host wants held, in its wire fields, so a reconnect re-sends it byte-for-byte.
/// Rules are session state on the same lifecycle as locks and catches (§3.14).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StoredRewrite {
    pub(crate) class: u8,
    pub(crate) id: u16,
    pub(crate) direction: u8,
    pub(crate) action: u8,
    pub(crate) offset: u16,
    pub(crate) match_bytes: Vec<u8>,
    pub(crate) mask: Vec<u8>,
    pub(crate) payload: Vec<u8>,
}

/// The `(class, id, direction, match, mask)` key the box files a rule under; two rules that differ in
/// any of these are separate entries.
pub(crate) type RewriteWireKey = (u8, u16, u8, Vec<u8>, Vec<u8>);

impl StoredRewrite {
    pub(crate) fn key(&self) -> RewriteWireKey {
        (
            self.class,
            self.id,
            self.direction,
            self.match_bytes.clone(),
            self.mask.clone(),
        )
    }
}

/// What [`DesiredState::apply_rewrite`]/[`remove_rewrite`](DesiredState::remove_rewrite) changed, enough
/// to put it back when the frame never went out.
#[derive(Debug)]
pub(crate) struct RewriteUndo {
    key: RewriteWireKey,
    prior: Option<(usize, StoredRewrite)>,
}

/// A field transform the host wants held, in its wire fields, so a reconnect re-sends it byte-for-byte.
/// Transforms are session state on the same lifecycle as locks and rewrites (§3.15).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StoredTransform {
    pub(crate) op: u8,
    pub(crate) sclass: u8,
    pub(crate) sid: u16,
    pub(crate) dclass: u8,
    pub(crate) did: u16,
}

/// The `(sclass, sid, dclass, did)` key the box files a transform under; two entries that differ in any
/// of these are separate rows, and setting one whose key exists overwrites its op.
pub(crate) type TransformWireKey = (u8, u16, u8, u16);

impl StoredTransform {
    pub(crate) fn key(&self) -> TransformWireKey {
        (self.sclass, self.sid, self.dclass, self.did)
    }
}

/// What [`DesiredState::apply_transform`]/[`remove_transform`](DesiredState::remove_transform) changed,
/// enough to put it back when the frame never went out.
#[derive(Debug)]
pub(crate) struct TransformUndo {
    key: TransformWireKey,
    prior: Option<(usize, StoredTransform)>,
}

/// PC-owned injection + subscription state, re-asserted after a reconnect so held usages and open catches survive a control-link blip.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct DesiredState {
    overrides: BTreeMap<(u8, u16), Override>, // never sits at None in the map
    // One entry per box lock-table row. A row every slot passes is not held at all and is dropped, so
    // `locks` stays exactly the set a reconnect has to re-send.
    locks: BTreeMap<(u8, u16), Slots>,
    // Granular media rows in the order they were taken.
    media_order: Vec<u16>,
    catch: FilterSet,
    // The rewrite-rule table the box should be holding, in the box's own order: it breaks a tie
    // between equally specific rules by the earlier entry. Re-asserted on reconnect and by the keepalive.
    rewrites: Vec<StoredRewrite>,
    // The field-transform table the box should be holding, keyed by (sclass, sid, dclass, did) so a
    // Session state re-asserted on reconnect and by the keepalive.
    transforms: Vec<StoredTransform>,
    // The clone's declared button count, cached from `RESP(CAPS)`: the handshake reads it, and a
    // reconnect re-reads it.
    declared_buttons: Option<u8>,
    clip: ClipHeld,
}

// What the box holds of a clip: a loaded ring, settings off their defaults, and both kinds of
// trigger.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct ClipHeld {
    // Appended since the last clear.
    loaded: bool,
    // Whether the box's ring held the clip when last known: an append sets it, a clear or a reading of
    // an empty idle ring clears it. A release loses a clip only while it is set.
    ring: bool,
    // Moved by every append and clear, so a reading taken across one is not trusted.
    ring_gen: u64,
    // The box dropped the loaded clip; the next append or clear resets it.
    lost: bool,
    settings: [u8; 4], // by CLIP_SET id; every default is 0
    triggers: BTreeMap<ClipTriggerKey, ClipTrigger>,
    // In the box's own order: it breaks a tie between equally specific triggers by the earlier one.
    packet_triggers: Vec<ClipPacketTrigger>,
}

// A ring with bytes in it, or an engine that is not idle (a streaming clip that ran dry stays
// playing), is a loaded clip.
fn ring_holds(status: &ClipStatus) -> bool {
    status.total != 0 || status.state != ClipState::Idle
}

// The (class, id, edge) key the box holds an input trigger under.
pub(crate) type ClipTriggerKey = (u8, u16, u8);

pub(crate) fn clip_trigger_key(t: &ClipTrigger) -> ClipTriggerKey {
    let (class, id) = t.on.class_id();
    (class, id, t.edge.as_u8())
}

// The (class, id, dir, match, mask) key the box holds a packet trigger under.
pub(crate) type ClipPacketKey = (u8, u16, u8, Vec<u8>, Vec<u8>);

pub(crate) fn clip_packet_key(t: &ClipPacketTrigger) -> ClipPacketKey {
    (
        t.class.as_u8(),
        t.id,
        t.direction.as_u8(),
        t.match_bytes.clone(),
        t.mask.clone(),
    )
}

impl DesiredState {
    // An append or a clear: either way the caller has dealt with a lost clip.
    pub(crate) fn clip_loaded(&mut self, loaded: bool) {
        self.clip.loaded = loaded;
        self.clip.ring = loaded;
        self.clip.ring_gen += 1;
        self.clip.lost = false;
    }

    pub(crate) fn clip_ring_gen(&self) -> u64 {
        self.clip.ring_gen
    }

    pub(crate) fn clip_ring_held(&self) -> bool {
        self.clip.ring
    }

    // A reading of the box's ring, taken when `ring_gen` read `seen`. With `released`, the box released
    // the session before it, and a ring it held is lost.
    pub(crate) fn clip_note_ring(&mut self, seen: u64, status: &ClipStatus, released: bool) {
        if seen != self.clip.ring_gen {
            return;
        }
        let holds = ring_holds(status);
        if released && self.clip.ring && !holds {
            self.clip.lost = true;
            self.clip.loaded = false;
        }
        self.clip.ring = holds;
    }

    pub(crate) fn clip_lost(&self) -> bool {
        self.clip.lost
    }

    pub(crate) fn clip_setting(&mut self, id: u8, value: u8) {
        if let Some(s) = self.clip.settings.get_mut(id as usize) {
            *s = value;
        }
    }

    pub(crate) fn clip_bind(&mut self, trigger: ClipTrigger) {
        self.clip
            .triggers
            .insert(clip_trigger_key(&trigger), trigger);
    }

    pub(crate) fn clip_unbind(&mut self, key: &ClipTriggerKey) {
        self.clip.triggers.remove(key);
    }

    // An overwrite keeps its place, as clip_ptrig_set keeps it.
    pub(crate) fn clip_packet_bind(&mut self, trigger: &ClipPacketTrigger) {
        let key = clip_packet_key(trigger);
        match self
            .clip
            .packet_triggers
            .iter_mut()
            .find(|t| clip_packet_key(t) == key)
        {
            Some(held) => *held = trigger.clone(),
            None => self.clip.packet_triggers.push(trigger.clone()),
        }
    }

    pub(crate) fn clip_packet_unbind(&mut self, key: &ClipPacketKey) {
        self.clip
            .packet_triggers
            .retain(|t| clip_packet_key(t) != *key);
    }

    // The opt-in went off, and the box removed every consuming packet trigger with it. Returns what
    // was dropped, for a caller whose frame never went out to put back.
    pub(crate) fn clip_packet_drop_consuming(&mut self) -> Vec<ClipPacketTrigger> {
        let dropped: Vec<ClipPacketTrigger> = self
            .clip
            .packet_triggers
            .iter()
            .filter(|t| t.consume)
            .cloned()
            .collect();
        self.clip.packet_triggers.retain(|t| !t.consume);
        dropped
    }

    // The frames that rebuild the held clip settings and triggers on a box that holds none of them.
    pub(crate) fn clip_config_frames(&self) -> Vec<(FrameType, Vec<u8>)> {
        let settings = (0u8..)
            .zip(self.clip.settings)
            .filter(|&(_, v)| v != 0)
            .map(|(id, v)| (FrameType::ClipSet, clip_set_payload(id, v).to_vec()));
        let triggers = self
            .clip
            .triggers
            .values()
            .map(|t| (FrameType::ClipTrigger, bind_payload(t).to_vec()));
        let packets = self
            .clip
            .packet_triggers
            .iter()
            .map(|t| (FrameType::ClipTrigger, bind_packet_payload(t)));
        settings.chain(triggers).chain(packets).collect()
    }

    pub(crate) fn clip_triggers_clear(&mut self) {
        self.clip.triggers.clear();
        self.clip.packet_triggers.clear();
    }

    // Take the box's own answer for what it holds, read back after a reconnect.
    pub(crate) fn clip_adopt(&mut self, status: &ClipStatus, settings: &ClipSettings) {
        let mut scalars = [0u8; 4];
        scalars[CLIP_SET_AUTOLOCK as usize] = blanket_scope(&settings.autolock);
        scalars[CLIP_SET_LOOP as usize] = settings.loop_ as u8;
        scalars[CLIP_SET_RETAIN as usize] = settings.retain as u8;
        scalars[CLIP_SET_RIDE as usize] = settings.ride as u8;
        let loaded = ring_holds(status);
        self.clip = ClipHeld {
            loaded,
            ring: loaded,
            ring_gen: self.clip.ring_gen + 1,
            lost: self.clip.lost || (self.clip.ring && !loaded),
            settings: scalars,
            triggers: settings
                .triggers
                .iter()
                .map(|t| (clip_trigger_key(t), *t))
                .collect(),
            packet_triggers: settings
                .packet_triggers
                .iter()
                .map(|e| e.trigger.clone())
                .collect(),
        };
    }

    /// Record a momentary-usage override (any class) for reconnect-replay.
    pub(crate) fn apply(&mut self, usage: Usage, action: Action) {
        let key = usage.class_id();
        match Override::applied(action) {
            Override::None => {
                self.overrides.remove(&key);
            }
            ov => {
                self.overrides.insert(key, ov);
            }
        }
    }

    // Track a scale (any class) so a reconnect re-asserts it, as the box's own table would hold it.
    //
    // A momentary usage carries one bit, so the box stores the block or pass it amounts to and the
    // number sent is truncated to that; recording the raw byte would leave a scale above a full pass
    // held here as a lock the box released. A button blanket is held as one unexpanded row and
    // expanded at reassert time onto the declared count (see `held_locks`); a single button touched
    // while it is held materialises it first, so releasing one button afterwards is not undone by the
    // replay.
    pub(crate) fn apply_lock(&mut self, key: LockKey, scale: i16) -> LockUndo {
        let (class, id, dir) = key;
        let scale = if class == LOCK_CLS_AXIS {
            scale
        } else if scale < LOCK_SCALE_PASS {
            LOCK_SCALE_BLOCK
        } else {
            LOCK_SCALE_PASS
        };
        let mut undo = LockUndo {
            rows: Vec::new(),
            media_order: self.media_order.clone(),
        };
        if class == LOCK_CLS_BTN && id == LOCK_ID_ALL {
            // The blanket subsumes every individual button row, the way a box-side re-expansion of
            // `LOCK[BTN][ID_ALL]` rewrites each one, and is then held unexpanded so a reconnect widens
            // it onto the count CAPS reports then.
            self.clear_button_rows(&mut undo);
            self.write_lock_row(class, id, dir, scale, &mut undo);
        } else if class == LOCK_CLS_BTN && self.locks.contains_key(&(LOCK_CLS_BTN, LOCK_ID_ALL)) {
            self.burst_button_blanket(&mut undo);
            self.write_lock_row(class, id, dir, scale, &mut undo);
        } else {
            self.write_lock_row(class, id, dir, scale, &mut undo);
        }
        undo
    }

    // Write one lock-table row, dropping it when every slot passes and tracking the media slot order.
    fn write_lock_row(&mut self, class: u8, id: u16, dir: u8, scale: i16, undo: &mut LockUndo) {
        let key = (class, id);
        undo.rows.push((key, self.locks.get(&key).copied()));
        let row = self.locks.entry(key).or_default();
        row.write(dir, scale);
        if row.is_clear() {
            self.locks.remove(&key);
            if is_media_slot(class, id) {
                self.media_order.retain(|&m| m != id);
            }
        } else if is_media_slot(class, id) && !self.media_order.contains(&id) {
            self.media_order.push(id);
        }
    }

    // Drop every individual button row: a fresh button blanket covers them all, exactly as the box
    // rewrites each button row when it re-expands `LOCK[BTN][ID_ALL]`.
    fn clear_button_rows(&mut self, undo: &mut LockUndo) {
        let ids: Vec<u16> = self
            .locks
            .keys()
            .filter(|(class, id)| *class == LOCK_CLS_BTN && *id != LOCK_ID_ALL)
            .map(|&(_, id)| id)
            .collect();
        for id in ids {
            let key = (LOCK_CLS_BTN, id);
            undo.rows.push((key, self.locks.get(&key).copied()));
            self.locks.remove(&key);
        }
    }

    // Materialise the held button blanket onto the declared buttons, then drop the blanket row, so a
    // single button written next (a release, most often) leaves the rest of the group held.
    fn burst_button_blanket(&mut self, undo: &mut LockUndo) {
        let key = (LOCK_CLS_BTN, LOCK_ID_ALL);
        let Some(blanket) = self.locks.get(&key).copied() else {
            return;
        };
        for b in 0..self.button_count() {
            let row = (LOCK_CLS_BTN, b);
            if let Entry::Vacant(slot) = self.locks.entry(row) {
                undo.rows.push((row, None));
                slot.insert(blanket);
            }
        }
        undo.rows.push((key, Some(blanket)));
        self.locks.remove(&key);
    }

    /// Cache the clone's declared button count from a `RESP(CAPS)`, capped at the box's ceiling. A
    /// button blanket expands onto this many rows at reassert time, so a reconnect re-asserts a lock
    /// on a button past the five named ones.
    pub(crate) fn note_declared_buttons(&mut self, n_buttons: u8) {
        self.declared_buttons = Some(n_buttons.min(MAX_BUTTONS));
    }

    // How many button rows a button blanket expands onto: the declared count once CAPS is read,
    // else the five named buttons.
    fn button_count(&self) -> u16 {
        self.declared_buttons.unwrap_or(BTN_COUNT) as u16
    }

    /// Put back what an `apply_lock` wrote, for a frame that never reached the transport. Undone
    /// newest-change-first, so a step that touched a row more than once (a blanket burst then the
    /// single-button write over it) rewinds to exactly the row it started from.
    pub(crate) fn restore_lock(&mut self, undo: LockUndo) {
        for (key, row) in undo.rows.into_iter().rev() {
            match row {
                Some(row) => self.locks.insert(key, row),
                None => self.locks.remove(&key),
            };
        }
        self.media_order = undo.media_order;
    }

    /// Record a rewrite rule (add or overwrite) for reconnect-replay, returning the prior state so the
    /// device layer can roll it back if the frame never went out (the [`apply_lock`] pattern).
    /// An overwrite that changes the rule moves it to the end, as the box re-adds it; the same rule
    /// again keeps its place.
    pub(crate) fn apply_rewrite(&mut self, rule: StoredRewrite) -> RewriteUndo {
        let key = rule.key();
        let at = self.rewrites.iter().position(|r| r.key() == key);
        if let Some(i) = at
            && self.rewrites[i] == rule
        {
            return RewriteUndo {
                key,
                prior: Some((i, rule)),
            };
        }
        let prior = at.map(|i| (i, self.rewrites.remove(i)));
        self.rewrites.push(rule);
        RewriteUndo { key, prior }
    }

    /// Record a rewrite-rule removal, returning the prior state for the same rollback path.
    pub(crate) fn remove_rewrite(&mut self, key: RewriteWireKey) -> RewriteUndo {
        let prior = self
            .rewrites
            .iter()
            .position(|r| r.key() == key)
            .map(|i| (i, self.rewrites.remove(i)));
        RewriteUndo { key, prior }
    }

    /// Put back what an `apply_rewrite`/`remove_rewrite` changed, for a frame that never went out: the
    /// rule returns to the place it held, or leaves if there was none.
    pub(crate) fn restore_rewrite(&mut self, undo: RewriteUndo) {
        self.rewrites.retain(|r| r.key() != undo.key);
        if let Some((i, rule)) = undo.prior {
            let at = i.min(self.rewrites.len());
            self.rewrites.insert(at, rule);
        }
    }

    /// Drop every held rewrite rule (the whole-table clear).
    pub(crate) fn clear_rewrites(&mut self) {
        self.rewrites.clear();
    }

    /// Every held rewrite rule, for the reconnect and keepalive re-assertion.
    pub(crate) fn held_rewrites(&self) -> Vec<StoredRewrite> {
        self.rewrites.clone()
    }

    /// Whether this key is already held, so a set is an overwrite rather than an insert.
    pub(crate) fn holds_rewrite(&self, key: &RewriteWireKey) -> bool {
        self.rewrites.iter().any(|r| r.key() == *key)
    }

    /// How many rules are held, against the box's ceiling.
    pub(crate) fn rewrite_count(&self) -> usize {
        self.rewrites.len()
    }

    /// Payload bytes the held rules take from the box's pool, less the rule under `key`: what the box
    /// counts before it costs an add or an overwrite of that key.
    pub(crate) fn rewrite_pool_used_except(&self, key: &RewriteWireKey) -> usize {
        self.rewrites
            .iter()
            .filter(|r| r.key() != *key)
            .map(|r| r.payload.len())
            .sum()
    }

    /// Record a field transform (add or overwrite) for reconnect-replay, returning the prior state so
    /// the device layer can roll it back if the frame never went out (the [`apply_rewrite`] pattern).
    /// An overwrite keeps the entry's position, which is what the box does.
    pub(crate) fn apply_transform(&mut self, entry: StoredTransform) -> TransformUndo {
        let key = entry.key();
        match self.transforms.iter().position(|e| e.key() == key) {
            Some(i) => {
                let prior = core::mem::replace(&mut self.transforms[i], entry);
                TransformUndo {
                    key,
                    prior: Some((i, prior)),
                }
            }
            None => {
                self.transforms.push(entry);
                TransformUndo { key, prior: None }
            }
        }
    }

    /// Record a transform removal, returning the prior state for the same rollback path.
    pub(crate) fn remove_transform(&mut self, key: TransformWireKey) -> TransformUndo {
        match self.transforms.iter().position(|e| e.key() == key) {
            Some(i) => TransformUndo {
                key,
                prior: Some((i, self.transforms.remove(i))),
            },
            None => TransformUndo { key, prior: None },
        }
    }

    /// Put back what an `apply_transform`/`remove_transform` changed, for a frame that never went out.
    /// The entry returns to the position it held, or leaves if there was none.
    pub(crate) fn restore_transform(&mut self, undo: TransformUndo) {
        match undo.prior {
            Some((i, entry)) => {
                if let Some(cur) = self.transforms.iter().position(|e| e.key() == undo.key) {
                    self.transforms[cur] = entry;
                } else {
                    let at = i.min(self.transforms.len());
                    self.transforms.insert(at, entry);
                }
            }
            None => self.transforms.retain(|e| e.key() != undo.key),
        }
    }

    /// Drop every held transform (the whole-table clear).
    pub(crate) fn clear_transforms(&mut self) {
        self.transforms.clear();
    }

    /// Whether this key is already held, so a set is an overwrite rather than an insert.
    pub(crate) fn holds_transform(&self, key: TransformWireKey) -> bool {
        self.transforms.iter().any(|e| e.key() == key)
    }

    /// How many transforms are held, against the box's ceiling.
    pub(crate) fn transform_count(&self) -> usize {
        self.transforms.len()
    }

    /// Every held transform, in installation order, for the reconnect and keepalive re-assertion.
    pub(crate) fn held_transforms(&self) -> Vec<StoredTransform> {
        self.transforms.clone()
    }

    pub(crate) fn clear(&mut self) {
        // Catch teardown is handled by Link::catch_disconnect_all (drops the EventStream senders); catch
        // otherwise clears firmware-side on the same lifecycle as injection.
        self.overrides.clear();
        self.locks.clear();
        self.media_order.clear();
        // `RESET` clears the box's rewrite table too (§3.14), so drop the local copy or the keepalive
        // would re-assert rules the reset was meant to remove. Patches are not session state and stay.
        self.rewrites.clear();
        // `RESET` clears the box's transform table too (§3.15, the injection-adjacent lifecycle), so
        // drop the local copy for the same reason.
        self.transforms.clear();
        self.clip = ClipHeld::default();
    }

    /// The catch subscription table the box should be holding (re-asserted on reconnect).
    pub(crate) fn set_catch(&mut self, filters: FilterSet) {
        self.catch = filters;
    }

    pub(crate) fn catch(&self) -> FilterSet {
        self.catch.clone()
    }

    /// Idle = nothing for the keepalive to hold alive; a catch subscription, a rewrite rule or a
    /// transform counts.
    pub(crate) fn is_idle(&self) -> bool {
        self.catch.is_empty()
            && self.overrides.is_empty()
            && self.locks.is_empty()
            && self.rewrites.is_empty()
            && self.transforms.is_empty()
            && !self.clip.loaded
            && self.clip.settings == [0; 4]
            && self.clip.triggers.is_empty()
            && self.clip.packet_triggers.is_empty()
    }

    /// Every held momentary override, as `(Usage, Action)`, for the reconnect reapply.
    pub(crate) fn held(&self) -> impl Iterator<Item = (Usage, Action)> + '_ {
        self.overrides.iter().filter_map(|(&(cls, id), ov)| {
            let action = ov.as_action()?;
            let class = Class::from_u8(cls)?;
            Some((Usage::new(class, id), action))
        })
    }

    // The `(key, scale)` commands that rebuild every held row, for the reconnect reapply.
    pub(crate) fn held_locks(&self) -> Vec<(LockKey, i16)> {
        let button_count = self.button_count();
        let mut rows: Vec<(&(u8, u16), &Slots)> = self.locks.iter().collect();
        rows.sort_by_key(|((class, id), _)| (*class, self.media_rank(*class, *id), *id));
        rows.into_iter()
            .flat_map(|(&(class, id), row)| {
                let commands = row.commands();
                let ids: Vec<u16> = if class == LOCK_CLS_BTN && id == LOCK_ID_ALL {
                    (0..button_count).collect()
                } else {
                    vec![id]
                };
                ids.into_iter().flat_map(move |id| {
                    commands
                        .clone()
                        .into_iter()
                        .map(move |(dir, scale)| ((class, id, dir), scale))
                })
            })
            .collect()
    }

    // Which slot a media row took; the media blanket holds none and goes last, and every other class
    // ranks alike and stays ordered by id.
    fn media_rank(&self, class: u8, id: u16) -> usize {
        if class != LOCK_CLS_MEDIA {
            return 0;
        }
        self.media_order
            .iter()
            .position(|&m| m == id)
            .unwrap_or(usize::MAX)
    }
}

// A granular media lock, the only class the box holds in a slot array. Its blanket is a separate flag.
fn is_media_slot(class: u8, id: u16) -> bool {
    class == LOCK_CLS_MEDIA && id != LOCK_ID_ALL
}
