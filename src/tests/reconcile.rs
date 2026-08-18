use crate::link::reconcile::DesiredState;
use crate::types::{Action, Button, Key, MediaKey, Usage};

#[test]
fn default_is_idle() {
    let d = DesiredState::default();
    assert!(d.is_idle());
    assert_eq!(d.held().count(), 0);
}

#[test]
fn press_is_held_and_non_idle() {
    let mut d = DesiredState::default();
    d.apply(Button::Left.into(), Action::Press);
    assert!(!d.is_idle());
    assert_eq!(
        d.held().collect::<Vec<_>>(),
        vec![(Usage::from(Button::Left), Action::Press)]
    );
}

#[test]
fn force_release_is_held() {
    let mut d = DesiredState::default();
    d.apply(Button::Right.into(), Action::ForceRelease);
    assert!(!d.is_idle());
    assert_eq!(
        d.held().collect::<Vec<_>>(),
        vec![(Usage::from(Button::Right), Action::ForceRelease)]
    );
}

#[test]
fn soft_release_clears_the_override() {
    let mut d = DesiredState::default();
    d.apply(Button::Middle.into(), Action::Press);
    d.apply(Button::Middle.into(), Action::SoftRelease);
    assert!(d.is_idle());
}

#[test]
fn clear_resets_all() {
    let mut d = DesiredState::default();
    d.apply(Button::Left.into(), Action::Press);
    d.apply(Button::Side2.into(), Action::ForceRelease);
    assert!(!d.is_idle());
    d.clear();
    assert!(d.is_idle());
    assert_eq!(d.held().count(), 0);
}

#[test]
fn held_preserves_identity_in_class_then_id_order() {
    let mut d = DesiredState::default();
    d.apply(Button::Left.into(), Action::Press);
    d.apply(Button::Side1.into(), Action::ForceRelease);
    assert_eq!(
        d.held().collect::<Vec<_>>(),
        vec![
            (Usage::from(Button::Left), Action::Press),
            (Usage::from(Button::Side1), Action::ForceRelease),
        ]
    );
}

#[test]
fn key_press_is_held_and_non_idle() {
    let mut d = DesiredState::default();
    d.apply(Key::A.into(), Action::Press);
    assert!(!d.is_idle());
    assert_eq!(
        d.held().collect::<Vec<_>>(),
        vec![(Usage::from(Key::A), Action::Press)]
    );
}

#[test]
fn key_soft_release_clears_the_override() {
    let mut d = DesiredState::default();
    d.apply(Key::LEFT_SHIFT.into(), Action::Press);
    d.apply(Key::LEFT_SHIFT.into(), Action::SoftRelease);
    assert!(d.is_idle());
    assert_eq!(d.held().count(), 0);
}

#[test]
fn media_press_is_held() {
    let mut d = DesiredState::default();
    d.apply(MediaKey::VOLUME_UP.into(), Action::Press);
    assert!(!d.is_idle());
    assert_eq!(
        d.held().collect::<Vec<_>>(),
        vec![(Usage::from(MediaKey::VOLUME_UP), Action::Press)]
    );
}

#[test]
fn media_soft_release_clears_the_override() {
    let mut d = DesiredState::default();
    d.apply(MediaKey::MUTE.into(), Action::Press);
    d.apply(MediaKey::MUTE.into(), Action::SoftRelease);
    assert!(d.is_idle());
}

#[test]
fn one_store_holds_every_class_and_orders_by_class_then_id() {
    let mut d = DesiredState::default();
    d.apply(MediaKey::VOLUME_UP.into(), Action::Press);
    d.apply(Button::Left.into(), Action::Press);
    d.apply(Key::A.into(), Action::Press);
    assert_eq!(
        d.held().map(|(u, _)| u).collect::<Vec<_>>(),
        vec![
            Usage::from(Button::Left),
            Usage::from(Key::A),
            Usage::from(MediaKey::VOLUME_UP),
        ]
    );
    d.clear();
    assert!(d.is_idle());
    assert_eq!(d.held().count(), 0);
}

#[test]
fn idle_requires_every_class_empty() {
    let mut d = DesiredState::default();
    d.apply(Key::ESCAPE.into(), Action::Press);
    assert!(!d.is_idle());
    d.apply(Key::ESCAPE.into(), Action::SoftRelease);
    assert!(d.is_idle());
    d.apply(MediaKey::PLAY_PAUSE.into(), Action::ForceRelease);
    assert!(!d.is_idle());
}

// The lock half. The keys are wire triples (class, id, direction), the shape a reapply re-sends.
use crate::protocol::opcode::{
    LOCK_AXIS_X, LOCK_CLS_AXIS, LOCK_CLS_BTN, LOCK_CLS_KEY, LOCK_DIR_AGAINST, LOCK_DIR_BOTH,
    LOCK_DIR_NEG, LOCK_DIR_POS, LOCK_DIR_WITH, LOCK_ID_ALL, LOCK_SCALE_BLOCK, LOCK_SCALE_PASS,
};

const X: (u8, u16) = (LOCK_CLS_AXIS, LOCK_AXIS_X);

#[test]
fn releasing_one_sign_of_a_both_lock_leaves_the_other() {
    let mut d = DesiredState::default();
    d.apply_lock((X.0, X.1, LOCK_DIR_BOTH), LOCK_SCALE_BLOCK);
    d.apply_lock((X.0, X.1, LOCK_DIR_NEG), LOCK_SCALE_PASS);
    assert_eq!(
        d.held_locks(),
        vec![((X.0, X.1, LOCK_DIR_POS), LOCK_SCALE_BLOCK)]
    );
    d.apply_lock((X.0, X.1, LOCK_DIR_POS), LOCK_SCALE_PASS);
    assert!(d.is_idle());
}

#[test]
fn a_both_write_replaces_the_single_directions_before_it() {
    let mut d = DesiredState::default();
    d.apply_lock((X.0, X.1, LOCK_DIR_NEG), 40);
    d.apply_lock((X.0, X.1, LOCK_DIR_AGAINST), 30);
    d.apply_lock((X.0, X.1, LOCK_DIR_BOTH), 50);
    // Both is the whole row: the fixed pair takes the scale and the relative pair goes back to
    // passing, so one command rebuilds it.
    assert_eq!(d.held_locks(), vec![((X.0, X.1, LOCK_DIR_BOTH), 50)]);
}

#[test]
fn a_relative_scale_survives_an_absolute_one() {
    let mut d = DesiredState::default();
    d.apply_lock((X.0, X.1, LOCK_DIR_AGAINST), 40);
    d.apply_lock((X.0, X.1, LOCK_DIR_POS), 60);
    assert_eq!(
        d.held_locks(),
        vec![
            ((X.0, X.1, LOCK_DIR_POS), 60),
            ((X.0, X.1, LOCK_DIR_AGAINST), 40),
        ]
    );
}

#[test]
fn a_full_unlock_clears_every_slot() {
    let mut d = DesiredState::default();
    d.apply_lock((X.0, X.1, LOCK_DIR_BOTH), LOCK_SCALE_BLOCK);
    d.apply_lock((X.0, X.1, LOCK_DIR_WITH), 30);
    assert!(!d.is_idle());
    d.apply_lock((X.0, X.1, LOCK_DIR_BOTH), LOCK_SCALE_PASS);
    assert!(d.is_idle());
    assert_eq!(d.held_locks(), vec![]);
}

#[test]
fn a_one_bit_class_holds_what_the_box_will_hold() {
    let mut d = DesiredState::default();
    // 150% on a button is an unlock on the box: it truncates to a pass. Held as 150 the keepalive
    // would stay open for a lock that does not exist.
    d.apply_lock((LOCK_CLS_BTN, 0, LOCK_DIR_POS), 150);
    assert!(d.is_idle());
    d.apply_lock((LOCK_CLS_BTN, 0, LOCK_DIR_POS), 50);
    assert_eq!(
        d.held_locks(),
        vec![((LOCK_CLS_BTN, 0, LOCK_DIR_POS), LOCK_SCALE_BLOCK)]
    );
    // An axis keeps the number itself.
    d.apply_lock((X.0, X.1, LOCK_DIR_POS), 150);
    assert!(d.held_locks().contains(&((X.0, X.1, LOCK_DIR_POS), 150)));
}

#[test]
fn a_button_blanket_expands_the_way_the_box_expands_it() {
    let mut d = DesiredState::default();
    d.apply_lock((LOCK_CLS_BTN, LOCK_ID_ALL, LOCK_DIR_BOTH), LOCK_SCALE_BLOCK);
    assert_eq!(d.held_locks().len(), 5);
    // Releasing one button afterwards must not be undone by a replay of the blanket.
    d.apply_lock((LOCK_CLS_BTN, 0, LOCK_DIR_BOTH), LOCK_SCALE_PASS);
    let held = d.held_locks();
    assert_eq!(held.len(), 4);
    assert!(!held.iter().any(|&((_, id, _), _)| id == 0));
}

#[test]
fn a_key_blanket_is_its_own_row() {
    // The box holds a key blanket as its own flag rather than expanding it over 256 usages, so it
    // stays one row here and reapplies as one command.
    let mut d = DesiredState::default();
    d.apply_lock((LOCK_CLS_KEY, LOCK_ID_ALL, LOCK_DIR_POS), LOCK_SCALE_BLOCK);
    assert_eq!(
        d.held_locks(),
        vec![((LOCK_CLS_KEY, LOCK_ID_ALL, LOCK_DIR_POS), LOCK_SCALE_BLOCK)]
    );
}
