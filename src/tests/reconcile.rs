use crate::link::reconcile::{DesiredState, clip_packet_key};
use crate::types::{
    Action, Blanket, Button, ClipAction, ClipPacketTrigger, ClipPacketTriggerEntry, ClipSettings,
    ClipState, ClipStatus, ClipTrigger, Direction, Edge, Key, MediaKey, TrafficClass, Usage,
};

#[test]
fn default_is_idle() {
    let d = DesiredState::default();
    assert!(d.is_idle());
    assert_eq!(d.held().count(), 0);
}

#[test]
fn press_is_held_and_non_idle() {
    let mut d = DesiredState::default();
    d.apply(Button::LEFT.into(), Action::Press);
    assert!(!d.is_idle());
    assert_eq!(
        d.held().collect::<Vec<_>>(),
        vec![(Usage::from(Button::LEFT), Action::Press)]
    );
}

#[test]
fn force_release_is_held() {
    let mut d = DesiredState::default();
    d.apply(Button::RIGHT.into(), Action::ForceRelease);
    assert!(!d.is_idle());
    assert_eq!(
        d.held().collect::<Vec<_>>(),
        vec![(Usage::from(Button::RIGHT), Action::ForceRelease)]
    );
}

#[test]
fn soft_release_clears_the_override() {
    let mut d = DesiredState::default();
    d.apply(Button::MIDDLE.into(), Action::Press);
    d.apply(Button::MIDDLE.into(), Action::SoftRelease);
    assert!(d.is_idle());
}

#[test]
fn clear_resets_all() {
    let mut d = DesiredState::default();
    d.apply(Button::LEFT.into(), Action::Press);
    d.apply(Button::SIDE2.into(), Action::ForceRelease);
    assert!(!d.is_idle());
    d.clear();
    assert!(d.is_idle());
    assert_eq!(d.held().count(), 0);
}

#[test]
fn held_preserves_identity_in_class_then_id_order() {
    let mut d = DesiredState::default();
    d.apply(Button::LEFT.into(), Action::Press);
    d.apply(Button::SIDE1.into(), Action::ForceRelease);
    assert_eq!(
        d.held().collect::<Vec<_>>(),
        vec![
            (Usage::from(Button::LEFT), Action::Press),
            (Usage::from(Button::SIDE1), Action::ForceRelease),
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
    d.apply(Button::LEFT.into(), Action::Press);
    d.apply(Key::A.into(), Action::Press);
    assert_eq!(
        d.held().map(|(u, _)| u).collect::<Vec<_>>(),
        vec![
            Usage::from(Button::LEFT),
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
fn a_button_blanket_widens_to_the_declared_count_when_caps_arrives() {
    let mut d = DesiredState::default();
    d.apply_lock((LOCK_CLS_BTN, LOCK_ID_ALL, LOCK_DIR_BOTH), LOCK_SCALE_BLOCK);
    // Before any CAPS read the blanket expands onto the five named buttons.
    assert_eq!(d.held_locks().len(), 5);
    // The blanket is held unexpanded, so once CAPS reports a wider device the SAME blanket re-expands
    // onto every declared button: a reconnect that re-read CAPS re-asserts the wide buttons instead of
    // the frozen five. Materialising at apply time (the old behaviour) could not do this.
    d.note_declared_buttons(8);
    let ids: Vec<u16> = d.held_locks().iter().map(|&((_, id, _), _)| id).collect();
    assert_eq!(ids, (0..8).collect::<Vec<u16>>());
    // Releasing one button afterwards must not be undone by a replay of the blanket.
    d.apply_lock((LOCK_CLS_BTN, 0, LOCK_DIR_BOTH), LOCK_SCALE_PASS);
    let held = d.held_locks();
    assert_eq!(held.len(), 7);
    assert!(!held.iter().any(|&((_, id, _), _)| id == 0));
}

#[test]
fn a_button_blanket_re_expands_when_the_declared_count_changes() {
    // A device swapped in during a reconnect blip re-reads CAPS, and the held blanket then re-asserts
    // onto the new count, wider or narrower, because it was never materialised at the old one.
    let mut d = DesiredState::default();
    d.note_declared_buttons(8);
    d.apply_lock((LOCK_CLS_BTN, LOCK_ID_ALL, LOCK_DIR_BOTH), LOCK_SCALE_BLOCK);
    assert_eq!(d.held_locks().len(), 8);
    d.note_declared_buttons(12);
    assert_eq!(d.held_locks().len(), 12);
    d.note_declared_buttons(4);
    assert_eq!(d.held_locks().len(), 4);
}

#[test]
fn an_undone_button_release_restores_the_blanket() {
    // The single-button write that bursts the blanket rolls back cleanly when its frame never went out,
    // leaving the blanket as it was rather than the materialised rows the burst created.
    let mut d = DesiredState::default();
    d.note_declared_buttons(8);
    d.apply_lock((LOCK_CLS_BTN, LOCK_ID_ALL, LOCK_DIR_BOTH), LOCK_SCALE_BLOCK);
    let before = d.held_locks();
    let undo = d.apply_lock((LOCK_CLS_BTN, 0, LOCK_DIR_BOTH), LOCK_SCALE_PASS);
    assert_eq!(d.held_locks().len(), 7); // burst to eight, minus the released button
    d.restore_lock(undo);
    assert_eq!(d.held_locks(), before);
}

#[test]
fn a_button_blanket_expands_onto_the_declared_count() {
    // Once CAPS reports a wide button count, the blanket expands onto every declared button, so a
    // reconnect re-asserts a lock on a button past the five named ones.
    let mut d = DesiredState::default();
    d.note_declared_buttons(16);
    d.apply_lock((LOCK_CLS_BTN, LOCK_ID_ALL, LOCK_DIR_BOTH), LOCK_SCALE_BLOCK);
    let ids: Vec<u16> = d.held_locks().iter().map(|&((_, id, _), _)| id).collect();
    assert_eq!(ids, (0..16).collect::<Vec<u16>>());
    assert!(
        d.held_locks()
            .iter()
            .any(|&((cls, id, _), _)| cls == LOCK_CLS_BTN && id == 8)
    );
}

#[test]
fn a_button_blanket_caps_at_the_box_ceiling() {
    // A device declaring more buttons than the box can drive still expands onto only the ceiling.
    let mut d = DesiredState::default();
    d.note_declared_buttons(40);
    d.apply_lock((LOCK_CLS_BTN, LOCK_ID_ALL, LOCK_DIR_BOTH), LOCK_SCALE_BLOCK);
    assert_eq!(d.held_locks().len(), 16);
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

// The box keeps granular media locks in a fixed 8-slot array filled first-free-slot-first
// (input_core.c media_set, INPUT_MEDIA_MAX = 8), so what a replay has to reproduce is the order they
// were taken in, not their ids.
use crate::protocol::opcode::LOCK_CLS_MEDIA;

fn media_ids(d: &DesiredState) -> Vec<u16> {
    d.held_locks()
        .iter()
        .filter(|&&((class, _, _), _)| class == LOCK_CLS_MEDIA)
        .map(|&((_, id, _), _)| id)
        .collect()
}

#[test]
fn media_locks_replay_in_the_order_they_were_taken() {
    let mut d = DesiredState::default();
    // Nine usages in an order id-sorting would not produce: MUTE, VOL_UP, VOL_DOWN come back in that
    // order only if the take order is what is remembered.
    let taken = [0x223u16, 0x30, 0xB5, 0xE9, 0xB6, 0xCD, 0xE2, 0xB7, 0xEA];
    for id in taken {
        d.apply_lock((LOCK_CLS_MEDIA, id, LOCK_DIR_BOTH), LOCK_SCALE_BLOCK);
    }
    // Past the box's eight slots the ninth taken is what falls off, here and on the box alike; by id
    // the ninth would be 0x223 and the box would still be dropping 0xEA.
    assert_eq!(media_ids(&d), taken);
}

#[test]
fn a_released_media_lock_leaves_the_order_of_the_rest() {
    let mut d = DesiredState::default();
    for id in [0xEAu16, 0xE9, 0x30] {
        d.apply_lock((LOCK_CLS_MEDIA, id, LOCK_DIR_BOTH), LOCK_SCALE_BLOCK);
    }
    d.apply_lock((LOCK_CLS_MEDIA, 0xE9, LOCK_DIR_BOTH), LOCK_SCALE_PASS);
    d.apply_lock((LOCK_CLS_MEDIA, 0xB5, LOCK_DIR_BOTH), LOCK_SCALE_BLOCK);
    assert_eq!(media_ids(&d), vec![0xEA, 0x30, 0xB5]);
    // Re-taking one already held keeps its place rather than moving it to the end.
    d.apply_lock((LOCK_CLS_MEDIA, 0xEA, LOCK_DIR_BOTH), LOCK_SCALE_BLOCK);
    assert_eq!(media_ids(&d), vec![0xEA, 0x30, 0xB5]);
}

#[test]
fn a_media_blanket_does_not_take_a_slot() {
    // The blanket is its own flag on the box (lock_all_media), not an entry in the slot array.
    let mut d = DesiredState::default();
    d.apply_lock((LOCK_CLS_MEDIA, 0xEA, LOCK_DIR_BOTH), LOCK_SCALE_BLOCK);
    d.apply_lock(
        (LOCK_CLS_MEDIA, LOCK_ID_ALL, LOCK_DIR_BOTH),
        LOCK_SCALE_BLOCK,
    );
    d.apply_lock((LOCK_CLS_MEDIA, 0x30, LOCK_DIR_BOTH), LOCK_SCALE_BLOCK);
    assert_eq!(media_ids(&d), vec![0xEA, 0x30, LOCK_ID_ALL]);
}

#[test]
fn every_other_class_still_replays_in_id_order() {
    let mut d = DesiredState::default();
    for id in [3u16, 0, 1] {
        d.apply_lock((LOCK_CLS_BTN, id, LOCK_DIR_POS), LOCK_SCALE_BLOCK);
    }
    assert_eq!(
        d.held_locks()
            .iter()
            .map(|&((_, id, _), _)| id)
            .collect::<Vec<_>>(),
        vec![0, 1, 3]
    );
}

#[test]
fn an_undone_apply_leaves_the_state_exactly_as_it_was() {
    let mut d = DesiredState::default();
    d.apply_lock((X.0, X.1, LOCK_DIR_BOTH), 40);
    d.apply_lock((LOCK_CLS_MEDIA, 0xEA, LOCK_DIR_BOTH), LOCK_SCALE_BLOCK);
    let before = (d.held_locks(), d.is_idle());

    let undo = d.apply_lock((X.0, X.1, LOCK_DIR_AGAINST), LOCK_SCALE_BLOCK);
    d.restore_lock(undo);
    assert_eq!((d.held_locks(), d.is_idle()), before);

    // Including the case that took a fresh row: undoing it must leave nothing behind, blanket
    // expansion and media order alike.
    let undo = d.apply_lock((LOCK_CLS_BTN, LOCK_ID_ALL, LOCK_DIR_BOTH), LOCK_SCALE_BLOCK);
    d.restore_lock(undo);
    let undo = d.apply_lock((LOCK_CLS_MEDIA, 0x30, LOCK_DIR_BOTH), LOCK_SCALE_BLOCK);
    d.restore_lock(undo);
    assert_eq!((d.held_locks(), d.is_idle()), before);

    // And the case that cleared a row: undoing an unlock puts the lock back.
    let undo = d.apply_lock((X.0, X.1, LOCK_DIR_BOTH), LOCK_SCALE_PASS);
    d.restore_lock(undo);
    assert_eq!((d.held_locks(), d.is_idle()), before);
}

#[test]
fn a_row_of_another_class_never_disturbs_the_media_order() {
    // The classes share an id space: media usage 3 and button 3 are different rows, and releasing
    // one must not move the other in the replay.
    let mut d = DesiredState::default();
    d.apply_lock((LOCK_CLS_MEDIA, 3, LOCK_DIR_BOTH), LOCK_SCALE_BLOCK);
    d.apply_lock((LOCK_CLS_MEDIA, 0xEA, LOCK_DIR_BOTH), LOCK_SCALE_BLOCK);
    d.apply_lock((LOCK_CLS_BTN, 3, LOCK_DIR_POS), LOCK_SCALE_BLOCK);
    d.apply_lock((LOCK_CLS_BTN, 3, LOCK_DIR_POS), LOCK_SCALE_PASS);
    assert_eq!(media_ids(&d), vec![3, 0xEA]);
}

// --- rewrite rules (§3.14): session state re-asserted like locks/catch ---

use crate::link::reconcile::StoredRewrite;

fn stored(class: u8, id: u16) -> StoredRewrite {
    StoredRewrite {
        class,
        id,
        direction: 0,
        action: 1,
        offset: 0,
        match_bytes: vec![],
        mask: vec![],
        payload: vec![],
    }
}

#[test]
fn rewrite_rule_is_held_and_non_idle() {
    let mut d = DesiredState::default();
    d.apply_rewrite(stored(9, 1));
    assert!(!d.is_idle());
    assert_eq!(d.held_rewrites().len(), 1);
}

#[test]
fn rewrite_overwrite_keeps_one_row() {
    let mut d = DesiredState::default();
    d.apply_rewrite(stored(9, 1));
    let mut r = stored(9, 1);
    r.action = 3; // same key, new action
    d.apply_rewrite(r);
    let held = d.held_rewrites();
    assert_eq!(held.len(), 1);
    assert_eq!(held[0].action, 3);
}

#[test]
fn rewrite_remove_and_restore() {
    let mut d = DesiredState::default();
    d.apply_rewrite(stored(9, 1));
    let key = stored(9, 1).key();
    let undo = d.remove_rewrite(key);
    assert!(d.held_rewrites().is_empty());
    d.restore_rewrite(undo); // a send that never went out is rolled back
    assert_eq!(d.held_rewrites().len(), 1);
}

#[test]
fn rewrite_apply_undo_puts_it_back() {
    let mut d = DesiredState::default();
    let undo = d.apply_rewrite(stored(9, 1));
    d.restore_rewrite(undo);
    assert!(d.held_rewrites().is_empty());
    assert!(d.is_idle());
}

#[test]
fn clear_rewrites_empties_the_table() {
    let mut d = DesiredState::default();
    d.apply_rewrite(stored(9, 1));
    d.apply_rewrite(stored(4, 0));
    d.clear_rewrites();
    assert!(d.held_rewrites().is_empty());
}

#[test]
fn reset_clears_rewrites_too() {
    let mut d = DesiredState::default();
    d.apply_rewrite(stored(9, 1));
    d.clear(); // the RESET path
    assert!(d.held_rewrites().is_empty());
    assert!(d.is_idle());
}

// --- field transforms (§3.15): session state re-asserted like locks/rewrites ---

use crate::link::reconcile::StoredTransform;

fn stored_xf(sclass: u8, sid: u16, dclass: u8, did: u16) -> StoredTransform {
    StoredTransform {
        op: 0, // Remap
        sclass,
        sid,
        dclass,
        did,
    }
}

#[test]
fn transform_is_held_and_non_idle() {
    let mut d = DesiredState::default();
    d.apply_transform(stored_xf(3, 1, 3, 1));
    assert!(!d.is_idle());
    assert_eq!(d.held_transforms().len(), 1);
}

#[test]
fn transform_overwrite_keeps_one_row() {
    let mut d = DesiredState::default();
    d.apply_transform(stored_xf(3, 1, 3, 1));
    let mut r = stored_xf(3, 1, 3, 1);
    r.op = 1; // same key (source, dest), new op
    d.apply_transform(r);
    let held = d.held_transforms();
    assert_eq!(held.len(), 1);
    assert_eq!(held[0].op, 1);
}

#[test]
fn transform_remove_and_restore() {
    let mut d = DesiredState::default();
    d.apply_transform(stored_xf(3, 1, 3, 1));
    let key = stored_xf(3, 1, 3, 1).key();
    let undo = d.remove_transform(key);
    assert!(d.held_transforms().is_empty());
    d.restore_transform(undo); // a send that never went out is rolled back
    assert_eq!(d.held_transforms().len(), 1);
}

#[test]
fn transform_apply_undo_puts_it_back() {
    let mut d = DesiredState::default();
    let undo = d.apply_transform(stored_xf(3, 1, 3, 1));
    d.restore_transform(undo);
    assert!(d.held_transforms().is_empty());
    assert!(d.is_idle());
}

#[test]
fn clear_transforms_empties_the_table() {
    let mut d = DesiredState::default();
    d.apply_transform(stored_xf(3, 0, 3, 0));
    d.apply_transform(stored_xf(3, 1, 3, 1));
    d.clear_transforms();
    assert!(d.held_transforms().is_empty());
}

#[test]
fn reset_clears_transforms_too() {
    let mut d = DesiredState::default();
    d.apply_transform(stored_xf(3, 1, 3, 1));
    d.clear(); // the RESET path
    assert!(d.held_transforms().is_empty());
    assert!(d.is_idle());
}

// What the box holds of a clip is not re-asserted, but a second of silence clears it, so any of it keeps
// the keepalive running.
#[test]
fn a_loaded_clip_a_setting_or_a_trigger_is_not_idle() {
    let mut d = DesiredState::default();
    d.clip_loaded(true);
    assert!(!d.is_idle());
    d.clip_loaded(false);
    assert!(d.is_idle());

    d.clip_setting(2, 1); // retain
    assert!(!d.is_idle());
    d.clip_setting(2, 0);
    assert!(d.is_idle());
    d.clip_setting(9, 1); // an id the box does not know either
    assert!(d.is_idle());

    d.clip_trigger((1, 0x3A, 1), true);
    d.clip_trigger((1, 0x3A, 2), true);
    d.clip_trigger((1, 0x3A, 1), false);
    assert!(!d.is_idle());
    d.clip_triggers_clear();
    assert!(d.is_idle());

    // A packet trigger is held under its whole key: the mask is part of it.
    let key = |mask: u8| (4u8, 2u16, 1u8, vec![0x07], vec![mask]);
    d.clip_packet_bind(key(0xFF), false);
    d.clip_packet_bind(key(0x0F), false);
    d.clip_packet_unbind(&key(0xFF));
    assert!(!d.is_idle());
    d.clip_packet_unbind(&key(0x0F));
    assert!(d.is_idle());

    // One clear drops both kinds from DesiredState.
    d.clip_trigger((1, 0x3A, 1), true);
    d.clip_packet_bind(key(0xFF), false);
    d.clip_triggers_clear();
    assert!(d.is_idle());
    d.clip_packet_bind(key(0xFF), false);
    d.clip_triggers_clear();
    assert!(d.is_idle());
}

// The box removes every consuming packet trigger when the opt-in goes off, and holds the rest.
#[test]
fn the_opt_in_going_off_drops_the_consuming_packet_triggers() {
    let key = |id: u16| (4u8, id, 1u8, vec![0x07], vec![0xFF]);
    let mut d = DesiredState::default();
    d.clip_packet_bind(key(1), true);
    d.clip_packet_bind(key(2), false);
    d.clip_packet_bind(key(3), true);
    assert_eq!(d.clip_packet_drop_consuming(), vec![key(1), key(3)]);
    assert!(!d.is_idle(), "the watching trigger stands");
    assert_eq!(d.clip_packet_drop_consuming(), vec![]);
    d.clip_packet_unbind(&key(2));
    assert!(d.is_idle());

    // A re-bind takes the flags it sent: a consuming trigger re-bound watching stays.
    d.clip_packet_bind(key(1), true);
    d.clip_packet_bind(key(1), false);
    assert_eq!(d.clip_packet_drop_consuming(), vec![]);
    assert!(!d.is_idle());
    // Only consuming triggers: nothing is left to hold.
    d.clip_packet_bind(key(1), true);
    d.clip_packet_drop_consuming();
    assert!(d.is_idle());

    // An adopted trigger carries the consume flag the box read back.
    let adopted = |consume: bool| ClipSettings {
        packet_triggers: vec![ClipPacketTriggerEntry {
            trigger: ClipPacketTrigger {
                consume,
                ..ClipPacketTrigger::new(TrafficClass::HidIn, 2, Direction::IN, ClipAction::Start)
            },
            hits: 0,
        }],
        ..ClipSettings::default()
    };
    for consume in [false, true] {
        let mut d = DesiredState::default();
        d.clip_adopt(&ClipStatus::default(), &adopted(consume));
        assert_eq!(d.clip_packet_drop_consuming().len(), consume as usize);
        assert_eq!(d.is_idle(), consume);
    }
}

#[test]
fn a_reset_forgets_the_clip() {
    let mut d = DesiredState::default();
    d.clip_loaded(true);
    d.clip_setting(1, 1);
    d.clip_trigger((0, 3, 1), true);
    d.clip_packet_bind((9, 1, 1, vec![], vec![]), false);
    d.clear();
    assert!(d.is_idle());
}

// After a reconnect the box's own answer replaces what was recorded: a blip shorter than the silence
// window leaves the clip standing, a longer one clears it.
#[test]
fn a_reconnect_adopts_what_the_box_still_holds_of_a_clip() {
    let empty = ClipStatus::default();
    let plain = ClipSettings::default();
    let mut d = DesiredState::default();
    d.clip_loaded(true);
    d.clip_setting(3, 1);
    d.clip_trigger((0, 3, 1), true);
    d.clip_packet_bind((9, 1, 1, vec![], vec![]), false);
    d.clip_adopt(&empty, &plain);
    assert!(
        d.is_idle(),
        "a box that lost the clip leaves nothing to keep alive"
    );

    let held = |status: &ClipStatus, settings: &ClipSettings| {
        let mut d = DesiredState::default();
        d.clip_adopt(status, settings);
        !d.is_idle()
    };
    assert!(held(
        &ClipStatus {
            total: 40,
            ..empty.clone()
        },
        &plain
    ));
    // A streaming clip that ran dry holds no bytes and is still playing.
    assert!(held(
        &ClipStatus {
            state: ClipState::Playing,
            ..empty.clone()
        },
        &plain
    ));
    assert!(held(
        &empty,
        &ClipSettings {
            autolock: vec![Blanket::Aim],
            ..plain.clone()
        }
    ));
    assert!(held(
        &empty,
        &ClipSettings {
            loop_: true,
            ..plain.clone()
        }
    ));
    assert!(held(
        &empty,
        &ClipSettings {
            retain: true,
            ..plain.clone()
        }
    ));
    assert!(held(
        &empty,
        &ClipSettings {
            ride: true,
            ..plain.clone()
        }
    ));
    // `finalized` is the ring's state, which `total` already covers.
    assert!(!held(
        &empty,
        &ClipSettings {
            finalized: true,
            ..plain.clone()
        }
    ));

    // Each adopted scalar lands on its own `CLIP_SET` id, where the next `set` of it overwrites it.
    for (id, only) in [
        (
            0u8,
            ClipSettings {
                autolock: vec![Blanket::Aim],
                ..plain.clone()
            },
        ),
        (
            1,
            ClipSettings {
                loop_: true,
                ..plain.clone()
            },
        ),
        (
            2,
            ClipSettings {
                retain: true,
                ..plain.clone()
            },
        ),
        (
            3,
            ClipSettings {
                ride: true,
                ..plain.clone()
            },
        ),
    ] {
        let mut d = DesiredState::default();
        d.clip_adopt(&empty, &only);
        for other in (0..4).filter(|&o| o != id) {
            d.clip_setting(other, 0);
        }
        assert!(!d.is_idle(), "setting {id} was adopted onto another id");
        d.clip_setting(id, 0);
        assert!(d.is_idle(), "setting {id}");
    }

    // An adopted trigger is the one `unbind` removes, and an adopted setting the one `set` overwrites.
    let bound = ClipSettings {
        triggers: vec![ClipTrigger::new(
            Button::SIDE1,
            Edge::Press,
            ClipAction::Toggle,
        )],
        ride: true,
        ..plain.clone()
    };
    let mut d = DesiredState::default();
    d.clip_adopt(&empty, &bound);
    d.clip_setting(3, 0);
    assert!(!d.is_idle());
    let (class, id) = Usage::from(Button::SIDE1).class_id();
    d.clip_trigger((class, id, 1), false);
    assert!(d.is_idle());

    // An adopted packet trigger alone holds the keepalive, under the key `unbind_packet` removes.
    let packet = ClipPacketTrigger::new(TrafficClass::HidIn, 2, Direction::IN, ClipAction::Start)
        .matching([0x07, 0x20], [0xFF, 0x20])
        .once_per_run(1);
    let watching = ClipSettings {
        packet_triggers: vec![ClipPacketTriggerEntry {
            trigger: packet.clone(),
            hits: 3,
        }],
        ..plain.clone()
    };
    assert!(held(&empty, &watching));
    let mut d = DesiredState::default();
    d.clip_adopt(&empty, &watching);
    // Every part of the key tells two triggers apart.
    for other in [
        ClipPacketTrigger {
            class: TrafficClass::Emit,
            ..packet.clone()
        },
        ClipPacketTrigger {
            id: 3,
            ..packet.clone()
        },
        ClipPacketTrigger {
            direction: Direction::OUT,
            ..packet.clone()
        },
        packet.clone().matching([0x07, 0x00], [0xFF, 0x20]),
        packet.clone().matching([0x07, 0x20], [0xFF, 0xFF]),
    ] {
        d.clip_packet_unbind(&clip_packet_key(&other));
        assert!(!d.is_idle(), "{other:?}");
    }
    // The verb and the flags are no part of it.
    let same_key = ClipPacketTrigger {
        action: ClipAction::Stop,
        consume: true,
        once_per_run: false,
        selector_len: 0,
        ..packet
    };
    d.clip_packet_unbind(&clip_packet_key(&same_key));
    assert!(d.is_idle());
}
