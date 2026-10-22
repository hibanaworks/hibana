use super::{EventCursor, EventCursorState};
use crate::{g, global::compiled::images::RoleDescriptorRef};
use core::mem::MaybeUninit;

fn check_fresh_parallel_entry(candidate: u8) {
    let global = g::par(
        g::seq(
            g::send::<0, 1, g::Msg<1, ()>>(),
            g::seq(
                g::send::<0, 1, g::Msg<2, ()>>(),
                g::send::<1, 0, g::Msg<3, ()>>(),
            )
            .roll(),
        ),
        g::send::<0, 2, g::Msg<4, ()>>(),
    )
    .roll();
    let projected = crate::runtime::program::project::<0>(&global);
    let descriptor = RoleDescriptorRef::from_resident(projected.role_image_ref());
    let mut state = MaybeUninit::<EventCursorState>::uninit();
    let mut cursor = MaybeUninit::<EventCursor>::uninit();
    let mut heads = [0u16; 2];
    let mut labels = [0u16; 2];
    // Four events and two retained scopes use one word; the two source rolls
    // each need two epoch words because this role can borrow their parent par.
    let mut completed = [0u32; 5];
    assert_eq!(
        projected
            .role_image_ref()
            .footprint()
            .completion_bit_count(),
        160
    );
    // SAFETY: the asserted descriptor footprint fits the disjoint stationary
    // arrays, and no cursor or reference escapes their lifetime.
    unsafe {
        EventCursor::init_from_compiled(
            cursor.as_mut_ptr(),
            state.as_mut_ptr(),
            heads.as_mut_ptr(),
            labels.as_mut_ptr(),
            completed.as_mut_ptr(),
            descriptor,
        );
    }
    // SAFETY: the production initializer wrote every field above.
    let cursor = unsafe { cursor.assume_init_mut() };
    assert_eq!(cursor.local_steps_len(), 4);
    let first = descriptor.local_event_rows().roll_scope_row(0).unwrap();
    let second = descriptor.local_event_rows().roll_scope_row(1).unwrap();
    let (parent, child) = if first.event_row().len() == 4 {
        (first.scope(), second.scope())
    } else {
        (second.scope(), first.scope())
    };
    for index in 0..4 {
        cursor.mark_local_event_done(index);
    }
    assert!(cursor.borrowed_roll_reentry_is_current(parent, 0, &mut |_| None));
    let (index, scope, permitted) = match candidate % 3 {
        0 => (1, parent, false),
        1 => (0, parent, true),
        2 => (3, parent, true),
        _ => unreachable!(),
    };
    let mut event: crate::global::typestate::EventCommitMeta =
        cursor.try_send_meta_at(index).unwrap().into();
    event.body_reentry = scope;
    assert_eq!(
        cursor.event_enabled(index, event, &mut |_, _| None).is_ok(),
        permitted
    );
    // Admission and borrow construction have not reset the committed visit.
    assert_eq!(cursor.completion_mutations(), 4);
    assert_eq!(cursor.roll_reentry_epoch(parent), 0);
    cursor.clear_reentry_scope_events(parent);
    assert_eq!(cursor.roll_reentry_epoch(parent), 1);
    assert_eq!(cursor.roll_reentry_epoch(child), 1);
    assert!(!cursor.borrowed_roll_reentry_is_current(parent, 0, &mut |_| None));
}

#[cfg(all(test, hibana_repo_tests))]
#[test]
fn a_parallel_entry_uses_the_same_fresh_scope_for_admission_and_commit() {
    for candidate in 0..3 {
        check_fresh_parallel_entry(candidate);
    }
}

#[cfg(kani)]
#[kani::proof]
#[kani::unwind(16)]
fn parallel_entry_rejects_a_completed_childs_prefix_for_the_new_parent() {
    check_fresh_parallel_entry(0);
}

#[cfg(kani)]
#[kani::proof]
#[kani::unwind(16)]
fn parallel_entry_accepts_the_new_parents_required_prefix() {
    check_fresh_parallel_entry(1);
}

#[cfg(kani)]
#[kani::proof]
#[kani::unwind(16)]
fn parallel_entry_accepts_the_independent_parent_arm() {
    check_fresh_parallel_entry(2);
}

fn check_child_after_same_lane_tail() {
    let global = g::seq(
        g::send::<0, 1, g::Msg<1, ()>>(),
        g::seq(
            g::seq(
                g::send::<0, 1, g::Msg<2, ()>>(),
                g::send::<1, 0, g::Msg<3, ()>>(),
            )
            .roll(),
            g::send::<0, 1, g::Msg<4, ()>>(),
        ),
    )
    .roll();
    let projected = crate::runtime::program::project::<0>(&global);
    let descriptor = RoleDescriptorRef::from_resident(projected.role_image_ref());
    let mut state = MaybeUninit::<EventCursorState>::uninit();
    let mut cursor = MaybeUninit::<EventCursor>::uninit();
    let mut heads = [0u16; 1];
    let mut labels = [0u16; 1];
    let mut completed = [0u32; 1];
    assert_eq!(descriptor.logical_lane_count(), heads.len());
    assert_eq!(
        projected
            .role_image_ref()
            .footprint()
            .completion_bit_count()
            .div_ceil(32),
        completed.len()
    );
    // SAFETY: the checked descriptor fits these disjoint stationary arrays.
    // They outlive the cursor and every reference made from it.
    unsafe {
        EventCursor::init_from_compiled(
            cursor.as_mut_ptr(),
            state.as_mut_ptr(),
            heads.as_mut_ptr(),
            labels.as_mut_ptr(),
            completed.as_mut_ptr(),
            descriptor,
        );
    }
    // SAFETY: the production initializer wrote every field above.
    let cursor = unsafe { cursor.assume_init_mut() };
    let first = descriptor.local_event_rows().roll_scope_row(0).unwrap();
    let second = descriptor.local_event_rows().roll_scope_row(1).unwrap();
    let (child, parent) = if first.event_row().len() == 2 {
        (first.scope(), second.scope())
    } else {
        (second.scope(), first.scope())
    };
    for index in 0..4 {
        cursor.mark_local_event_done(index);
    }
    let target = cursor
        .relocatable_resident_lane_step_at_index(1, 0)
        .unwrap();
    let selected = cursor
        .roll_body_reentry_scope_for_step(target, &mut |_| None)
        .unwrap();
    assert!(selected.same(child));
    let mut event: crate::global::typestate::EventCommitMeta =
        cursor.try_send_meta_at(1).unwrap().into();
    event.body_reentry = selected;
    assert!(cursor.event_enabled(1, event, &mut |_, _| None).is_ok());
    event.body_reentry = parent;
    assert!(cursor.event_enabled(1, event, &mut |_, _| None).is_err());
    cursor.clear_reentry_scope_events(child);
    assert!(cursor.node_event_done_for_lane(0, 0));
    assert!(!cursor.node_event_done_for_lane(1, 0));
    assert!(!cursor.node_event_done_for_lane(2, 0));
    assert!(cursor.node_event_done_for_lane(3, 0));
}

#[cfg(all(test, hibana_repo_tests))]
#[test]
fn a_same_lane_tail_does_not_force_a_fresh_enclosing_prefix() {
    check_child_after_same_lane_tail();
}

#[cfg(kani)]
#[kani::proof]
#[kani::unwind(16)]
fn same_lane_tail_preserves_the_childs_enclosing_prefix() {
    check_child_after_same_lane_tail();
}

fn with_cross_lane_prefix(
    check: impl FnOnce(
        &mut EventCursor,
        (
            crate::global::const_dsl::ScopeId,
            crate::global::const_dsl::ScopeId,
        ),
        u8,
    ),
) {
    let global = g::seq(
        g::send::<0, 1, g::Msg<1, ()>>(),
        g::seq(
            g::par(
                g::send::<0, 1, g::Msg<2, ()>>(),
                g::send::<0, 2, g::Msg<3, ()>>(),
            )
            .roll(),
            g::par(
                g::send::<0, 1, g::Msg<4, ()>>(),
                g::send::<0, 2, g::Msg<5, ()>>(),
            ),
        ),
    )
    .roll();
    let projected = crate::runtime::program::project::<0>(&global);
    let descriptor = RoleDescriptorRef::from_resident(projected.role_image_ref());
    let mut state = MaybeUninit::<EventCursorState>::uninit();
    let mut cursor = MaybeUninit::<EventCursor>::uninit();
    let mut heads = [0u16; 2];
    let mut labels = [0u16; 2];
    let mut completed = [0u32; 5];
    assert_eq!(descriptor.logical_lane_count(), heads.len());
    assert_eq!(
        projected
            .role_image_ref()
            .footprint()
            .completion_bit_count()
            .div_ceil(32),
        completed.len()
    );
    // SAFETY: the checked descriptor fits the disjoint stationary arrays,
    // including its two roll epochs. They outlive the actual cursor.
    unsafe {
        EventCursor::init_from_compiled(
            cursor.as_mut_ptr(),
            state.as_mut_ptr(),
            heads.as_mut_ptr(),
            labels.as_mut_ptr(),
            completed.as_mut_ptr(),
            descriptor,
        );
    }
    // SAFETY: the production initializer wrote every field above.
    let cursor = unsafe { cursor.assume_init_mut() };
    let first = descriptor.local_event_rows().roll_scope_row(0).unwrap();
    let second = descriptor.local_event_rows().roll_scope_row(1).unwrap();
    let (child, parent) = if first.event_row().len() == 2 {
        (first.scope(), second.scope())
    } else {
        (second.scope(), first.scope())
    };
    let lane = cursor.event_lane_at(2).unwrap();
    assert_ne!(cursor.event_lane_at(0), Some(lane));
    assert!((3..5).any(|index| cursor.event_lane_at(index) == Some(lane)));
    for index in 0..5 {
        cursor.mark_local_event_done(index);
    }
    check(cursor, (child, parent), lane);
}

fn check_cross_lane_selected_head() {
    with_cross_lane_prefix(|cursor, (child, _), lane| {
        let target = cursor
            .relocatable_resident_lane_step_at_index(2, lane as usize)
            .unwrap();
        let selected = cursor
            .roll_body_reentry_scope_for_step(target, &mut |_| None)
            .unwrap();
        assert!(selected.same(child));
    });
}

fn check_cross_lane_child_admission() {
    with_cross_lane_prefix(|cursor, (child, _), _| {
        let mut event: crate::global::typestate::EventCommitMeta =
            cursor.try_send_meta_at(2).unwrap().into();
        event.body_reentry = child;
        assert!(cursor.event_enabled(2, event, &mut |_, _| None).is_ok());
    });
}

fn check_cross_lane_parent_and_reset() {
    with_cross_lane_prefix(|cursor, (child, parent), lane| {
        let mut event: crate::global::typestate::EventCommitMeta =
            cursor.try_send_meta_at(2).unwrap().into();
        event.body_reentry = parent;
        assert!(cursor.event_enabled(2, event, &mut |_, _| None).is_err());
        cursor.clear_reentry_scope_events(child);
        assert!(cursor.node_event_done_for_lane(0, cursor.event_lane_at(0).unwrap()));
        assert!(!cursor.node_event_done_for_lane(1, cursor.event_lane_at(1).unwrap()));
        assert!(!cursor.node_event_done_for_lane(2, lane));
    });
}

#[cfg(all(test, hibana_repo_tests))]
#[test]
fn a_child_reentry_keeps_its_cross_lane_prefix() {
    check_cross_lane_selected_head();
    check_cross_lane_child_admission();
    check_cross_lane_parent_and_reset();
}

#[cfg(kani)]
#[kani::proof]
#[kani::unwind(16)]
fn selected_reentry_scope_preserves_cross_lane_predecessors() {
    check_cross_lane_selected_head();
}

#[cfg(kani)]
#[kani::proof]
#[kani::unwind(16)]
fn cross_lane_child_admission_requires_only_its_own_fresh_steps() {
    check_cross_lane_child_admission();
}

#[cfg(kani)]
#[kani::proof]
#[kani::unwind(16)]
fn cross_lane_parent_admission_requires_its_prefix() {
    check_cross_lane_parent_and_reset();
}
