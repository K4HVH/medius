//! Desired-state tracking + reapply (§8) — the data half of reconcile.
//!
//! [`DesiredState`] is the host's record of the button overrides it *intends* the box to hold. It is
//! the single source of truth for two recovery behaviours wired up in Task 3.6:
//!
//! - **Keepalive** — while the state is non-idle (any held `press`/`force`), a cheap frame is sent
//!   sub-1 s to defeat the firmware's 1000 ms silence auto-clear of intentionally held state; when
//!   idle, nothing is sent, so the firmware safety auto-clear still fires on a real host crash.
//! - **Auto-reapply on reconnect** — after a VID/PID rescan reconnect, every non-`None` override is
//!   re-sent so the box returns to the intended state.
//!
//! Commands (Task 3.3) update this map as they send: `press`/`force_release` set an override,
//! `soft_release` clears it, `reset` clears all.

use crate::protocol::opcode::BTN_COUNT;
use crate::protocol::types::{Button, ButtonAction};

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
    /// The [`ButtonAction`] that re-establishes this override on the box, or `None` if there is
    /// nothing to re-send (the override is `None`). `soft-release` is never a *held* state, so it is
    /// not represented here — it clears an override rather than holding one.
    #[cfg_attr(not(test), allow(dead_code))] // used by `held` (reapply/keepalive, Task 3.6)
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
    /// Record the effect of a `BUTTON` command on the intended state.
    ///
    /// - `Press` → hold the button down.
    /// - `ForceRelease` → hold the button up (masking physical).
    /// - `SoftRelease` → clear our override (back to `None`).
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

    /// `true` if no override is held — the keepalive stays *off* in this state so the firmware safety
    /// auto-clear remains intact (§8).
    #[cfg_attr(not(test), allow(dead_code))] // gates the keepalive (Task 3.6)
    pub(crate) fn is_idle(&self) -> bool {
        self.overrides.iter().all(|o| *o == Override::None)
    }

    /// The held overrides as `(Button, ButtonAction)` pairs to re-send on reapply/reconnect. Skips
    /// `None` slots.
    #[cfg_attr(not(test), allow(dead_code))] // drives reapply/reconnect (Task 3.6)
    pub(crate) fn held(&self) -> impl Iterator<Item = (Button, ButtonAction)> + '_ {
        self.overrides.iter().enumerate().filter_map(|(id, ov)| {
            let action = ov.as_action()?;
            let button = Button::from_id(id as u8)?;
            Some((button, action))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_idle() {
        let d = DesiredState::default();
        assert!(d.is_idle());
        assert_eq!(d.held().count(), 0);
    }

    #[test]
    fn press_then_held_and_non_idle() {
        let mut d = DesiredState::default();
        d.apply(Button::Left, ButtonAction::Press);
        assert!(!d.is_idle());
        let held: Vec<_> = d.held().collect();
        assert_eq!(held, vec![(Button::Left, ButtonAction::Press)]);
    }

    #[test]
    fn force_release_is_held() {
        let mut d = DesiredState::default();
        d.apply(Button::Right, ButtonAction::ForceRelease);
        assert!(!d.is_idle());
        assert_eq!(
            d.held().collect::<Vec<_>>(),
            vec![(Button::Right, ButtonAction::ForceRelease)]
        );
    }

    #[test]
    fn soft_release_clears_override() {
        let mut d = DesiredState::default();
        d.apply(Button::Middle, ButtonAction::Press);
        d.apply(Button::Middle, ButtonAction::SoftRelease);
        assert!(d.is_idle());
    }

    #[test]
    fn clear_resets_all() {
        let mut d = DesiredState::default();
        d.apply(Button::Left, ButtonAction::Press);
        d.apply(Button::Side2, ButtonAction::ForceRelease);
        assert!(!d.is_idle());
        d.clear();
        assert!(d.is_idle());
        assert_eq!(d.held().count(), 0);
    }

    #[test]
    fn held_preserves_button_identity_in_order() {
        let mut d = DesiredState::default();
        d.apply(Button::Left, ButtonAction::Press);
        d.apply(Button::Side1, ButtonAction::ForceRelease);
        let held: Vec<_> = d.held().collect();
        assert_eq!(
            held,
            vec![
                (Button::Left, ButtonAction::Press),
                (Button::Side1, ButtonAction::ForceRelease),
            ]
        );
    }
}
