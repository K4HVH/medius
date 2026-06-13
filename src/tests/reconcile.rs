//! `DesiredState` invariants — the held-override map the keepalive and reconnect-reapply act on.
//! Internal unit tests (no feature needed): press/force-release are held, soft-release clears, and
//! `held()` preserves button identity in order.

use crate::device::reconcile::DesiredState;
use crate::types::{Button, ButtonAction};

#[test]
fn default_is_idle() {
    let d = DesiredState::default();
    assert!(d.is_idle());
    assert_eq!(d.held().count(), 0);
}

#[test]
fn press_is_held_and_non_idle() {
    let mut d = DesiredState::default();
    d.apply(Button::Left, ButtonAction::Press);
    assert!(!d.is_idle());
    assert_eq!(
        d.held().collect::<Vec<_>>(),
        vec![(Button::Left, ButtonAction::Press)]
    );
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
fn soft_release_clears_the_override() {
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
    assert_eq!(
        d.held().collect::<Vec<_>>(),
        vec![
            (Button::Left, ButtonAction::Press),
            (Button::Side1, ButtonAction::ForceRelease),
        ]
    );
}
