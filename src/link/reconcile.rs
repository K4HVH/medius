use std::collections::{BTreeMap, BTreeSet};

use crate::types::{Action, CatchFilter, Class, Usage};

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

/// PC-owned injection + subscription state, re-asserted after a reconnect so held usages and open catches survive a control-link blip.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct DesiredState {
    overrides: BTreeMap<(u8, u16), Override>, // never sits at None in the map
    locks: BTreeSet<LockKey>,
    catch: BTreeSet<CatchFilter>,
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

    /// Track a lock (any class) so a reconnect re-asserts it. `on=false` forgets it.
    pub(crate) fn apply_lock(&mut self, key: LockKey, on: bool) {
        if on {
            self.locks.insert(key);
        } else {
            self.locks.remove(&key);
        }
    }

    pub(crate) fn clear(&mut self) {
        // Catch teardown is handled by Link::catch_disconnect_all (drops the EventStream senders); catch
        // otherwise clears firmware-side on the same lifecycle as injection.
        self.overrides.clear();
        self.locks.clear();
    }

    /// The catch subscription table the box should be holding (re-asserted on reconnect).
    pub(crate) fn set_catch(&mut self, filters: BTreeSet<CatchFilter>) {
        self.catch = filters;
    }

    pub(crate) fn catch(&self) -> BTreeSet<CatchFilter> {
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

    pub(crate) fn held_locks(&self) -> impl Iterator<Item = LockKey> + '_ {
        self.locks.iter().copied()
    }
}
