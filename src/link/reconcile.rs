use std::collections::BTreeMap;

use crate::link::catch::FilterSet;
use crate::protocol::opcode::{
    LOCK_CLS_AXIS, LOCK_CLS_BTN, LOCK_CLS_MEDIA, LOCK_DIR_AGAINST, LOCK_DIR_BOTH, LOCK_DIR_NEG,
    LOCK_DIR_POS, LOCK_DIR_WITH, LOCK_ID_ALL, LOCK_SCALE_BLOCK, LOCK_SCALE_PASS,
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
// than the direction byte that was sent is what makes a release exact -- Both writes the absolute
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

    /// Track a scale (any class) so a reconnect re-asserts it, as the box's own table would hold it.
    ///
    /// A momentary usage carries one bit, so the box stores the block or pass it will render and the
    /// number sent is truncated to that; recording the raw byte would leave a scale above a full pass
    /// held here as a lock the box released. A button blanket expands the way the box expands it,
    /// onto the five button rows, so releasing one button afterwards is not undone by the replay.
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
        for id in expand_blanket(class, id) {
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
        undo
    }

    /// Put back what an `apply_lock` wrote, for a frame that never reached the transport.
    pub(crate) fn restore_lock(&mut self, undo: LockUndo) {
        for (key, row) in undo.rows {
            match row {
                Some(row) => self.locks.insert(key, row),
                None => self.locks.remove(&key),
            };
        }
        self.media_order = undo.media_order;
    }

    pub(crate) fn clear(&mut self) {
        // Catch teardown is handled by Link::catch_disconnect_all (drops the EventStream senders); catch
        // otherwise clears firmware-side on the same lifecycle as injection.
        self.overrides.clear();
        self.locks.clear();
        self.media_order.clear();
    }

    /// The catch subscription table the box should be holding (re-asserted on reconnect).
    pub(crate) fn set_catch(&mut self, filters: FilterSet) {
        self.catch = filters;
    }

    pub(crate) fn catch(&self) -> FilterSet {
        self.catch.clone()
    }

    /// Idle = nothing for the keepalive to hold alive; a catch subscription counts.
    pub(crate) fn is_idle(&self) -> bool {
        self.catch.is_empty() && self.overrides.is_empty() && self.locks.is_empty()
    }

    /// Every held momentary override, as `(Usage, Action)`, for the reconnect reapply.
    pub(crate) fn held(&self) -> impl Iterator<Item = (Usage, Action)> + '_ {
        self.overrides.iter().filter_map(|(&(cls, id), ov)| {
            let action = ov.as_action()?;
            let class = Class::from_u8(cls)?;
            Some((Usage::new(class, id), action))
        })
    }

    /// The `(key, scale)` commands that rebuild every held row, for the reconnect reapply. Media rows
    /// come out in the order they were taken, so the replay fills the box's slot array the way the
    /// live box filled it.
    pub(crate) fn held_locks(&self) -> Vec<(LockKey, u8)> {
        let mut rows: Vec<(&(u8, u16), &Slots)> = self.locks.iter().collect();
        rows.sort_by_key(|((class, id), _)| (*class, self.media_rank(*class, *id), *id));
        rows.into_iter()
            .flat_map(|(&(class, id), row)| {
                row.commands()
                    .into_iter()
                    .map(move |(dir, scale)| ((class, id, dir), scale))
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

// The box has no button-blanket state: it writes the five button rows and forgets it was ever one
// command. A key or media blanket is its own flag on the box, so it stays its own row here.
fn expand_blanket(class: u8, id: u16) -> Vec<u16> {
    if class == LOCK_CLS_BTN && id == LOCK_ID_ALL {
        (0..crate::protocol::opcode::BTN_COUNT as u16).collect()
    } else {
        vec![id]
    }
}
