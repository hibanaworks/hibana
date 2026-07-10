//! Production cursor/reference frontier equivalence tests.

#[path = "event_program_generated_corpus_tests.rs"]
mod generated_corpus;
#[path = "../test_support/lean_proof_export.rs"]
mod lean_proof_export;

use super::event_program_tests::{ReferenceLocalProgram, ReferenceProject, ReferenceState};
use crate::global::{
    compiled::images::RoleDescriptorRef,
    const_dsl::ScopeId,
    event_program::LocalEventProgram,
    program::Projectable,
    typestate::{
        EventCursor, EventCursorState, LocalAction, LocalConflict, PackedEventConflict,
        RelocatableResidentLaneStep, SendMeta, SendPreviewError, StateIndex, state_index_to_usize,
    },
};
use std::{boxed::Box, mem::MaybeUninit, string::String, vec, vec::Vec};

type A = crate::g::Send<0, 1, crate::g::Msg<1, ()>>;
type B = crate::g::Send<0, 1, crate::g::Msg<2, ()>>;
type C = crate::g::Send<0, 1, crate::g::Msg<3, ()>>;
type D = crate::g::Send<0, 1, crate::g::Msg<4, ()>>;
type E = crate::g::Send<0, 1, crate::g::Msg<5, ()>>;
type R = crate::g::Send<0, 1, crate::g::Msg<6, ()>>;
type Post = crate::g::Send<0, 1, crate::g::Msg<7, ()>>;

#[test]
fn resident_descriptor_local_labels_match_event_program_witness_for_nested_parallel_join() {
    type InnerJoin = crate::g::Par<A, B>;
    type Left = crate::g::Seq<InnerJoin, D>;
    type Program = crate::g::Seq<crate::g::Par<Left, E>, Post>;

    let projected: crate::runtime::program::RoleProgram<0> =
        crate::runtime::program::project(&crate::g::seq(
            crate::g::par(
                crate::g::seq(
                    crate::g::par(
                        crate::g::send::<0, 1, crate::g::Msg<1, ()>>(),
                        crate::g::send::<0, 1, crate::g::Msg<2, ()>>(),
                    ),
                    crate::g::send::<0, 1, crate::g::Msg<4, ()>>(),
                ),
                crate::g::send::<0, 1, crate::g::Msg<5, ()>>(),
            ),
            crate::g::send::<0, 1, crate::g::Msg<7, ()>>(),
        ));
    let descriptor = RoleDescriptorRef::from_resident(projected.role_image_ref());
    let event_program = LocalEventProgram::from_rows(descriptor.local_event_rows());
    let reference = ReferenceLocalProgram::from_steps::<Program, 0>();

    assert_eq!(
        event_program_labels(event_program),
        reference_labels(&reference)
    );
}

#[test]
fn production_cursor_enabled_frontier_matches_reference_for_nested_parallel_join() {
    type InnerJoin = crate::g::Par<A, B>;
    type Left = crate::g::Seq<InnerJoin, D>;
    type Program = crate::g::Seq<crate::g::Par<Left, E>, Post>;

    let runtime = crate::g::seq(
        crate::g::par(
            crate::g::seq(
                crate::g::par(
                    crate::g::send::<0, 1, crate::g::Msg<1, ()>>(),
                    crate::g::send::<0, 1, crate::g::Msg<2, ()>>(),
                ),
                crate::g::send::<0, 1, crate::g::Msg<4, ()>>(),
            ),
            crate::g::send::<0, 1, crate::g::Msg<5, ()>>(),
        ),
        crate::g::send::<0, 1, crate::g::Msg<7, ()>>(),
    );
    let mut trace = ReferenceCursorTrace::<Program, 0>::new(&runtime);

    trace.assert_enabled(&[1, 2, 5]);
    trace.commit(1);
    trace.assert_enabled(&[2, 5]);
    trace.assert_not_enabled(4);
    trace.commit(2);
    trace.assert_enabled(&[4, 5]);
    trace.commit(4);
    trace.assert_enabled(&[5]);
    trace.commit(5);
    trace.assert_enabled(&[7]);
}

#[test]
fn production_cursor_keeps_multipeer_parallel_join_live_after_sibling_first() {
    const LOCAL: u8 = 1;
    const WORKER: u8 = 2;
    const SIDE: u8 = 3;
    const OBSERVER: u8 = 4;
    const A: u8 = 221;
    const B: u8 = 222;
    const D: u8 = 224;
    const E: u8 = 204;
    const POST: u8 = 226;

    let runtime = crate::g::seq(
        crate::g::par(
            crate::g::seq(
                crate::g::par(
                    crate::g::send::<LOCAL, WORKER, crate::g::Msg<{ A }, ()>>(),
                    crate::g::send::<LOCAL, SIDE, crate::g::Msg<{ B }, ()>>(),
                ),
                crate::g::send::<LOCAL, WORKER, crate::g::Msg<{ D }, ()>>(),
            ),
            crate::g::send::<LOCAL, OBSERVER, crate::g::Msg<{ E }, ()>>(),
        ),
        crate::g::send::<LOCAL, OBSERVER, crate::g::Msg<{ POST }, ()>>(),
    );
    let mut trace = ProductionCursorTrace::new::<LOCAL>(&runtime);

    assert_sorted_eq(trace.enabled_labels(), &[A, B, E]);
    trace.commit_label(E);
    assert!(!trace.enabled_labels().contains(&POST));
    trace.commit_label(A);
    assert!(!trace.enabled_labels().contains(&D));
    trace.commit_label(B);
    assert_sorted_eq(trace.enabled_labels(), &[D]);
    trace.commit_label(D);
    assert_sorted_eq(trace.enabled_labels(), &[POST]);
}

#[test]
fn production_cursor_reenters_completed_rolled_seq_head() {
    let rolled = crate::g::seq(
        crate::g::send::<0, 1, crate::g::Msg<91, ()>>(),
        crate::g::send::<1, 0, crate::g::Msg<92, ()>>(),
    )
    .roll();
    let program = crate::g::seq(rolled, crate::g::send::<0, 1, crate::g::Msg<93, ()>>());

    let mut runtime = ProductionCursorTrace::new::<0>(&program);
    runtime.commit_label(91);
    runtime.commit_label(92);

    assert!(
        runtime.enabled_commit_at(0).is_some(),
        "rolled head should be event-enabled after its scope completed"
    );
    let (meta, cursor_index) = runtime
        .preview_send_meta_for_label::<0>(91)
        .expect("rolled head should be send-previewable");
    assert_eq!(meta.label, 91);
    assert_eq!(state_index_to_usize(cursor_index), 0);
}

#[test]
fn production_cursor_reenters_rolled_route_scope_inside_sequence() {
    let inner = crate::g::route(
        crate::g::send::<0, 1, crate::g::Msg<162, ()>>(),
        crate::g::send::<0, 1, crate::g::Msg<163, ()>>(),
    )
    .roll();
    let program = crate::g::seq(
        crate::g::send::<0, 1, crate::g::Msg<161, ()>>(),
        crate::g::seq(inner, crate::g::send::<1, 0, crate::g::Msg<164, ()>>()),
    );

    let mut runtime = ProductionCursorTrace::new::<0>(&program);
    runtime.commit_label(161);
    runtime
        .preview_send_meta_for_label::<0>(162)
        .expect("rolled route head should be send-previewable after prefix");
    runtime.commit_label(162);
    runtime
        .preview_send_meta_for_label::<0>(162)
        .expect("rolled route head should be send-previewable after selected arm completes");
}

#[test]
fn production_cursor_reenters_rolled_route_from_completed_outer_arm_to_nested_arm() {
    let a = crate::g::seq(
        crate::g::send::<0, 1, crate::g::Msg<181, ()>>(),
        crate::g::send::<1, 0, crate::g::Msg<182, ()>>(),
    );
    let b = crate::g::seq(
        crate::g::send::<0, 1, crate::g::Msg<183, ()>>(),
        crate::g::send::<1, 0, crate::g::Msg<184, ()>>(),
    );
    let c = crate::g::seq(
        crate::g::send::<0, 1, crate::g::Msg<185, ()>>(),
        crate::g::send::<1, 0, crate::g::Msg<186, ()>>(),
    );
    let program = crate::g::route(a, crate::g::route(b, c)).roll();

    let mut runtime = ProductionCursorTrace::new::<0>(&program);
    runtime.commit_label(181);
    runtime.commit_label(182);

    let (meta, cursor_index) = runtime
        .preview_send_meta_for_label::<0>(183)
        .expect("rolled outer route should reenter nested right-left arm after left arm completes");
    assert_eq!(meta.label, 183);
    assert_eq!(state_index_to_usize(cursor_index), 2);
}

#[test]
fn production_cursor_resolves_deep_left_spine_first_recv_dispatch_from_root() {
    let a = crate::g::seq(
        crate::g::send::<0, 1, crate::g::Msg<201, ()>>(),
        crate::g::send::<1, 0, crate::g::Msg<202, ()>>(),
    );
    let b = crate::g::seq(
        crate::g::send::<0, 1, crate::g::Msg<203, ()>>(),
        crate::g::send::<1, 0, crate::g::Msg<204, ()>>(),
    );
    let c = crate::g::seq(
        crate::g::send::<0, 1, crate::g::Msg<205, ()>>(),
        crate::g::send::<1, 0, crate::g::Msg<206, ()>>(),
    );
    let d = crate::g::seq(
        crate::g::send::<0, 1, crate::g::Msg<207, ()>>(),
        crate::g::send::<1, 0, crate::g::Msg<208, ()>>(),
    );
    let e = crate::g::seq(
        crate::g::send::<0, 1, crate::g::Msg<209, ()>>(),
        crate::g::send::<1, 0, crate::g::Msg<210, ()>>(),
    );
    let f = crate::g::seq(
        crate::g::send::<0, 1, crate::g::Msg<211, ()>>(),
        crate::g::send::<1, 0, crate::g::Msg<212, ()>>(),
    );
    let g = crate::g::seq(
        crate::g::send::<0, 1, crate::g::Msg<213, ()>>(),
        crate::g::send::<1, 0, crate::g::Msg<214, ()>>(),
    );
    let h = crate::g::seq(
        crate::g::send::<0, 1, crate::g::Msg<215, ()>>(),
        crate::g::send::<1, 0, crate::g::Msg<216, ()>>(),
    );
    let program = crate::g::route(
        crate::g::route(
            crate::g::route(
                crate::g::route(
                    crate::g::route(crate::g::route(crate::g::route(a, b), c), d),
                    e,
                ),
                f,
            ),
            g,
        ),
        h,
    )
    .roll();
    let trace = ProductionCursorTrace::new::<1>(&program);

    assert_eq!(trace.event_program.footprint().route_scope_count, 7);
    let root_scope = (0..trace.event_program.footprint().route_scope_count)
        .find_map(|slot| {
            trace
                .event_program
                .route_scope_conflict_by_slot(slot)
                .to_conflict()
                .is_none()
                .then(|| {
                    trace
                        .event_program
                        .route_scope_rows_by_slot(slot)
                        .expect("root route scope rows")
                        .scope()
                })
        })
        .expect("root route scope");
    let lane_h = trace.lane_for_label(215);
    let frame_h = trace.frame_label_for_label(215);

    assert_eq!(
        trace
            .cursor()
            .passive_descendant_dispatch_arm_from_exact_frame_label(root_scope, lane_h, frame_h),
        Some(1)
    );
}

#[test]
fn production_cursor_previews_repeated_label_continuation_inside_rolled_route_arm() {
    let prompt = crate::g::seq(
        crate::g::send::<0, 1, crate::g::Msg<85, ()>>(),
        crate::g::send::<1, 0, crate::g::Msg<86, ()>>(),
    );
    let read = || {
        crate::g::seq(
            crate::g::send::<0, 1, crate::g::Msg<87, ()>>(),
            crate::g::send::<1, 0, crate::g::Msg<88, ()>>(),
        )
    };
    let close = crate::g::seq(
        crate::g::send::<0, 1, crate::g::Msg<91, ()>>(),
        crate::g::send::<1, 0, crate::g::Msg<92, ()>>(),
    );
    let runtime =
        crate::g::route(prompt, crate::g::seq(read(), crate::g::seq(read(), close))).roll();
    let mut trace = ProductionCursorTrace::new::<0>(&runtime);
    let region = trace
        .event_program
        .route_scope_rows_by_slot(0)
        .expect("route row");
    let slot = trace
        .event_program
        .route_scope_slot(region.scope())
        .expect("route slot");
    let right = trace
        .event_program
        .route_arm_event_row_by_slot(slot, 1)
        .expect("right arm row");
    assert_eq!((right.start(), right.end()), (2, 8));

    trace.commit_label(87);
    trace.commit_label(88);
    assert!(
        trace.preview_send_meta_for_label::<0>(87).is_ok(),
        "second same-label send in selected rolled route arm must remain previewable"
    );
}

#[test]
fn production_cursor_previews_repeated_intrinsic_route_segments() {
    const LEFT: u8 = 84;
    const RIGHT: u8 = 85;
    const ACK: u8 = 2;

    let route_segment = || {
        crate::g::seq(
            crate::g::route(
                crate::g::send::<0, 1, crate::g::Msg<{ LEFT }, ()>>(),
                crate::g::send::<0, 1, crate::g::Msg<{ RIGHT }, ()>>(),
            ),
            crate::g::send::<1, 0, crate::g::Msg<{ ACK }, ()>>(),
        )
    };
    let program = crate::g::seq(
        route_segment(),
        crate::g::seq(
            route_segment(),
            crate::g::seq(route_segment(), route_segment()),
        ),
    );
    let mut trace = ProductionCursorTrace::new::<0>(&program);

    trace.preview_send_meta_for_label::<0>(LEFT).unwrap();
    trace.commit_label(LEFT);
    trace.commit_label(ACK);
    assert_sorted_eq(trace.enabled_labels(), &[LEFT, RIGHT]);
    trace.preview_send_meta_for_label::<0>(RIGHT).unwrap();
    trace.commit_label(RIGHT);
    trace.commit_label(ACK);
    trace.preview_send_meta_for_label::<0>(LEFT).unwrap();
    trace.commit_label(LEFT);
    trace.commit_label(ACK);
    trace.preview_send_meta_for_label::<0>(RIGHT).unwrap();
}

#[test]
fn production_cursor_enabled_frontier_matches_reference_for_route_inside_join() {
    type Choice = crate::g::Route<A, B>;
    type Program = crate::g::Seq<crate::g::Par<Choice, C>, Post>;

    let runtime = crate::g::seq(
        crate::g::par(
            crate::g::route(
                crate::g::send::<0, 1, crate::g::Msg<1, ()>>(),
                crate::g::send::<0, 1, crate::g::Msg<2, ()>>(),
            ),
            crate::g::send::<0, 1, crate::g::Msg<3, ()>>(),
        ),
        crate::g::send::<0, 1, crate::g::Msg<7, ()>>(),
    );
    let mut left = ReferenceCursorTrace::<Program, 0>::new(&runtime);
    left.assert_enabled(&[1, 2, 3]);
    left.commit(1);
    left.assert_enabled(&[3]);
    left.assert_not_enabled(2);
    left.commit(3);
    left.assert_enabled(&[7]);

    let mut right = ReferenceCursorTrace::<Program, 0>::new(&runtime);
    right.assert_enabled(&[1, 2, 3]);
    right.commit(2);
    right.assert_enabled(&[3]);
    right.assert_not_enabled(1);
    right.commit(3);
    right.assert_enabled(&[7]);
}

#[test]
fn production_cursor_enabled_frontier_matches_reference_for_dead_nested_route_arm() {
    type InnerLeft = crate::g::Par<B, C>;
    type InnerRightRoute = crate::g::Route<InnerLeft, D>;
    type Choice = crate::g::Route<A, InnerRightRoute>;
    type Program = crate::g::Seq<Choice, Post>;

    let runtime = crate::g::seq(
        crate::g::route(
            crate::g::send::<0, 1, crate::g::Msg<1, ()>>(),
            crate::g::route(
                crate::g::par(
                    crate::g::send::<0, 1, crate::g::Msg<2, ()>>(),
                    crate::g::send::<0, 1, crate::g::Msg<3, ()>>(),
                ),
                crate::g::send::<0, 1, crate::g::Msg<4, ()>>(),
            ),
        ),
        crate::g::send::<0, 1, crate::g::Msg<7, ()>>(),
    );
    let mut trace = ReferenceCursorTrace::<Program, 0>::new(&runtime);

    trace.assert_enabled(&[1, 2, 3, 4]);
    trace.commit(1);
    trace.assert_enabled(&[7]);
    for label in [2, 3, 4] {
        trace.assert_not_enabled(label);
    }
}

#[test]
fn production_cursor_commits_full_conflict_chain_for_triple_nested_route() {
    let runtime = crate::g::seq(
        crate::g::route(
            crate::g::route(
                crate::g::route(
                    crate::g::send::<0, 1, crate::g::Msg<1, ()>>(),
                    crate::g::send::<0, 1, crate::g::Msg<2, ()>>(),
                ),
                crate::g::send::<0, 1, crate::g::Msg<3, ()>>(),
            ),
            crate::g::send::<0, 1, crate::g::Msg<6, ()>>(),
        ),
        crate::g::send::<0, 1, crate::g::Msg<7, ()>>(),
    );
    let mut trace = ProductionCursorTrace::new::<0>(&runtime);

    assert_sorted_eq(trace.enabled_labels(), &[1, 2, 3, 6]);
    trace.commit_label(1);
    assert_eq!(trace.selected.len(), 3);
    for label in [2, 3, 6] {
        assert!(!trace.enabled_labels().contains(&label));
    }
    assert_sorted_eq(trace.enabled_labels(), &[7]);
}

#[test]
fn production_cursor_chain_commit_preserves_nested_route_continuation() {
    let runtime = crate::g::seq(
        crate::g::route(
            crate::g::seq(
                crate::g::route(
                    crate::g::send::<0, 1, crate::g::Msg<1, ()>>(),
                    crate::g::send::<0, 1, crate::g::Msg<2, ()>>(),
                ),
                crate::g::send::<0, 1, crate::g::Msg<3, ()>>(),
            ),
            crate::g::send::<0, 1, crate::g::Msg<6, ()>>(),
        ),
        crate::g::send::<0, 1, crate::g::Msg<7, ()>>(),
    );
    let mut trace = ProductionCursorTrace::new::<0>(&runtime);

    assert_sorted_eq(trace.enabled_labels(), &[1, 2, 6]);
    trace.commit_label(1);
    assert_eq!(trace.selected.len(), 2);
    assert!(!trace.enabled_labels().contains(&2));
    assert!(!trace.enabled_labels().contains(&6));
    assert_sorted_eq(trace.enabled_labels(), &[3]);
    assert!(!trace.enabled_labels().contains(&7));
    trace.commit_label(3);
    assert_sorted_eq(trace.enabled_labels(), &[7]);
}

#[test]
fn production_cursor_chain_commit_waits_for_parallel_sibling() {
    let runtime = crate::g::seq(
        crate::g::par(
            crate::g::route(
                crate::g::route(
                    crate::g::send::<0, 1, crate::g::Msg<1, ()>>(),
                    crate::g::send::<0, 1, crate::g::Msg<2, ()>>(),
                ),
                crate::g::send::<0, 1, crate::g::Msg<6, ()>>(),
            ),
            crate::g::send::<0, 1, crate::g::Msg<5, ()>>(),
        ),
        crate::g::send::<0, 1, crate::g::Msg<7, ()>>(),
    );
    let mut trace = ProductionCursorTrace::new::<0>(&runtime);

    assert_sorted_eq(trace.enabled_labels(), &[1, 2, 5, 6]);
    trace.commit_label(1);
    assert_eq!(trace.selected.len(), 2);
    assert!(!trace.enabled_labels().contains(&2));
    assert!(!trace.enabled_labels().contains(&6));
    assert_sorted_eq(trace.enabled_labels(), &[5]);
    assert!(!trace.enabled_labels().contains(&7));
    trace.commit_label(5);
    assert_sorted_eq(trace.enabled_labels(), &[7]);
}

#[test]
fn passive_arm_child_facts_are_strict_projected_descendants() {
    let runtime = crate::g::route(
        crate::g::route(
            crate::g::send::<0, 1, crate::g::Msg<1, ()>>(),
            crate::g::send::<0, 1, crate::g::Msg<2, ()>>(),
        ),
        crate::g::send::<0, 1, crate::g::Msg<6, ()>>(),
    );
    let trace = ProductionCursorTrace::new::<1>(&runtime);
    let (outer, inner) = trace.outer_inner_route_scopes();

    assert_eq!(trace.cursor().passive_child_scope(outer, 0), Some(inner));
    assert_eq!(trace.cursor().passive_child_scope(outer, 1), None);
    assert_eq!(trace.cursor().passive_child_scope(inner, 0), None);
    assert_eq!(trace.cursor().passive_child_scope(inner, 1), None);
    assert_ne!(trace.cursor().passive_child_scope(outer, 0), Some(outer));

    let lane_a = trace.lane_for_label(1);
    let lane_b = trace.lane_for_label(2);
    let lane_r = trace.lane_for_label(6);
    let frame_b = trace.frame_label_for_label(2);
    let frame_r = trace.frame_label_for_label(6);
    assert_eq!(
        trace
            .cursor()
            .passive_descendant_dispatch_arm_from_exact_frame_label(outer, lane_b, frame_b),
        Some(0)
    );
    assert_eq!(
        trace
            .cursor()
            .passive_descendant_dispatch_arm_from_exact_frame_label(outer, lane_r, frame_r),
        Some(1)
    );
    assert_eq!(
        trace
            .cursor()
            .passive_descendant_dispatch_arm_from_exact_frame_label(outer, lane_a, 99),
        None
    );
}

#[test]
fn passive_arm_child_facts_cover_right_arm_descendants() {
    let runtime = crate::g::route(
        crate::g::send::<0, 1, crate::g::Msg<6, ()>>(),
        crate::g::route(
            crate::g::send::<0, 1, crate::g::Msg<1, ()>>(),
            crate::g::send::<0, 1, crate::g::Msg<2, ()>>(),
        ),
    );
    let trace = ProductionCursorTrace::new::<1>(&runtime);
    let (outer, inner) = trace.outer_inner_route_scopes_for_arm(1);

    assert_eq!(trace.cursor().passive_child_scope(outer, 0), None);
    assert_eq!(trace.cursor().passive_child_scope(outer, 1), Some(inner));
    assert_eq!(trace.cursor().passive_child_scope(inner, 0), None);
    assert_eq!(trace.cursor().passive_child_scope(inner, 1), None);
    assert_ne!(trace.cursor().passive_child_scope(outer, 1), Some(outer));

    let lane_a = trace.lane_for_label(1);
    let lane_b = trace.lane_for_label(2);
    let lane_l = trace.lane_for_label(6);
    let frame_a = trace.frame_label_for_label(1);
    let frame_b = trace.frame_label_for_label(2);
    let frame_l = trace.frame_label_for_label(6);
    assert_eq!(
        trace
            .cursor()
            .passive_descendant_dispatch_arm_from_exact_frame_label(outer, lane_a, frame_a),
        Some(1)
    );
    assert_eq!(
        trace
            .cursor()
            .passive_descendant_dispatch_arm_from_exact_frame_label(outer, lane_l, frame_l),
        Some(0)
    );
    assert_eq!(
        trace
            .cursor()
            .passive_descendant_dispatch_arm_from_exact_frame_label(outer, lane_b, frame_b),
        Some(1)
    );
    assert_eq!(
        trace
            .cursor()
            .passive_descendant_dispatch_arm_from_exact_frame_label(inner, lane_b, frame_b),
        Some(1)
    );
}

#[test]
fn passive_arm_entry_keeps_right_spine_outer_direct_arm() {
    let runtime = crate::g::route(
        crate::g::send::<0, 1, crate::g::Msg<6, ()>>(),
        crate::g::route(
            crate::g::send::<0, 1, crate::g::Msg<1, ()>>(),
            crate::g::send::<0, 1, crate::g::Msg<2, ()>>(),
        ),
    );
    let trace = ProductionCursorTrace::new::<1>(&runtime);
    let (outer, _inner) = trace.outer_inner_route_scopes_for_arm(1);
    let outer_left = trace
        .cursor()
        .passive_observer_arm_entry_index(outer, 0)
        .expect("outer direct passive arm entry");
    assert_eq!(trace.action_label_at(outer_left), Some(6));
}

#[test]
fn reentrant_completion_covers_right_spine_prefix_then_nested_route() {
    let inner = crate::g::route(
        crate::g::send::<0, 1, crate::g::Msg<1, ()>>(),
        crate::g::send::<0, 1, crate::g::Msg<2, ()>>(),
    );
    let runtime = crate::g::route(
        crate::g::send::<0, 1, crate::g::Msg<6, ()>>(),
        crate::g::seq(crate::g::send::<0, 1, crate::g::Msg<9, ()>>(), inner),
    )
    .roll();
    let mut trace = ProductionCursorTrace::new::<1>(&runtime);
    let (outer, _inner) = trace.outer_inner_route_scopes_for_arm(1);

    trace.commit_label(9);
    trace.commit_label(2);

    let mut selected_arm_for_scope = |scope| selected_arm(&trace.selected, scope);
    assert!(
        trace
            .cursor()
            .reentrant_route_arm_event_row_done(outer, 1, &mut selected_arm_for_scope),
        "outer right arm must be complete after its prefix and selected nested arm"
    );
}

#[test]
fn reentrant_completion_covers_selected_arm_with_nested_rolled_parallel() {
    let rolled_parallel = crate::g::par(
        crate::g::send::<0, 1, crate::g::Msg<31, ()>>(),
        crate::g::send::<0, 1, crate::g::Msg<32, ()>>(),
    )
    .roll();
    let runtime = crate::g::route(
        rolled_parallel,
        crate::g::send::<0, 1, crate::g::Msg<33, ()>>(),
    )
    .roll();
    let mut trace = ProductionCursorTrace::new::<0>(&runtime);
    let outer = trace.root_route_scope();

    trace.commit_label(31);
    trace.commit_label(32);

    let mut selected_arm_for_scope = |scope| selected_arm(&trace.selected, scope);
    assert!(
        trace
            .cursor()
            .reentrant_route_arm_event_row_done(outer, 0, &mut selected_arm_for_scope),
        "route arm with a completed nested rolled parallel body must be complete"
    );
    let (meta, cursor_index) = trace
        .preview_send_meta_for_label::<0>(33)
        .expect("completed left arm should allow rolled route reentry to the right arm");
    assert_eq!(meta.route_scope, outer);
    assert_eq!(meta.selected_route_arm, Some(1));
    assert!(
        trace
            .cursor()
            .route_commit_range_for_conflict(PackedEventConflict::route_arm(meta.route_scope, 1))
            .is_some()
    );
    assert_eq!(state_index_to_usize(cursor_index), 2);
}

#[test]
fn reentrant_completion_covers_selected_arm_with_nested_rolled_sequence() {
    let rolled_sequence = crate::g::seq(
        crate::g::send::<0, 1, crate::g::Msg<34, ()>>(),
        crate::g::send::<1, 0, crate::g::Msg<35, ()>>(),
    )
    .roll();
    let runtime = crate::g::route(
        crate::g::seq(
            rolled_sequence,
            crate::g::send::<0, 1, crate::g::Msg<36, ()>>(),
        ),
        crate::g::send::<0, 1, crate::g::Msg<37, ()>>(),
    )
    .roll();
    let mut trace = ProductionCursorTrace::new::<0>(&runtime);
    let outer = trace.root_route_scope();

    trace.commit_label(34);
    trace.commit_label(35);
    trace.commit_label(36);

    let mut selected_arm_for_scope = |scope| selected_arm(&trace.selected, scope);
    assert!(
        trace
            .cursor()
            .reentrant_route_arm_event_row_done(outer, 0, &mut selected_arm_for_scope),
        "route arm with a completed nested rolled sequence and tail must be complete"
    );
    let (meta, cursor_index) = trace
        .preview_send_meta_for_label::<0>(37)
        .expect("completed left arm should allow rolled route reentry to the right arm");
    assert_eq!(meta.route_scope, outer);
    assert_eq!(meta.selected_route_arm, Some(1));
    assert!(
        trace
            .cursor()
            .route_commit_range_for_conflict(PackedEventConflict::route_arm(meta.route_scope, 1))
            .is_some()
    );
    assert_eq!(state_index_to_usize(cursor_index), 3);
}

#[test]
fn production_cursor_enabled_frontier_matches_reference_for_alternating_route_parallel() {
    type InnerChoice = crate::g::Route<A, B>;
    type OuterLeft = crate::g::Par<InnerChoice, C>;
    type OuterChoice = crate::g::Route<OuterLeft, R>;
    type Program = crate::g::Seq<crate::g::Par<OuterChoice, E>, Post>;

    let runtime = crate::g::seq(
        crate::g::par(
            crate::g::route(
                crate::g::par(
                    crate::g::route(
                        crate::g::send::<0, 1, crate::g::Msg<1, ()>>(),
                        crate::g::send::<0, 1, crate::g::Msg<2, ()>>(),
                    ),
                    crate::g::send::<0, 1, crate::g::Msg<3, ()>>(),
                ),
                crate::g::send::<0, 1, crate::g::Msg<6, ()>>(),
            ),
            crate::g::send::<0, 1, crate::g::Msg<5, ()>>(),
        ),
        crate::g::send::<0, 1, crate::g::Msg<7, ()>>(),
    );
    let mut left_inner_left = ReferenceCursorTrace::<Program, 0>::new(&runtime);
    left_inner_left.assert_enabled(&[1, 2, 3, 5, 6]);
    left_inner_left.commit(3);
    left_inner_left.assert_enabled(&[1, 2, 5]);
    left_inner_left.commit(5);
    left_inner_left.assert_enabled(&[1, 2]);
    left_inner_left.commit(1);
    left_inner_left.assert_enabled(&[7]);

    let mut left_inner_right = ReferenceCursorTrace::<Program, 0>::new(&runtime);
    left_inner_right.assert_enabled(&[1, 2, 3, 5, 6]);
    left_inner_right.commit(2);
    left_inner_right.assert_enabled(&[3, 5]);
    left_inner_right.commit(3);
    left_inner_right.assert_enabled(&[5]);
    left_inner_right.commit(5);
    left_inner_right.assert_enabled(&[7]);

    let mut outer_right = ReferenceCursorTrace::<Program, 0>::new(&runtime);
    outer_right.assert_enabled(&[1, 2, 3, 5, 6]);
    outer_right.commit(6);
    outer_right.assert_enabled(&[5]);
    outer_right.commit(5);
    outer_right.assert_enabled(&[7]);
}

type Cm21 = crate::g::Send<0, 1, crate::g::Msg<21, ()>>;
type Cm22 = crate::g::Send<0, 1, crate::g::Msg<22, ()>>;
type Cm23 = crate::g::Send<0, 1, crate::g::Msg<23, ()>>;
type Cm24 = crate::g::Send<0, 1, crate::g::Msg<24, ()>>;
type CmRoute<Left, Right> = crate::g::Route<Left, Right>;
type CmSeq<Left, Right> = crate::g::Seq<Left, Right>;
type CmPar<Left, Right> = crate::g::Par<Left, Right>;
type CmRoll<Inner> = crate::g::Roll<Inner>;

fn run_cursor_matrix<Steps>(labels: &[u8])
where
    crate::g::Program<Steps>: Projectable,
{
    let program = crate::g::Program::<Steps>::new();
    let mut trace = ProductionCursorTrace::new::<0>(&program);
    for &label in labels {
        trace.commit_label(label);
    }
}

#[test]
fn cursor_matrix_route_p_seq_p_par_p() {
    type Steps = CmRoute<CmSeq<CmPar<Cm21, Cm22>, Cm23>, Cm24>;
    run_cursor_matrix::<Steps>(&[21, 22, 23]);
}

#[test]
fn cursor_matrix_route_p_seq_p_par_r() {
    type Steps = CmRoute<CmSeq<CmRoll<CmPar<Cm21, Cm22>>, Cm23>, Cm24>;
    run_cursor_matrix::<Steps>(&[21, 22, 21, 22, 23]);
}

#[test]
fn cursor_matrix_route_p_seq_r_par_p() {
    type Steps = CmRoute<CmRoll<CmSeq<CmPar<Cm21, Cm22>, Cm23>>, Cm24>;
    run_cursor_matrix::<Steps>(&[21, 22, 23, 21, 22, 23]);
}

#[test]
fn cursor_matrix_route_p_seq_r_par_r() {
    type Steps = CmRoute<CmRoll<CmSeq<CmRoll<CmPar<Cm21, Cm22>>, Cm23>>, Cm24>;
    run_cursor_matrix::<Steps>(&[21, 22, 21, 22, 23, 21, 22, 21, 22, 23]);
}

#[test]
fn cursor_matrix_route_r_seq_p_par_p() {
    type Steps = CmRoll<CmRoute<CmSeq<CmPar<Cm21, Cm22>, Cm23>, Cm24>>;
    run_cursor_matrix::<Steps>(&[21, 22, 23, 24]);
}

#[test]
fn cursor_matrix_route_r_seq_p_par_r() {
    type Steps = CmRoll<CmRoute<CmSeq<CmRoll<CmPar<Cm21, Cm22>>, Cm23>, Cm24>>;
    run_cursor_matrix::<Steps>(&[21, 22, 21, 22, 23, 24]);
}

#[test]
fn cursor_matrix_route_r_seq_r_par_p() {
    type Steps = CmRoll<CmRoute<CmRoll<CmSeq<CmPar<Cm21, Cm22>, Cm23>>, Cm24>>;
    run_cursor_matrix::<Steps>(&[21, 22, 23, 21, 22, 23, 24]);
}

#[test]
fn cursor_matrix_route_r_seq_r_par_r() {
    type Steps = CmRoll<CmRoute<CmRoll<CmSeq<CmRoll<CmPar<Cm21, Cm22>>, Cm23>>, Cm24>>;
    run_cursor_matrix::<Steps>(&[21, 22, 21, 22, 23, 21, 22, 21, 22, 23, 24]);
}

#[test]
fn cursor_matrix_route_p_par_p_seq_p() {
    type Steps = CmRoute<CmPar<CmSeq<Cm21, Cm22>, Cm23>, Cm24>;
    run_cursor_matrix::<Steps>(&[21, 22, 23]);
}

#[test]
fn cursor_matrix_route_p_par_p_seq_r() {
    type Steps = CmRoute<CmPar<CmRoll<CmSeq<Cm21, Cm22>>, Cm23>, Cm24>;
    run_cursor_matrix::<Steps>(&[21, 22, 21, 22, 23]);
}

#[test]
fn cursor_matrix_route_p_par_r_seq_p() {
    type Steps = CmRoute<CmRoll<CmPar<CmSeq<Cm21, Cm22>, Cm23>>, Cm24>;
    run_cursor_matrix::<Steps>(&[21, 22, 23, 21, 22, 23]);
}

#[test]
fn cursor_matrix_route_p_par_r_seq_r() {
    type Steps = CmRoute<CmRoll<CmPar<CmRoll<CmSeq<Cm21, Cm22>>, Cm23>>, Cm24>;
    run_cursor_matrix::<Steps>(&[21, 22, 21, 22, 23, 21, 22, 21, 22, 23]);
}

#[test]
fn cursor_matrix_route_r_par_p_seq_p() {
    type Steps = CmRoll<CmRoute<CmPar<CmSeq<Cm21, Cm22>, Cm23>, Cm24>>;
    run_cursor_matrix::<Steps>(&[21, 22, 23, 24]);
}

#[test]
fn cursor_matrix_route_r_par_p_seq_r() {
    type Steps = CmRoll<CmRoute<CmPar<CmRoll<CmSeq<Cm21, Cm22>>, Cm23>, Cm24>>;
    run_cursor_matrix::<Steps>(&[21, 22, 21, 22, 23, 24]);
}

#[test]
fn cursor_matrix_route_r_par_r_seq_p() {
    type Steps = CmRoll<CmRoute<CmRoll<CmPar<CmSeq<Cm21, Cm22>, Cm23>>, Cm24>>;
    run_cursor_matrix::<Steps>(&[21, 22, 23, 21, 22, 23, 24]);
}

#[test]
fn cursor_matrix_route_r_par_r_seq_r() {
    type Steps = CmRoll<CmRoute<CmRoll<CmPar<CmRoll<CmSeq<Cm21, Cm22>>, Cm23>>, Cm24>>;
    run_cursor_matrix::<Steps>(&[21, 22, 21, 22, 23, 24]);
}

#[test]
fn cursor_matrix_seq_p_route_p_par_p() {
    type Steps = CmSeq<CmRoute<CmPar<Cm21, Cm22>, Cm23>, Cm24>;
    run_cursor_matrix::<Steps>(&[21, 22, 24]);
}

#[test]
fn cursor_matrix_seq_p_route_p_par_r() {
    type Steps = CmSeq<CmRoute<CmRoll<CmPar<Cm21, Cm22>>, Cm23>, Cm24>;
    run_cursor_matrix::<Steps>(&[21, 22, 21, 22, 24]);
}

#[test]
fn cursor_matrix_seq_p_route_r_par_p() {
    type Steps = CmSeq<CmRoll<CmRoute<CmPar<Cm21, Cm22>, Cm23>>, Cm24>;
    run_cursor_matrix::<Steps>(&[21, 22, 23, 24]);
}

#[test]
fn cursor_matrix_seq_p_route_r_par_r() {
    type Steps = CmSeq<CmRoll<CmRoute<CmRoll<CmPar<Cm21, Cm22>>, Cm23>>, Cm24>;
    run_cursor_matrix::<Steps>(&[21, 22, 21, 22, 23, 24]);
}

#[test]
fn cursor_matrix_seq_r_route_p_par_p() {
    type Steps = CmRoll<CmSeq<CmRoute<CmPar<Cm21, Cm22>, Cm23>, Cm24>>;
    run_cursor_matrix::<Steps>(&[21, 22, 24, 21, 22, 24]);
}

#[test]
fn cursor_matrix_seq_r_route_p_par_r() {
    type Steps = CmRoll<CmSeq<CmRoute<CmRoll<CmPar<Cm21, Cm22>>, Cm23>, Cm24>>;
    run_cursor_matrix::<Steps>(&[21, 22, 21, 22, 24, 21, 22, 21, 22, 24]);
}

#[test]
fn cursor_matrix_seq_r_route_r_par_p() {
    type Steps = CmRoll<CmSeq<CmRoll<CmRoute<CmPar<Cm21, Cm22>, Cm23>>, Cm24>>;
    run_cursor_matrix::<Steps>(&[21, 22, 23, 24, 21, 22, 23, 24]);
}

#[test]
fn cursor_matrix_seq_r_route_r_par_r() {
    type Steps = CmRoll<CmSeq<CmRoll<CmRoute<CmRoll<CmPar<Cm21, Cm22>>, Cm23>>, Cm24>>;
    run_cursor_matrix::<Steps>(&[21, 22, 21, 22, 23, 24, 21, 22, 21, 22, 23, 24]);
}

#[test]
fn cursor_matrix_seq_p_par_p_route_p() {
    type Steps = CmSeq<CmPar<CmRoute<Cm21, Cm22>, Cm23>, Cm24>;
    run_cursor_matrix::<Steps>(&[21, 23, 24]);
}

#[test]
fn cursor_matrix_seq_p_par_p_route_r() {
    type Steps = CmSeq<CmPar<CmRoll<CmRoute<Cm21, Cm22>>, Cm23>, Cm24>;
    run_cursor_matrix::<Steps>(&[21, 22, 23, 24]);
}

#[test]
fn cursor_matrix_seq_p_par_r_route_p() {
    type Steps = CmSeq<CmRoll<CmPar<CmRoute<Cm21, Cm22>, Cm23>>, Cm24>;
    run_cursor_matrix::<Steps>(&[21, 23, 21, 23, 24]);
}

#[test]
fn cursor_matrix_seq_p_par_r_route_r() {
    type Steps = CmSeq<CmRoll<CmPar<CmRoll<CmRoute<Cm21, Cm22>>, Cm23>>, Cm24>;
    run_cursor_matrix::<Steps>(&[21, 22, 23, 21, 24]);
}

#[test]
fn cursor_matrix_seq_r_par_p_route_p() {
    type Steps = CmRoll<CmSeq<CmPar<CmRoute<Cm21, Cm22>, Cm23>, Cm24>>;
    run_cursor_matrix::<Steps>(&[21, 23, 24, 21, 23, 24]);
}

#[test]
fn cursor_matrix_seq_r_par_p_route_r() {
    type Steps = CmRoll<CmSeq<CmPar<CmRoll<CmRoute<Cm21, Cm22>>, Cm23>, Cm24>>;
    run_cursor_matrix::<Steps>(&[21, 22, 23, 24, 21, 22, 23, 24]);
}

#[test]
fn cursor_matrix_seq_r_par_r_route_p() {
    type Steps = CmRoll<CmSeq<CmRoll<CmPar<CmRoute<Cm21, Cm22>, Cm23>>, Cm24>>;
    run_cursor_matrix::<Steps>(&[21, 23, 21, 23, 24, 21, 23, 21, 23, 24]);
}

#[test]
fn cursor_matrix_seq_r_par_r_route_r() {
    type Steps = CmRoll<CmSeq<CmRoll<CmPar<CmRoll<CmRoute<Cm21, Cm22>>, Cm23>>, Cm24>>;
    run_cursor_matrix::<Steps>(&[21, 22, 23, 21, 24, 21, 22, 23, 21, 24]);
}

#[test]
fn cursor_matrix_par_p_route_p_seq_p() {
    type Steps = CmPar<CmRoute<CmSeq<Cm21, Cm22>, Cm23>, Cm24>;
    run_cursor_matrix::<Steps>(&[21, 22, 24]);
}

#[test]
fn cursor_matrix_par_p_route_p_seq_r() {
    type Steps = CmPar<CmRoute<CmRoll<CmSeq<Cm21, Cm22>>, Cm23>, Cm24>;
    run_cursor_matrix::<Steps>(&[21, 22, 21, 22, 24]);
}

#[test]
fn cursor_matrix_par_p_route_r_seq_p() {
    type Steps = CmPar<CmRoll<CmRoute<CmSeq<Cm21, Cm22>, Cm23>>, Cm24>;
    run_cursor_matrix::<Steps>(&[21, 22, 23, 24]);
}

#[test]
fn cursor_matrix_par_p_route_r_seq_r() {
    type Steps = CmPar<CmRoll<CmRoute<CmRoll<CmSeq<Cm21, Cm22>>, Cm23>>, Cm24>;
    run_cursor_matrix::<Steps>(&[21, 22, 21, 22, 23, 24]);
}

#[test]
fn cursor_matrix_par_r_route_p_seq_p() {
    type Steps = CmRoll<CmPar<CmRoute<CmSeq<Cm21, Cm22>, Cm23>, Cm24>>;
    run_cursor_matrix::<Steps>(&[21, 22, 24, 21, 22, 24]);
}

#[test]
fn cursor_matrix_par_r_route_p_seq_r() {
    type Steps = CmRoll<CmPar<CmRoute<CmRoll<CmSeq<Cm21, Cm22>>, Cm23>, Cm24>>;
    run_cursor_matrix::<Steps>(&[21, 22, 21, 22, 24, 21, 22, 21, 22, 24]);
}

#[test]
fn cursor_matrix_par_r_route_r_seq_p() {
    type Steps = CmRoll<CmPar<CmRoll<CmRoute<CmSeq<Cm21, Cm22>, Cm23>>, Cm24>>;
    run_cursor_matrix::<Steps>(&[21, 22, 23, 24, 21, 22, 23, 24]);
}

#[test]
fn cursor_matrix_par_r_route_r_seq_r() {
    type Steps = CmRoll<CmPar<CmRoll<CmRoute<CmRoll<CmSeq<Cm21, Cm22>>, Cm23>>, Cm24>>;
    run_cursor_matrix::<Steps>(&[21, 22, 21, 22, 23, 24, 21, 22, 21, 22, 23, 24]);
}

#[test]
fn cursor_matrix_par_p_seq_p_route_p() {
    type Steps = CmPar<CmSeq<CmRoute<Cm21, Cm22>, Cm23>, Cm24>;
    run_cursor_matrix::<Steps>(&[21, 23, 24]);
}

#[test]
fn cursor_matrix_par_p_seq_p_route_r() {
    type Steps = CmPar<CmSeq<CmRoll<CmRoute<Cm21, Cm22>>, Cm23>, Cm24>;
    run_cursor_matrix::<Steps>(&[21, 22, 23, 24]);
}

#[test]
fn cursor_matrix_par_p_seq_r_route_p() {
    type Steps = CmPar<CmRoll<CmSeq<CmRoute<Cm21, Cm22>, Cm23>>, Cm24>;
    run_cursor_matrix::<Steps>(&[21, 23, 21, 23, 24]);
}

#[test]
fn cursor_matrix_par_p_seq_r_route_r() {
    type Steps = CmPar<CmRoll<CmSeq<CmRoll<CmRoute<Cm21, Cm22>>, Cm23>>, Cm24>;
    run_cursor_matrix::<Steps>(&[21, 22, 23, 21, 22, 23, 24]);
}

#[test]
fn cursor_matrix_par_r_seq_p_route_p() {
    type Steps = CmRoll<CmPar<CmSeq<CmRoute<Cm21, Cm22>, Cm23>, Cm24>>;
    run_cursor_matrix::<Steps>(&[21, 23, 24, 21, 23, 24]);
}

#[test]
fn cursor_matrix_par_r_seq_p_route_r() {
    type Steps = CmRoll<CmPar<CmSeq<CmRoll<CmRoute<Cm21, Cm22>>, Cm23>, Cm24>>;
    run_cursor_matrix::<Steps>(&[21, 22, 23, 24, 21, 22, 23, 24]);
}

#[test]
fn cursor_matrix_par_r_seq_r_route_p() {
    type Steps = CmRoll<CmPar<CmRoll<CmSeq<CmRoute<Cm21, Cm22>, Cm23>>, Cm24>>;
    run_cursor_matrix::<Steps>(&[21, 23, 21, 23, 24, 21, 23, 21, 23, 24]);
}

#[test]
fn cursor_matrix_par_r_seq_r_route_r() {
    type Steps = CmRoll<CmPar<CmRoll<CmSeq<CmRoll<CmRoute<Cm21, Cm22>>, Cm23>>, Cm24>>;
    run_cursor_matrix::<Steps>(&[21, 22, 23, 21, 22, 23, 24, 21, 22, 23, 21, 22, 23, 24]);
}

#[test]
fn cursor_matrix_par_route_seq_roll_keeps_tail_pending_after_head() {
    let inner = crate::g::seq(
        crate::g::send::<0, 1, crate::g::Msg<21, ()>>(),
        crate::g::send::<0, 1, crate::g::Msg<22, ()>>(),
    )
    .roll();
    let middle = crate::g::route(inner, crate::g::send::<0, 1, crate::g::Msg<23, ()>>());
    let program = crate::g::par(middle, crate::g::send::<0, 1, crate::g::Msg<24, ()>>());
    let mut trace = ProductionCursorTrace::new::<0>(&program);

    trace.commit_label(21);

    assert!(trace.event_done_at(0));
    assert!(
        !trace.event_done_at(1),
        "committing rolled sequence head must not mark its tail done"
    );
    assert_sorted_eq(trace.enabled_labels(), &[22, 24]);
}

#[test]
fn cursor_matrix_seq_par_roll_route_roll_can_exit_after_body_settles() {
    let route = crate::g::route(
        crate::g::send::<0, 1, crate::g::Msg<21, ()>>(),
        crate::g::send::<0, 1, crate::g::Msg<22, ()>>(),
    )
    .roll();
    let body = crate::g::par(route, crate::g::send::<0, 1, crate::g::Msg<23, ()>>()).roll();
    let program = crate::g::seq(body, crate::g::send::<0, 1, crate::g::Msg<24, ()>>());
    let mut trace = ProductionCursorTrace::new::<0>(&program);

    trace.commit_label(21);
    trace.commit_label(22);
    assert_eq!(trace.body_reentry_scope_for_label(23), None);
    trace.commit_label(23);

    assert!(
        trace.enabled_labels().contains(&24),
        "completed par.roll body must allow the following seq tail; state:\n{}",
        trace.debug_rows()
    );
}

#[test]
fn cursor_matrix_seq_par_roll_right_route_roll_keeps_completed_sibling_for_exit() {
    let route = crate::g::route(
        crate::g::send::<0, 1, crate::g::Msg<21, ()>>(),
        crate::g::send::<0, 1, crate::g::Msg<22, ()>>(),
    )
    .roll();
    let body = crate::g::par(route, crate::g::send::<0, 1, crate::g::Msg<23, ()>>()).roll();
    let program = crate::g::seq(body, crate::g::send::<0, 1, crate::g::Msg<24, ()>>());
    let mut trace = ProductionCursorTrace::new::<0>(&program);

    trace.commit_label(22);
    trace.commit_label(23);
    trace.commit_label(21);

    assert!(
        trace.enabled_labels().contains(&24),
        "nested route.roll reentry must not clear a completed par.roll sibling; state:\n{}",
        trace.debug_rows()
    );
}

struct ReferenceCursorTrace<Steps, const ROLE: u8>
where
    Steps: ReferenceProject<ROLE>,
{
    reference: ReferenceState<'static>,
    _reference_program: &'static ReferenceLocalProgram,
    production: ProductionCursorTrace,
    _marker: core::marker::PhantomData<Steps>,
}

impl<Steps, const ROLE: u8> ReferenceCursorTrace<Steps, ROLE>
where
    Steps: ReferenceProject<ROLE>,
{
    fn new(program: &impl Projectable) -> Self {
        let reference_program =
            Box::leak(Box::new(ReferenceLocalProgram::from_steps::<Steps, ROLE>()));
        Self {
            reference: reference_program.state(),
            _reference_program: reference_program,
            production: ProductionCursorTrace::new::<ROLE>(program),
            _marker: core::marker::PhantomData,
        }
    }

    fn assert_enabled(&self, expected: &[u8]) {
        assert_enabled(&self.reference, expected);
        assert_sorted_eq(self.production.enabled_labels(), expected);
        assert_eq!(
            sorted(self.reference.enabled_labels()),
            sorted(self.production.enabled_labels()),
            "reference and production cursor enabled frontiers diverged"
        );
    }

    fn assert_not_enabled(&self, label: u8) {
        assert!(
            !self.reference.enabled_labels().contains(&label),
            "reference unexpectedly enabled label {label}"
        );
        assert!(
            !self.production.enabled_labels().contains(&label),
            "production cursor unexpectedly enabled label {label}"
        );
    }

    fn commit(&mut self, label: u8) {
        self.reference
            .commit_label(label)
            .expect("reference label must be enabled before production commit");
        self.production.commit_label(label);
        assert_eq!(
            sorted(self.reference.enabled_labels()),
            sorted(self.production.enabled_labels()),
            "reference and production cursor enabled frontiers diverged after label {label}"
        );
    }
}

struct ProductionCursorTrace {
    descriptor: RoleDescriptorRef,
    event_program: LocalEventProgram,
    selected: Vec<(ScopeId, u8)>,
    cursor: *mut EventCursor,
    _cursor_storage: Box<MaybeUninit<EventCursor>>,
    _state_storage: Box<MaybeUninit<EventCursorState>>,
    _lane_cursors: Vec<u16>,
    _current_labels: Vec<u16>,
    _completed_words: Vec<u32>,
}

impl ProductionCursorTrace {
    fn new<const ROLE: u8>(program: &impl Projectable) -> Self {
        let projected: crate::runtime::program::RoleProgram<ROLE> =
            crate::runtime::program::project(program);
        let descriptor = RoleDescriptorRef::from_resident(projected.role_image_ref());
        let event_program = LocalEventProgram::from_rows(descriptor.local_event_rows());
        let mut cursor_storage = Box::new(MaybeUninit::<EventCursor>::uninit());
        let mut state_storage = Box::new(MaybeUninit::<EventCursorState>::uninit());
        let mut lane_cursors = vec![0u16; descriptor.logical_lane_count()];
        let mut current_labels = vec![0u16; descriptor.logical_lane_count()];
        let completed_word_count = descriptor
            .local_len()
            .checked_add(31)
            .expect("completed word count overflow")
            / 32;
        let mut completed_words = vec![0u32; completed_word_count];
        let cursor = cursor_storage.as_mut_ptr();
        unsafe {
            EventCursor::init_from_compiled(
                cursor,
                state_storage.as_mut_ptr(),
                lane_cursors.as_mut_ptr(),
                current_labels.as_mut_ptr(),
                completed_words.as_mut_ptr(),
                descriptor,
            );
        }
        Self {
            descriptor,
            event_program,
            selected: Vec::new(),
            cursor,
            _cursor_storage: cursor_storage,
            _state_storage: state_storage,
            _lane_cursors: lane_cursors,
            _current_labels: current_labels,
            _completed_words: completed_words,
        }
    }

    fn cursor(&self) -> &EventCursor {
        unsafe { &*self.cursor }
    }

    fn cursor_mut(&mut self) -> &mut EventCursor {
        unsafe { &mut *self.cursor }
    }

    fn enabled_labels(&self) -> Vec<u8> {
        let mut labels = Vec::new();
        let mut idx = 0usize;
        while idx < self.descriptor.local_len() {
            if let Some((label, _, _)) = self.enabled_commit_at(idx) {
                labels.push(label);
            }
            idx += 1;
        }
        labels
    }

    fn enabled_commit_at(&self, idx: usize) -> Option<(u8, usize, RelocatableResidentLaneStep)> {
        self.event_program.event_row_at(idx)?;
        let lane = self.event_program.local_step_lane(idx)?;
        let node = self.event_program.node(idx);
        let (eff_index, label, origin) = match node.action() {
            LocalAction::Send {
                eff_index,
                label,
                origin,
                ..
            }
            | LocalAction::Recv {
                eff_index,
                label,
                origin,
                ..
            }
            | LocalAction::Local {
                eff_index,
                label,
                origin,
                ..
            } => (eff_index, label, origin),
            LocalAction::Terminate => return None,
        };
        let selected = &self.selected;
        let mut selected_arm_for_scope = |scope| selected_arm(selected, scope);
        self.cursor()
            .event_enabled(
                idx,
                crate::global::typestate::EventCommitMeta::new(
                    eff_index,
                    label,
                    origin,
                    node.scope(),
                    node.route_arm(),
                    lane,
                ),
                &mut selected_arm_for_scope,
            )
            .map(|commit| {
                (
                    label,
                    state_index_to_usize(commit.cursor_after()),
                    commit.progress_step(),
                )
            })
            .ok()
    }

    fn outer_inner_route_scopes(&self) -> (ScopeId, ScopeId) {
        self.outer_inner_route_scopes_for_arm(0)
    }

    fn root_route_scope(&self) -> ScopeId {
        let route_count = self.event_program.footprint().route_scope_count;
        let mut slot = 0usize;
        while slot < route_count {
            let scope = self
                .event_program
                .route_scope_rows_by_slot(slot)
                .expect("route slot must contain route rows")
                .scope();
            if self
                .event_program
                .route_scope_conflict_by_slot(slot)
                .to_conflict()
                .is_none()
            {
                return scope;
            }
            slot += 1;
        }
        panic!("root route scope missing");
    }

    fn outer_inner_route_scopes_for_arm(&self, child_arm: u8) -> (ScopeId, ScopeId) {
        let route_count = self.event_program.footprint().route_scope_count;
        let mut outer = ScopeId::none();
        let mut inner = ScopeId::none();
        let mut slot = 0usize;
        while slot < route_count {
            let scope = self
                .event_program
                .route_scope_rows_by_slot(slot)
                .expect("route slot must contain route rows")
                .scope();
            match self
                .event_program
                .route_scope_conflict_by_slot(slot)
                .to_conflict()
            {
                Some(LocalConflict::RouteArm { scope: parent, arm }) => {
                    if arm == child_arm {
                        inner = scope;
                        outer = parent;
                    }
                }
                None => {
                    if outer.is_none() {
                        outer = scope;
                    }
                }
                Some(LocalConflict::Unconditional) | Some(LocalConflict::SharedRoute) => {
                    panic!("route scope test expected route-arm conflict or no conflict")
                }
            }
            slot += 1;
        }
        assert!(!outer.is_none(), "outer route scope missing");
        assert!(!inner.is_none(), "inner route scope missing");
        (outer, inner)
    }

    fn lane_for_label(&self, target_label: u8) -> u8 {
        let mut idx = 0usize;
        while idx < self.descriptor.local_len() {
            let label = match self.event_program.node(idx).action() {
                LocalAction::Send { label, .. }
                | LocalAction::Recv { label, .. }
                | LocalAction::Local { label, .. } => label,
                LocalAction::Terminate => {
                    idx += 1;
                    continue;
                }
            };
            if label == target_label {
                return self
                    .event_program
                    .local_step_lane(idx)
                    .expect("event row must carry lane");
            }
            idx += 1;
        }
        panic!("label {target_label} missing from event rows");
    }

    fn frame_label_for_label(&self, target_label: u8) -> u8 {
        let mut idx = 0usize;
        while idx < self.descriptor.local_len() {
            if let LocalAction::Recv {
                label, frame_label, ..
            } = self.event_program.node(idx).action()
                && label == target_label
            {
                return frame_label;
            }
            idx += 1;
        }
        panic!("recv label {target_label} missing from event rows");
    }

    fn action_label_at(&self, idx: usize) -> Option<u8> {
        match self.event_program.node(idx).action() {
            LocalAction::Send { label, .. }
            | LocalAction::Recv { label, .. }
            | LocalAction::Local { label, .. } => Some(label),
            LocalAction::Terminate => None,
        }
    }

    fn event_done_at(&self, idx: usize) -> bool {
        let Some(lane) = self.event_program.local_step_lane(idx) else {
            return false;
        };
        self.cursor()
            .relocatable_resident_lane_step_at_index(idx, lane as usize)
            .ok()
            .is_some_and(|step| self.cursor().relocatable_step_done(step))
    }

    fn preview_send_meta_for_label<const ROLE: u8>(
        &self,
        target_label: u8,
    ) -> Result<(SendMeta, StateIndex), SendPreviewError> {
        let selected = &self.selected;
        let mut committed_arm_for_scope = |scope| selected_arm(selected, scope);
        let mut preview_controller_arm_for_scope = |_scope| None;
        let mut selected_arm_for_scope = |scope| selected_arm(selected, scope);
        let mut lane_for_label_or_offer = |_scope: ScopeId, label| self.lane_for_label(label);
        self.cursor().send_preview_meta_for_label::<ROLE>(
            target_label,
            &mut committed_arm_for_scope,
            &mut preview_controller_arm_for_scope,
            &mut selected_arm_for_scope,
            &mut lane_for_label_or_offer,
        )
    }

    fn commit_label(&mut self, label: u8) {
        if self.commit_enabled_label_with_reentry_scope(label, None) {
            return;
        }
        if let Ok((meta, cursor_index)) = self.preview_send_meta_for_label::<0>(label) {
            let preview_idx = state_index_to_usize(cursor_index);
            let reentry_scope = self.body_reentry_scope_for_index(preview_idx, meta.lane);
            if meta.selected_route_arm.is_some() {
                self.record_event_conflict_selection(preview_idx);
            }
            if self.commit_enabled_label_with_reentry_scope(label, reentry_scope) {
                return;
            }
        }
        panic!(
            "production cursor label {label} was not enabled; enabled={:?}; preview={:?}\n{}",
            sorted(self.enabled_labels()),
            self.preview_send_meta_for_label::<0>(label),
            self.debug_rows()
        );
    }

    fn commit_enabled_label_with_reentry_scope(
        &mut self,
        label: u8,
        reentry_scope: Option<ScopeId>,
    ) -> bool {
        let mut idx = 0usize;
        while idx < self.descriptor.local_len() {
            if let Some((candidate, cursor_after, progress_step)) = self.enabled_commit_at(idx)
                && candidate == label
            {
                let reentry_scope =
                    reentry_scope.or_else(|| self.body_reentry_scope_for_step(progress_step));
                self.clear_body_reentry_scope(reentry_scope);
                self.record_event_conflict_selection(idx);
                self.cursor_mut().set_index(cursor_after);
                let _ = self
                    .cursor_mut()
                    .advance_lane_to_relocatable_step(progress_step);
                self.apply_selected_route_completion_cursor();
                return true;
            }
            idx += 1;
        }
        false
    }

    fn body_reentry_scope_for_step(
        &self,
        progress_step: RelocatableResidentLaneStep,
    ) -> Option<ScopeId> {
        let selected = &self.selected;
        let mut selected_arm_for_scope = |scope| selected_arm(selected, scope);
        self.cursor()
            .roll_body_reentry_scope_for_step(progress_step, &mut selected_arm_for_scope)
    }

    fn body_reentry_scope_for_index(&self, idx: usize, lane: u8) -> Option<ScopeId> {
        let step = self
            .cursor()
            .relocatable_resident_lane_step_at_index(idx, lane as usize)
            .ok()?;
        self.body_reentry_scope_for_step(step)
    }

    fn clear_body_reentry_scope(&mut self, reentry_scope: Option<ScopeId>) {
        if let Some(scope) = reentry_scope {
            self.cursor_mut().clear_reentry_scope_events(scope);
            self.clear_selected_routes_inside_roll(scope);
        }
    }

    fn clear_selected_routes_inside_roll(&mut self, scope: ScopeId) {
        let mut idx = 0usize;
        while idx < self.selected.len() {
            let route_scope = self.selected[idx].0;
            if self
                .cursor()
                .route_scope_contained_in_roll_scope(route_scope, scope)
            {
                self.selected.remove(idx);
            } else {
                idx += 1;
            }
        }
    }

    fn body_reentry_scope_for_label(&self, label: u8) -> Option<ScopeId> {
        let mut idx = 0usize;
        while idx < self.descriptor.local_len() {
            if self.action_label_at(idx) == Some(label)
                && let Some(lane) = self.event_program.local_step_lane(idx)
                && let Ok(step) = self
                    .cursor()
                    .relocatable_resident_lane_step_at_index(idx, lane as usize)
            {
                let selected = &self.selected;
                let mut selected_arm_for_scope = |scope| selected_arm(selected, scope);
                return self
                    .cursor()
                    .roll_body_reentry_scope_for_step(step, &mut selected_arm_for_scope);
            }
            idx += 1;
        }
        None
    }

    fn apply_selected_route_completion_cursor(&mut self) {
        let idx = self.cursor().index();
        let selected = &self.selected;
        if let Some(end) = self
            .cursor()
            .selected_enclosing_route_scope_end_at(idx, |scope| selected_arm(selected, scope))
            && end != idx
            && self.cursor().contains_node_index(end)
        {
            self.cursor_mut().set_index(end);
        }
    }

    fn record_event_conflict_selection(&mut self, idx: usize) {
        let mut conflict = self.event_program.event_conflict_for_index(idx);
        let mut depth = 0usize;
        while depth < PackedEventConflict::MAX_CHAIN_DEPTH {
            let Some(LocalConflict::RouteArm { scope, arm }) = conflict.to_conflict() else {
                return;
            };
            self.record_or_replace_selected_arm(scope, arm);
            let slot = self
                .event_program
                .route_scope_slot(scope)
                .expect("route conflict scope must have a dense route slot");
            conflict = self.event_program.route_scope_conflict_by_slot(slot);
            depth += 1;
        }
        panic!("production conflict row chain exceeded depth bound");
    }

    fn record_or_replace_selected_arm(&mut self, scope: ScopeId, arm: u8) {
        if let Some(idx) = self
            .selected
            .iter()
            .position(|(candidate, _)| *candidate == scope)
        {
            let old = self.selected[idx].1;
            if old == arm {
                return;
            }
            let mut selected_arm_for_scope = |candidate| selected_arm(&self.selected, candidate);
            assert!(
                self.cursor().reentrant_route_arm_event_row_done(
                    scope,
                    old,
                    &mut selected_arm_for_scope
                ),
                "route scope arm replacement requires completed reentrant arm"
            );
            self.selected[idx].1 = arm;
            self.cursor_mut().clear_reentry_scope_events(scope);
            let mut candidate_idx = 0usize;
            while candidate_idx < self.selected.len() {
                let candidate = self.selected[candidate_idx].0;
                if candidate != scope
                    && self
                        .cursor()
                        .route_scope_conflict_arm_for_scope(candidate, scope)
                        == Some(old)
                {
                    self.selected.remove(candidate_idx);
                } else {
                    candidate_idx += 1;
                }
            }
            return;
        }
        self.selected.push((scope, arm));
    }

    fn debug_rows(&self) -> String {
        use std::fmt::Write;

        let mut out = String::new();
        let _ = writeln!(out, "cursor index={}", self.cursor().index());
        let _ = writeln!(out, "lane heads:");
        let mut lane = 0usize;
        while lane < self.descriptor.logical_lane_count() {
            let step = self.cursor().step_index_at_lane(lane);
            let label = step.and_then(|idx| self.action_label_at(idx));
            let _ = writeln!(out, "  lane={lane} step={step:?} label={label:?}");
            lane += 1;
        }
        let _ = writeln!(out, "events:");
        let mut idx = 0usize;
        while idx < self.descriptor.local_len() {
            let label = self.action_label_at(idx);
            let lane = self.event_program.local_step_lane(idx);
            let done = lane
                .and_then(|lane| {
                    self.cursor()
                        .relocatable_resident_lane_step_at_index(idx, lane as usize)
                        .ok()
                })
                .is_some_and(|step| self.cursor().relocatable_step_done(step));
            let conflict = self
                .event_program
                .event_conflict_for_index(idx)
                .to_conflict();
            let scope = self.event_program.node(idx).scope();
            let _ = writeln!(
                out,
                "  {idx}: label={label:?} lane={lane:?} done={done} scope={scope:?} conflict={conflict:?}"
            );
            idx += 1;
        }
        let _ = writeln!(out, "route rows:");
        let mut slot = 0usize;
        while let Some(region) = self.event_program.route_scope_rows_by_slot(slot) {
            let conflict = self
                .event_program
                .route_scope_conflict_by_slot(slot)
                .to_conflict();
            let left = self.event_program.route_arm_event_row_by_slot(slot, 0);
            let right = self.event_program.route_arm_event_row_by_slot(slot, 1);
            let _ = writeln!(
                out,
                "  slot={slot} scope={:?} rows=({}, {}) reentry={} conflict={conflict:?} left={left:?} right={right:?}",
                region.scope(),
                region.start(),
                region.end(),
                region.reentry()
            );
            slot += 1;
        }
        let _ = writeln!(out, "selected arms={:?}", self.selected);
        let selected = &self.selected;
        let mut probe = 0usize;
        while probe <= self.descriptor.local_len() {
            let end = self
                .cursor()
                .selected_enclosing_route_scope_end_at(probe, |scope| {
                    selected_arm(selected, scope)
                });
            let _ = writeln!(out, "  selected_enclosing_end_at[{probe}]={end:?}");
            probe += 1;
        }
        let _ = writeln!(out, "roll rows:");
        let mut slot = 0usize;
        while let Some((scope, row)) = self.event_program.roll_scope_row_by_slot(slot) {
            let _ = writeln!(
                out,
                "  slot={slot} scope={scope:?} row=({}, {})",
                row.start(),
                row.end()
            );
            slot += 1;
        }
        out
    }
}

fn assert_enabled(state: &ReferenceState<'_>, expected: &[u8]) {
    assert_sorted_eq(state.enabled_labels(), expected);
}

fn assert_sorted_eq(actual: Vec<u8>, expected: &[u8]) {
    assert_eq!(sorted(actual), sorted(expected.to_vec()));
}

fn sorted(mut labels: Vec<u8>) -> Vec<u8> {
    labels.sort_unstable();
    labels
}

fn selected_arm(selected: &[(ScopeId, u8)], scope: ScopeId) -> Option<u8> {
    selected
        .iter()
        .find(|(candidate, _)| *candidate == scope)
        .map(|(_, arm)| *arm)
}

fn reference_labels(program: &ReferenceLocalProgram) -> Vec<u8> {
    program.events().iter().map(|event| event.label()).collect()
}

fn event_program_labels(event_program: LocalEventProgram) -> Vec<u8> {
    let mut labels = Vec::new();
    for idx in 0..event_program.local_len() {
        if let LocalAction::Send { label, .. }
        | LocalAction::Recv { label, .. }
        | LocalAction::Local { label, .. } = event_program.node(idx).action()
        {
            labels.push(label);
        }
    }
    labels
}
