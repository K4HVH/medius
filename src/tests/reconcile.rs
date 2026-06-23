use crate::link::reconcile::DesiredState;
use crate::types::{Action, Button, Key, MediaKey};

#[test]
fn default_is_idle() {
    let d = DesiredState::default();
    assert!(d.is_idle());
    assert_eq!(d.held().count(), 0);
}

#[test]
fn press_is_held_and_non_idle() {
    let mut d = DesiredState::default();
    d.apply(Button::Left, Action::Press);
    assert!(!d.is_idle());
    assert_eq!(
        d.held().collect::<Vec<_>>(),
        vec![(Button::Left, Action::Press)]
    );
}

#[test]
fn force_release_is_held() {
    let mut d = DesiredState::default();
    d.apply(Button::Right, Action::ForceRelease);
    assert!(!d.is_idle());
    assert_eq!(
        d.held().collect::<Vec<_>>(),
        vec![(Button::Right, Action::ForceRelease)]
    );
}

#[test]
fn soft_release_clears_the_override() {
    let mut d = DesiredState::default();
    d.apply(Button::Middle, Action::Press);
    d.apply(Button::Middle, Action::SoftRelease);
    assert!(d.is_idle());
}

#[test]
fn clear_resets_all() {
    let mut d = DesiredState::default();
    d.apply(Button::Left, Action::Press);
    d.apply(Button::Side2, Action::ForceRelease);
    assert!(!d.is_idle());
    d.clear();
    assert!(d.is_idle());
    assert_eq!(d.held().count(), 0);
}

#[test]
fn held_preserves_button_identity_in_order() {
    let mut d = DesiredState::default();
    d.apply(Button::Left, Action::Press);
    d.apply(Button::Side1, Action::ForceRelease);
    assert_eq!(
        d.held().collect::<Vec<_>>(),
        vec![
            (Button::Left, Action::Press),
            (Button::Side1, Action::ForceRelease),
        ]
    );
}

#[test]
fn key_press_is_held_and_non_idle() {
    let mut d = DesiredState::default();
    d.apply_key(Key::A, Action::Press);
    assert!(!d.is_idle());
    assert_eq!(
        d.held_keys().collect::<Vec<_>>(),
        vec![(Key::A, Action::Press)]
    );
}

#[test]
fn key_soft_release_clears_the_override() {
    let mut d = DesiredState::default();
    d.apply_key(Key::LEFT_SHIFT, Action::Press);
    d.apply_key(Key::LEFT_SHIFT, Action::SoftRelease);
    assert!(d.is_idle());
    assert_eq!(d.held_keys().count(), 0);
}

#[test]
fn held_keys_preserved_in_usage_order() {
    let mut d = DesiredState::default();
    d.apply_key(Key::ENTER, Action::Press); // 0x28
    d.apply_key(Key::A, Action::ForceRelease); // 0x04, orders first
    assert_eq!(
        d.held_keys().collect::<Vec<_>>(),
        vec![(Key::A, Action::ForceRelease), (Key::ENTER, Action::Press),]
    );
}

#[test]
fn media_press_is_held() {
    let mut d = DesiredState::default();
    d.apply_media(MediaKey::VOLUME_UP, Action::Press);
    assert!(!d.is_idle());
    assert_eq!(
        d.held_media().collect::<Vec<_>>(),
        vec![(MediaKey::VOLUME_UP, Action::Press)]
    );
}

#[test]
fn media_soft_release_clears_the_override() {
    let mut d = DesiredState::default();
    d.apply_media(MediaKey::MUTE, Action::Press);
    d.apply_media(MediaKey::MUTE, Action::SoftRelease);
    assert!(d.is_idle());
}

#[test]
fn clear_resets_buttons_keys_and_media_together() {
    // The reconnect-reapply path can hold all three classes at once; clear must drop every one.
    let mut d = DesiredState::default();
    d.apply(Button::Left, Action::Press);
    d.apply_key(Key::A, Action::Press);
    d.apply_media(MediaKey::VOLUME_UP, Action::Press);
    assert!(!d.is_idle());
    assert_eq!(d.held().count(), 1);
    assert_eq!(d.held_keys().count(), 1);
    assert_eq!(d.held_media().count(), 1);
    d.clear();
    assert!(d.is_idle());
    assert_eq!(d.held_keys().count() + d.held_media().count(), 0);
}

#[test]
fn idle_requires_every_class_empty() {
    // A held key alone keeps the state non-idle so reconnect reapplies it (not just buttons).
    let mut d = DesiredState::default();
    d.apply_key(Key::ESCAPE, Action::Press);
    assert!(!d.is_idle());
    d.apply_key(Key::ESCAPE, Action::SoftRelease);
    assert!(d.is_idle());
    d.apply_media(MediaKey::PLAY_PAUSE, Action::ForceRelease);
    assert!(!d.is_idle());
}
