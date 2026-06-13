//! Production cursor/reference frontier equivalence tests.

use super::event_program_tests::{ReferenceLocalProgram, ReferenceProject, ReferenceState};
use crate::global::{
    compiled::images::RoleDescriptorRef,
    const_dsl::ScopeId,
    event_program::LocalEventProgram,
    program::Projectable,
    typestate::{
        EventCursor, EventCursorState, LocalAction, LocalConflict, PackedEventConflict,
        RelocatableResidentLaneStep, state_index_to_usize,
    },
};
use std::{boxed::Box, mem::MaybeUninit, vec, vec::Vec};

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

    assert_eq!(
        runtime.cursor().roll_reentry_index_for_label(91, |_| None),
        Some(0)
    );
    assert!(
        runtime.enabled_commit_at(0).is_some(),
        "rolled head should be event-enabled after its scope completed"
    );
    let (meta, cursor_index) = runtime
        .cursor()
        .flow_preview_send_meta_for_label::<0>(91, |_| None, |_| None, |_, _| 0)
        .expect("rolled head should be flow-previewable");
    assert_eq!(meta.label, 91);
    assert_eq!(state_index_to_usize(cursor_index), 0);
}

#[test]
fn production_cursor_reenters_rolled_route_head_inside_sequence() {
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
        .cursor()
        .flow_preview_send_meta_for_label::<0>(162, |_| None, |_| None, |_, _| 0)
        .expect("rolled route head should be flow-previewable after prefix");
    runtime.commit_label(162);
    runtime
        .cursor()
        .flow_preview_send_meta_for_label::<0>(162, |_| None, |_| None, |_, _| 0)
        .expect("rolled route head should be flow-previewable after selected arm completes");
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
        let (eff_index, label, is_internal) = match node.action() {
            LocalAction::Send {
                eff_index,
                label,
                is_internal,
                ..
            }
            | LocalAction::Recv {
                eff_index,
                label,
                is_internal,
                ..
            }
            | LocalAction::Local {
                eff_index,
                label,
                is_internal,
                ..
            } => (eff_index, label, is_internal),
            LocalAction::Terminate => return None,
        };
        let selected = &self.selected;
        self.cursor()
            .event_enabled(
                idx,
                crate::global::typestate::EventCommitMeta::new(
                    eff_index,
                    label,
                    is_internal,
                    node.scope(),
                    node.route_arm(),
                    lane,
                ),
                |scope| selected_arm(selected, scope),
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
                    if arm == 0 {
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

    fn commit_label(&mut self, label: u8) {
        let mut idx = 0usize;
        while idx < self.descriptor.local_len() {
            if let Some((candidate, cursor_after, progress_step)) = self.enabled_commit_at(idx)
                && candidate == label
            {
                self.record_event_conflict_selection(idx);
                self.cursor_mut().set_index(cursor_after);
                let _ = self
                    .cursor_mut()
                    .advance_lane_to_relocatable_step(progress_step);
                return;
            }
            idx += 1;
        }
        panic!("production cursor label {label} was not enabled");
    }

    fn record_event_conflict_selection(&mut self, idx: usize) {
        let mut conflict = self.event_program.event_conflict_for_index(idx);
        let mut depth = 0usize;
        while depth < PackedEventConflict::MAX_CHAIN_DEPTH {
            let Some(LocalConflict::RouteArm { scope, arm }) = conflict.to_conflict() else {
                return;
            };
            self.record_selected_arm(scope, arm);
            let slot = self
                .event_program
                .route_scope_slot(scope)
                .expect("route conflict scope must have a dense route slot");
            conflict = self.event_program.route_scope_conflict_by_slot(slot);
            depth += 1;
        }
        panic!("production conflict row chain exceeded depth bound");
    }

    fn record_selected_arm(&mut self, scope: ScopeId, arm: u8) {
        if let Some((_, selected)) = self
            .selected
            .iter_mut()
            .find(|(candidate, _)| *candidate == scope)
        {
            assert_eq!(
                *selected, arm,
                "production conflict row attempted to select a different arm"
            );
            return;
        }
        self.selected.push((scope, arm));
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
