use std::collections::{BTreeMap, BTreeSet};

use crate::protocol::opcode::BTN_COUNT;
use crate::types::{Action, Button, CatchMask, Key, MediaKey};

/// A lock the host wants held, keyed by its wire fields so a reapply is exact and idempotent.
pub(crate) type LockKey = (u8, u16, u8); // (class, usage, direction)

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

    /// Apply a tri-state action to an override slot; returns the new state.
    fn applied(action: Action) -> Override {
        match action {
            Action::Press => Override::Press,
            Action::ForceRelease => Override::Force,
            Action::SoftRelease => Override::None,
        }
    }
}

/// The PC-owned injection + subscription state, re-asserted after a reconnect so a held button/key/
/// media key and an open catch stream survive a control-link blip. Buttons use a fixed slot array;
/// keys and media (sparse) use ordered maps so a reapply is deterministic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DesiredState {
    overrides: [Override; BTN_COUNT as usize],
    keys: BTreeMap<u8, Override>, // usage -> Press/Force (a key never sits at None in the map)
    media: BTreeMap<u16, Override>, // Consumer usage -> Press/Force
    locks: BTreeSet<LockKey>,     // active locks (any class), re-asserted after a reconnect
    catch: CatchMask,
}

impl Default for DesiredState {
    fn default() -> Self {
        DesiredState {
            overrides: [Override::None; BTN_COUNT as usize],
            keys: BTreeMap::new(),
            media: BTreeMap::new(),
            locks: BTreeSet::new(),
            catch: CatchMask::empty(),
        }
    }
}

fn apply_to_map<K: Ord>(map: &mut BTreeMap<K, Override>, key: K, action: Action) {
    match Override::applied(action) {
        Override::None => {
            map.remove(&key);
        }
        ov => {
            map.insert(key, ov);
        }
    }
}

impl DesiredState {
    pub(crate) fn apply(&mut self, button: Button, action: Action) {
        self.overrides[button.as_id() as usize] = Override::applied(action);
    }

    pub(crate) fn apply_key(&mut self, key: Key, action: Action) {
        apply_to_map(&mut self.keys, key.usage(), action);
    }

    pub(crate) fn apply_media(&mut self, key: MediaKey, action: Action) {
        apply_to_map(&mut self.media, key.usage(), action);
    }

    /// Track a lock (any class) so a reconnect re-asserts it. `on=false` forgets it.
    pub(crate) fn apply_lock(&mut self, key: LockKey, on: bool) {
        if on {
            self.locks.insert(key);
        } else {
            self.locks.remove(&key);
        }
    }

    pub(crate) fn clear(&mut self) {
        // Injection overrides + locks. Catch teardown on reset() is handled by Link::catch_disconnect_all
        // (it drops the EventStream senders so recv() returns Err — a plain field-clear here couldn't);
        // catch otherwise clears firmware-side on the same lifecycle as injection.
        self.overrides = [Override::None; BTN_COUNT as usize];
        self.keys.clear();
        self.media.clear();
        self.locks.clear();
    }

    /// The catch subscription mask the box should be streaming (re-asserted on reconnect).
    pub(crate) fn set_catch(&mut self, mask: CatchMask) {
        self.catch = mask;
    }

    pub(crate) fn catch(&self) -> CatchMask {
        self.catch
    }

    /// Idle = nothing for the keepalive to hold alive. A catch subscription counts, so the silence
    /// timer keeps being fed and the box keeps streaming while a stream is open.
    pub(crate) fn is_idle(&self) -> bool {
        self.catch.is_empty()
            && self.keys.is_empty()
            && self.media.is_empty()
            && self.locks.is_empty()
            && self.overrides.iter().all(|o| *o == Override::None)
    }

    pub(crate) fn held(&self) -> impl Iterator<Item = (Button, Action)> + '_ {
        self.overrides.iter().enumerate().filter_map(|(id, ov)| {
            let action = ov.as_action()?;
            let button = Button::from_id(id as u8)?;
            Some((button, action))
        })
    }

    pub(crate) fn held_keys(&self) -> impl Iterator<Item = (Key, Action)> + '_ {
        self.keys
            .iter()
            .filter_map(|(&usage, ov)| Some((Key::new(usage), ov.as_action()?)))
    }

    pub(crate) fn held_media(&self) -> impl Iterator<Item = (MediaKey, Action)> + '_ {
        self.media
            .iter()
            .filter_map(|(&usage, ov)| Some((MediaKey::new(usage), ov.as_action()?)))
    }

    pub(crate) fn held_locks(&self) -> impl Iterator<Item = LockKey> + '_ {
        self.locks.iter().copied()
    }
}
