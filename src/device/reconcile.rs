//! Desired-state tracking + reapply (§8) — the data half of reconcile.
//!
//! [`DesiredState`] records the button overrides the host *intends* the box to hold, the source of
//! truth for two recovery behaviours: keepalive (while non-idle, send a cheap frame sub-1 s to defeat
//! the firmware's 1000 ms silence auto-clear; while idle, send nothing so the safety auto-clear still
//! fires on a real crash) and auto-reapply on reconnect (re-send every held override). Commands update
//! this map as they send: `press`/`force_release` set an override, `soft_release` clears it, `reset`
//! clears all.

use crate::protocol::opcode::BTN_COUNT;
use crate::types::{Button, ButtonAction};

/// One button's intended override, mirroring the firmware's `{NONE, PRESS, FORCE}` (§5.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Override {
    /// No injected override — defer to physical state.
    #[default]
    None,
    /// Force the button down (`press`).
    Press,
    /// Force the button up, masking a physical hold (`force_release`).
    Force,
}

impl Override {
    /// The [`ButtonAction`] re-establishing this override on the box, or `None` if there is nothing to
    /// re-send. `soft-release` is never a held state, so it has no representation here.
    pub(crate) fn as_action(self) -> Option<ButtonAction> {
        match self {
            Override::None => None,
            Override::Press => Some(ButtonAction::Press),
            Override::Force => Some(ButtonAction::ForceRelease),
        }
    }
}

/// The host's intended button overrides: one [`Override`] per standard button (§3.3 ids 0..=4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DesiredState {
    overrides: [Override; BTN_COUNT as usize],
}

impl Default for DesiredState {
    fn default() -> Self {
        DesiredState {
            overrides: [Override::None; BTN_COUNT as usize],
        }
    }
}

impl DesiredState {
    /// Record the effect of a `BUTTON` command: `Press` → down, `ForceRelease` → up (masking physical),
    /// `SoftRelease` → clear our override.
    pub(crate) fn apply(&mut self, button: Button, action: ButtonAction) {
        let slot = &mut self.overrides[button.as_id() as usize];
        *slot = match action {
            ButtonAction::Press => Override::Press,
            ButtonAction::ForceRelease => Override::Force,
            ButtonAction::SoftRelease => Override::None,
        };
    }

    /// Clear every override (the effect of `RESET`, §3.4).
    pub(crate) fn clear(&mut self) {
        self.overrides = [Override::None; BTN_COUNT as usize];
    }

    /// `true` if no override is held — the keepalive stays off so the firmware safety auto-clear stays
    /// intact (§8).
    pub(crate) fn is_idle(&self) -> bool {
        self.overrides.iter().all(|o| *o == Override::None)
    }

    /// The held overrides as `(Button, ButtonAction)` pairs to re-send on reapply/reconnect.
    pub(crate) fn held(&self) -> impl Iterator<Item = (Button, ButtonAction)> + '_ {
        self.overrides.iter().enumerate().filter_map(|(id, ov)| {
            let action = ov.as_action()?;
            let button = Button::from_id(id as u8)?;
            Some((button, action))
        })
    }
}
