use std::collections::BTreeMap;
use std::collections::btree_map::Entry;

use crate::link::catch::FilterSet;
use crate::protocol::opcode::{
    BTN_COUNT, LOCK_CLS_AXIS, LOCK_CLS_BTN, LOCK_CLS_MEDIA, LOCK_DIR_AGAINST, LOCK_DIR_BOTH,
    LOCK_DIR_NEG, LOCK_DIR_POS, LOCK_DIR_WITH, LOCK_ID_ALL, LOCK_SCALE_BLOCK, LOCK_SCALE_PASS,
    MAX_BUTTONS,
};
use crate::types::{Action, Class, Usage};

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

// One row of the box's lock table: the scale each of the four slots holds. Tracking the row rather
// than the direction byte that was sent is what makes a release exact: Both writes the absolute
// pair and passes the relative one, so a later single-direction unlock has to clear one slot out of a
// group write, which a key per direction cannot express.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Slots([u8; 4]);

impl Default for Slots {
    fn default() -> Slots {
        Slots([LOCK_SCALE_PASS; 4])
    }
}

impl Slots {
    fn write(&mut self, dir: u8, scale: u8) {
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
    fn commands(self) -> Vec<(u8, u8)> {
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
    prior: Option<StoredRewrite>,
}

/// PC-owned injection + subscription state, re-asserted after a reconnect so held usages and open catches survive a control-link blip.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct DesiredState {
    overrides: BTreeMap<(u8, u16), Override>, // never sits at None in the map
    // One entry per box lock-table row. A row every slot passes is not held at all and is dropped, so
    // `locks` stays exactly the set a reconnect has to re-send.
    locks: BTreeMap<(u8, u16), Slots>,
    // Granular media rows in the order they were taken. The box keeps its media locks in a fixed
    // 8-slot array filled first-free-slot-first, so a replay in id order refills those slots in a
    // different order and, past the eight it holds, drops a different usage than the box was
    // dropping. The blanket is its own flag on the box, not a slot, so it stays out.
    media_order: Vec<u16>,
    catch: FilterSet,
    // The rewrite-rule table the box should be holding, keyed by wire key so a re-set is exact and
    // idempotent. Re-asserted on reconnect and by the keepalive, exactly like `catch`.
    rewrites: BTreeMap<RewriteWireKey, StoredRewrite>,
    // The clone's declared button count, cached from `RESP(CAPS)`: the handshake reads it, and a
    // reconnect re-reads it. A button blanket is held UNEXPANDED and expanded onto this many rows at
    // reassert time, so a wide-button lock set before the caller's own `caps()` still re-asserts every
    // declared button across a reconnect, and a device swapped in during the blip re-asserts onto its
    // count. `None` before any CAPS read: the blanket then expands onto the five named buttons. A
    // device fact, not PC-owned injection state, so `clear()`/`is_idle()` leave it alone.
    declared_buttons: Option<u8>,
}

impl DesiredState {
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
    pub(crate) fn apply_lock(&mut self, key: LockKey, scale: u8) -> LockUndo {
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
    fn write_lock_row(&mut self, class: u8, id: u16, dir: u8, scale: u8, undo: &mut LockUndo) {
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

    // How many button rows a button blanket expands onto: the declared count once CAPS is read, else
    // the five named buttons. The box holds no button-blanket flag, so the host does the expansion, at
    // reassert time off the current count. Always within the box's ceiling.
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
    pub(crate) fn apply_rewrite(&mut self, rule: StoredRewrite) -> RewriteUndo {
        let key = rule.key();
        let prior = self.rewrites.insert(key.clone(), rule);
        RewriteUndo { key, prior }
    }

    /// Record a rewrite-rule removal, returning the prior state for the same rollback path.
    pub(crate) fn remove_rewrite(&mut self, key: RewriteWireKey) -> RewriteUndo {
        let prior = self.rewrites.remove(&key);
        RewriteUndo { key, prior }
    }

    /// Put back what an `apply_rewrite`/`remove_rewrite` changed, for a frame that never went out.
    pub(crate) fn restore_rewrite(&mut self, undo: RewriteUndo) {
        match undo.prior {
            Some(rule) => {
                self.rewrites.insert(undo.key, rule);
            }
            None => {
                self.rewrites.remove(&undo.key);
            }
        }
    }

    /// Drop every held rewrite rule (the whole-table clear).
    pub(crate) fn clear_rewrites(&mut self) {
        self.rewrites.clear();
    }

    /// Every held rewrite rule, for the reconnect and keepalive re-assertion.
    pub(crate) fn held_rewrites(&self) -> Vec<StoredRewrite> {
        self.rewrites.values().cloned().collect()
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
    }

    /// The catch subscription table the box should be holding (re-asserted on reconnect).
    pub(crate) fn set_catch(&mut self, filters: FilterSet) {
        self.catch = filters;
    }

    pub(crate) fn catch(&self) -> FilterSet {
        self.catch.clone()
    }

    /// Idle = nothing for the keepalive to hold alive; a catch subscription or a rewrite rule counts.
    pub(crate) fn is_idle(&self) -> bool {
        self.catch.is_empty()
            && self.overrides.is_empty()
            && self.locks.is_empty()
            && self.rewrites.is_empty()
    }

    /// Every held momentary override, as `(Usage, Action)`, for the reconnect reapply.
    pub(crate) fn held(&self) -> impl Iterator<Item = (Usage, Action)> + '_ {
        self.overrides.iter().filter_map(|(&(cls, id), ov)| {
            let action = ov.as_action()?;
            let class = Class::from_u8(cls)?;
            Some((Usage::new(class, id), action))
        })
    }

    // The `(key, scale)` commands that rebuild every held row, for the reconnect reapply. The button
    // blanket is expanded here onto the count CAPS last reported, not at apply time, so a reconnect
    // that re-read CAPS re-asserts every declared button. Media rows come out in the order they were
    // taken, so the replay fills the box's slot array the way the live box filled it.
    pub(crate) fn held_locks(&self) -> Vec<(LockKey, u8)> {
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
