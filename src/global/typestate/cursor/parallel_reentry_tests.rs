use super::{EventCursor, EventCursorState};
use crate::{g, global::compiled::images::RoleDescriptorRef};
use core::mem::MaybeUninit;

fn parallel_continuation_global() -> impl crate::runtime::program::Projectable {
    g::par(
        g::seq(
            g::par(
                g::send::<0, 1, g::Msg<0, ()>>(),
                g::send::<0, 1, g::Msg<1, ()>>(),
            )
            .roll(),
            g::send::<0, 1, g::Msg<2, ()>>(),
        ),
        g::send::<0, 1, g::Msg<3, ()>>(),
    )
}

fn check_parallel_continuation_order(candidate: u8) {
    let global = parallel_continuation_global();
    let projected = crate::runtime::program::project::<1>(&global);
    let program = projected.role_image_ref().program();
    let descriptor = RoleDescriptorRef::from_resident(projected.role_image_ref());
    let scope = descriptor
        .local_event_rows()
        .roll_scope_row(0)
        .unwrap()
        .scope();
    assert_eq!(
        program.roll_precedes(scope, usize::from(candidate)),
        candidate == 2
    );
}

fn with_parallel_continuation_cursor<const ROLE: u8>(check: impl FnOnce(&mut EventCursor)) {
    let global = parallel_continuation_global();
    let projected = crate::runtime::program::project::<ROLE>(&global);
    let descriptor = RoleDescriptorRef::from_resident(projected.role_image_ref());
    let mut state = MaybeUninit::<EventCursorState>::uninit();
    let mut cursor = MaybeUninit::<EventCursor>::uninit();
    let mut heads = [0u16; 3];
    let mut labels = [0u16; 3];
    let mut completed = [0u32; 3];
    assert_eq!(descriptor.logical_lane_count(), heads.len());
    assert_eq!(
        projected
            .role_image_ref()
            .footprint()
            .completion_bit_count()
            .div_ceil(32),
        completed.len()
    );
    // SAFETY: these disjoint arrays fit the checked image and outlive the
    // initialized cursor, including its roll-completion and borrow epoch words.
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
    // SAFETY: the production initializer wrote both cursor objects above.
    let cursor = unsafe { cursor.assume_init_mut() };
    check(cursor);
}

fn check_parallel_continuation_lifetime() {
    with_parallel_continuation_cursor::<1>(|cursor| {
        assert!(
            cursor
                .completed_parallel_reentry(0, 2, &mut |_, _| None)
                .is_none()
        );
        cursor.mark_local_event_done(0);
        assert!(
            cursor
                .completed_parallel_reentry(0, 2, &mut |_, _| None)
                .is_none()
        );
        cursor.mark_local_event_done(1);
        let (scope, epoch) = cursor
            .completed_parallel_reentry(0, 2, &mut |_, _| None)
            .unwrap();
        assert!(cursor.borrowed_roll_reentry_is_current(scope, epoch, &mut |_| None));
        cursor.clear_reentry_scope_events(scope);
        assert!(!cursor.borrowed_roll_reentry_is_current(scope, epoch, &mut |_| None));
        assert!(
            cursor
                .completed_parallel_reentry(0, 2, &mut |_, _| None)
                .is_none()
        );
    });
}

fn check_retained_continuation<const ROLE: u8>(
    candidate: usize,
    source_end: usize,
    retained: bool,
) {
    with_parallel_continuation_cursor::<ROLE>(|cursor| {
        if retained {
            cursor.mark_local_event_done(0);
            cursor.mark_local_event_done(1);
            let (scope, _) = cursor.machine().roll_scope_row_by_slot(0).unwrap();
            cursor.clear_reentry_scope_events(scope);
        }
        assert_eq!(
            cursor.retained_roll_continuation_after(candidate, 0..source_end),
            retained && ROLE == 1 && candidate == 2 && source_end == 2,
        );
    });
}

fn check_completed_choice_alternative(candidate: u8, complete: bool) {
    let global = g::route(
        g::par(
            g::send::<0, 1, g::Msg<0, ()>>(),
            g::send::<0, 1, g::Msg<1, ()>>(),
        )
        .roll(),
        g::send::<0, 1, g::Msg<2, ()>>(),
    )
    .resolve::<0>()
    .roll();
    let projected = crate::runtime::program::project::<1>(&global);
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
    // SAFETY: these disjoint arrays fit the checked image and outlive both
    // cursor objects, including every completion bit and borrow epoch word.
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
    // SAFETY: the initializer wrote both cursor objects above.
    let cursor = unsafe { cursor.assume_init_mut() };
    cursor.mark_local_event_done(0);
    if complete {
        cursor.mark_local_event_done(1);
    }
    assert_eq!(
        cursor.completed_roll_alternative_to(usize::from(candidate), 0, &mut |_| Some(0)),
        complete && candidate == 2,
    );
}

#[cfg(all(test, hibana_repo_tests))]
#[test]
fn completed_choice_alternatives_keep_unfinished_and_parallel_work_required() {
    for candidate in 0..3 {
        for complete in [false, true] {
            check_completed_choice_alternative(candidate, complete);
        }
    }
}

#[cfg(kani)]
#[kani::proof]
#[kani::unwind(24)]
fn an_unfinished_choice_keeps_its_first_parallel_head_required() {
    check_completed_choice_alternative(0, false);
}

#[cfg(kani)]
#[kani::proof]
#[kani::unwind(24)]
fn an_unfinished_choice_keeps_its_second_parallel_head_required() {
    check_completed_choice_alternative(1, false);
}

#[cfg(kani)]
#[kani::proof]
#[kani::unwind(24)]
fn an_unfinished_choice_cannot_hide_its_other_arm() {
    check_completed_choice_alternative(2, false);
}

#[cfg(kani)]
#[kani::proof]
#[kani::unwind(24)]
fn a_completed_choice_keeps_its_first_parallel_head_required() {
    check_completed_choice_alternative(0, true);
}

#[cfg(kani)]
#[kani::proof]
#[kani::unwind(24)]
fn a_completed_choice_keeps_its_second_parallel_head_required() {
    check_completed_choice_alternative(1, true);
}

#[cfg(kani)]
#[kani::proof]
#[kani::unwind(24)]
fn a_completed_choice_can_instead_offer_its_other_arm() {
    check_completed_choice_alternative(2, true);
}

#[cfg(all(test, hibana_repo_tests))]
#[test]
fn completed_parallel_reentry_omits_only_its_causal_continuation() {
    check_parallel_continuation_lifetime();
    for candidate in 0u8..4 {
        for source_end in [0, 2, 4] {
            for retained in [false, true] {
                check_retained_continuation::<0>(usize::from(candidate), source_end, retained);
                check_retained_continuation::<1>(usize::from(candidate), source_end, retained);
            }
        }
        check_parallel_continuation_order(candidate);
    }
}

#[cfg(kani)]
#[kani::proof]
#[kani::unwind(16)]
fn completed_parallel_reentry_requires_a_live_completed_origin() {
    check_parallel_continuation_lifetime();
}

#[cfg(kani)]
fn check_retained_receive_case(candidate: usize) {
    let source_end: u8 = kani::any();
    kani::assume(matches!(source_end, 0 | 2 | 4));
    check_retained_continuation::<1>(candidate, usize::from(source_end), kani::any());
}

#[cfg(kani)]
#[kani::proof]
#[kani::unwind(16)]
fn retained_roll_receives_keep_their_enclosing_scope() {
    check_retained_receive_case(2);
}

#[cfg(kani)]
#[kani::proof]
#[kani::unwind(16)]
fn retained_roll_first_body_receive_remains_required() {
    check_retained_receive_case(0);
}

#[cfg(kani)]
#[kani::proof]
#[kani::unwind(16)]
fn retained_roll_second_body_receive_remains_required() {
    check_retained_receive_case(1);
}

#[cfg(kani)]
#[kani::proof]
#[kani::unwind(16)]
fn retained_roll_parallel_sibling_remains_required() {
    check_retained_receive_case(3);
}

#[cfg(kani)]
#[kani::proof]
#[kani::unwind(16)]
fn completed_parallel_reentry_retains_its_first_arm() {
    check_parallel_continuation_order(0);
}

#[cfg(kani)]
#[kani::proof]
#[kani::unwind(16)]
fn completed_parallel_reentry_retains_its_second_arm() {
    check_parallel_continuation_order(1);
}

#[cfg(kani)]
#[kani::proof]
#[kani::unwind(16)]
fn completed_parallel_reentry_allows_its_sequential_continuation() {
    check_parallel_continuation_order(2);
}

#[cfg(kani)]
#[kani::proof]
#[kani::unwind(16)]
fn completed_parallel_reentry_retains_independent_siblings() {
    check_parallel_continuation_order(3);
}

fn check_parallel_offer_scope(candidate: u8) {
    let global = g::par(
        g::send::<0, 1, g::Msg<80, ()>>(),
        g::route(
            g::par(
                g::par(
                    g::par(
                        g::send::<0, 1, g::Msg<81, ()>>(),
                        g::send::<0, 1, g::Msg<82, ()>>(),
                    ),
                    g::send::<0, 1, g::Msg<83, ()>>(),
                ),
                g::send::<0, 1, g::Msg<84, ()>>(),
            ),
            g::send::<0, 1, g::Msg<85, ()>>(),
        ),
    );
    let projected = crate::runtime::program::project::<1>(&global);
    let descriptor = RoleDescriptorRef::from_resident(projected.role_image_ref());
    let mut state = MaybeUninit::<EventCursorState>::uninit();
    let mut cursor = MaybeUninit::<EventCursor>::uninit();
    let mut heads = [0u16; 5];
    let mut labels = [0u16; 5];
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
    // SAFETY: the verified image fits these disjoint stationary arrays, which
    // outlive the cursor and every reference created by its initializer.
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
    // SAFETY: the production initializer wrote both cursor objects above.
    let cursor = unsafe { cursor.assume_init_mut() };
    assert_eq!(cursor.local_steps_len(), 6);
    let index = usize::from(candidate % 6);
    let node_scope = cursor.node_scope_id_at(index);
    let route = cursor.route_scope_for_offer_node(node_scope, index);
    assert_eq!(route.is_some(), index != 0);
    if let Some(route) = route {
        assert!(cursor.has_route_scope(route));
        let normalized = cursor
            .normalize_lane_offer_entry(route, index, |_| None, || None)
            .unwrap();
        assert!(normalized.scope_id().same(route));
        assert_eq!(cursor.logical_role_at(normalized.entry_idx()), Some(1));
        assert!(cursor.route_arm_for_index(route, index).is_some());
    }
    if index == 1 {
        assert!(!cursor.has_route_scope(node_scope));
    }
    assert_eq!(cursor.index(), 0);
    assert_eq!(cursor.completion_mutations(), 0);
}

#[cfg(all(test, hibana_repo_tests))]
#[test]
fn parallel_offer_heads_keep_their_enclosing_choice() {
    for candidate in 0..6 {
        check_parallel_offer_scope(candidate);
    }
}

#[cfg(kani)]
#[kani::proof]
#[kani::unwind(20)]
fn parallel_offer_outside_step_has_no_choice() {
    check_parallel_offer_scope(0);
}

#[cfg(kani)]
#[kani::proof]
#[kani::unwind(20)]
fn parallel_offer_first_head_keeps_its_choice() {
    check_parallel_offer_scope(1);
}

#[cfg(kani)]
#[kani::proof]
#[kani::unwind(20)]
fn parallel_offer_second_head_keeps_its_choice() {
    check_parallel_offer_scope(2);
}

#[cfg(kani)]
#[kani::proof]
#[kani::unwind(20)]
fn parallel_offer_third_head_keeps_its_choice() {
    check_parallel_offer_scope(3);
}

#[cfg(kani)]
#[kani::proof]
#[kani::unwind(20)]
fn parallel_offer_fourth_head_keeps_its_choice() {
    check_parallel_offer_scope(4);
}

#[cfg(kani)]
#[kani::proof]
#[kani::unwind(20)]
fn parallel_offer_other_arm_keeps_its_choice() {
    check_parallel_offer_scope(5);
}

fn check_late_locator_admission(case: u8) {
    let global = g::route(
        g::route(
            g::send::<0, 1, g::Msg<0, ()>>(),
            g::par(
                g::route(
                    g::send::<0, 1, g::Msg<1, ()>>(),
                    g::send::<0, 1, g::Msg<2, ()>>(),
                ),
                g::send::<0, 1, g::Msg<3, ()>>(),
            ),
        ),
        g::par(
            g::send::<0, 1, g::Msg<4, ()>>(),
            g::send::<0, 1, g::Msg<5, ()>>(),
        ),
    );
    let projected = crate::runtime::program::project::<1>(&global);
    let descriptor = RoleDescriptorRef::from_resident(projected.role_image_ref());
    let mut state = MaybeUninit::<EventCursorState>::uninit();
    let mut cursor = MaybeUninit::<EventCursor>::uninit();
    let mut heads = [0u16; 2];
    let mut labels = [0u16; 2];
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
    // SAFETY: the verified image fits these disjoint stationary arrays, which
    // outlive the cursor and every reference created by its initializer.
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
    // SAFETY: the production initializer wrote both cursor objects above.
    let cursor = unsafe { cursor.assume_init_mut() };
    assert_eq!(cursor.local_steps_len(), 6);
    cursor.mark_local_event_done(3);
    let target = cursor.try_recv_meta_at(1).unwrap();
    let late = cursor.try_recv_meta_at(4).unwrap();
    assert_eq!(target.lane, late.lane);
    let position = cursor
        .relocatable_resident_lane_step_at_index(4, usize::from(late.lane))
        .unwrap();
    let mutations = cursor.completion_mutations();
    cursor.set_lane_cursor_to_relocatable_step(position);
    assert_eq!(cursor.completion_mutations(), mutations);
    assert_eq!(
        cursor.index_for_lane_step(usize::from(target.lane)),
        Some(4)
    );
    let index = match case {
        0 => 1,
        1 => 4,
        2 => {
            cursor.mark_local_event_done(1);
            1
        }
        _ => unreachable!(),
    };
    let meta = cursor.try_recv_meta_at(index).unwrap();
    let admitted = cursor.event_enabled(index, meta.into(), &mut |scope, _| {
        if scope.same(crate::global::const_dsl::ScopeId::route(0)) {
            Some(0)
        } else if scope.same(crate::global::const_dsl::ScopeId::route(1)) {
            Some(1)
        } else {
            None
        }
    });
    assert_eq!(admitted.is_ok(), case == 0);
}

#[cfg(all(test, hibana_repo_tests))]
#[test]
fn locator_relocation_preserves_completion_and_committed_choice_checks() {
    for case in 0..3 {
        check_late_locator_admission(case);
    }
}

#[cfg(kani)]
#[kani::proof]
#[kani::unwind(20)]
fn a_later_locator_keeps_an_independent_receive_enabled() {
    check_late_locator_admission(0);
}

#[cfg(kani)]
#[kani::proof]
#[kani::unwind(20)]
fn a_later_locator_does_not_authorize_another_committed_branch() {
    check_late_locator_admission(1);
}

#[cfg(kani)]
#[kani::proof]
#[kani::unwind(20)]
fn a_later_locator_cannot_replay_a_completed_step() {
    check_late_locator_admission(2);
}

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
        assert_eq!(
            cursor.event_enabled(2, event, &mut |_, _| None).err(),
            Some(super::EventAdmissionError::PredecessorPending),
        );
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
