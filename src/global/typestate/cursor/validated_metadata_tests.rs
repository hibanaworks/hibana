//! Complete metadata and conflict queries against frozen pre-optimization reads.
use super::*;
use crate::{
    g::{self, Msg},
    global::role_program::{RoleImageRef, RoleProgram, project},
};
use core::mem::MaybeUninit;
use std::panic::{AssertUnwindSafe, catch_unwind};

#[path = "tests/receive_candidates.rs"]
mod receive_candidates;
#[path = "tests/send_candidates.rs"]
mod send_candidates;

pub(super) fn with_rows(rows: &'static RoleImageRef, test: impl FnOnce(&mut EventCursor)) {
    let descriptor = RoleDescriptorRef::from_resident(rows);
    let mut state = MaybeUninit::<EventCursorState>::uninit();
    let mut cursor = MaybeUninit::<EventCursor>::uninit();
    let mut heads = [0u16; 256];
    let mut labels = [0u16; 256];
    let mut completed = std::vec![0u32; rows.footprint().completion_bit_count().div_ceil(32)];
    // SAFETY: static descriptor and disjoint, stationary backing arrays outlive
    // the callback. No initialized cursor or backing pointer escapes.
    unsafe {
        EventCursor::init_from_compiled(
            cursor.as_mut_ptr(),
            state.as_mut_ptr(),
            heads.as_mut_ptr(),
            labels.as_mut_ptr(),
            completed.as_mut_ptr(),
            descriptor,
        );
        test(cursor.assume_init_mut());
    }
}

fn legacy_conflict(cursor: &EventCursor, idx: usize) -> PackedEventConflict {
    let program = cursor.machine().event_program();
    if idx == program.local_len() {
        return PackedEventConflict::none();
    }
    match program.event_row_at(idx) {
        Some(row) => row.conflict(),
        None => crate::invariant(),
    }
}

struct Legacy<'a>(&'a EventCursor);
impl Legacy<'_> {
    fn machine(&self) -> &EventCursorMachine {
        self.0.machine()
    }
    fn route_arm_for_index(&self, scope: ScopeId, idx: usize) -> Option<u8> {
        self.0.route_arm_for_index(scope, idx)
    }
    // Frozen enclosing authority checks; deliberately uses the original full
    // event-row conflict read rather than the new direct conflict query.
    fn enclosing_route_scope_rows_at(&self, idx: usize) -> Option<RouteScopeRows> {
        if matches!(self.0.action_at(idx), LocalAction::Terminate) {
            return None;
        }
        let conflict = legacy_conflict(self.0, idx);
        let Some(crate::global::typestate::LocalConflict::RouteArm { scope, arm }) =
            conflict.to_conflict()
        else {
            let mut slot = 0;
            while slot < self.machine().route_scope_slot_count() {
                if self
                    .machine()
                    .route_scope_rows_by_slot(slot)
                    .is_some_and(|r| r.start() <= idx && idx < r.end())
                {
                    crate::invariant();
                }
                slot += 1;
            }
            return None;
        };
        let region = self
            .0
            .route_scope_rows(scope)
            .unwrap_or_else(|| crate::invariant());
        if idx < region.start()
            || idx >= region.end()
            || self.route_arm_for_index(scope, idx) != Some(arm)
        {
            crate::invariant();
        }
        Some(region)
    }
    #[inline]
    fn route_authority_at(&self, idx: usize, route_arm: Option<u8>) -> (ScopeId, Option<u8>) {
        let Some(region) = self.enclosing_route_scope_rows_at(idx) else {
            return (ScopeId::none(), None);
        };
        let scope = region.scope();
        if route_arm != self.route_arm_for_index(scope, idx) {
            crate::invariant();
        }
        (scope, route_arm)
    }

    pub(super) fn try_send_meta_from_node(&self, idx: usize) -> Option<SendMeta> {
        let node = self.machine().node(idx);
        match node.action() {
            LocalAction::Send {
                eff_index,
                peer,
                label,
                frame_label,
                origin,
                lane,
            } => {
                let payload_kind = self
                    .machine()
                    .program_ref()
                    .event_atom_at(eff_index.dense_ordinal())
                    .payload;
                let scope = node.scope();
                let route_arm = node.route_arm();
                let (route_scope, selected_route_arm) = self.route_authority_at(idx, route_arm);
                Some(SendMeta {
                    eff_index,
                    peer,
                    label,
                    payload_kind,
                    frame_label,
                    semantic: node.event_semantic(),
                    origin,
                    next: state_index_to_usize(node.next()),
                    scope,
                    route_scope,
                    route_arm,
                    selected_route_arm,
                    lane,
                })
            }
            LocalAction::Recv { .. } | LocalAction::Local { .. } | LocalAction::Terminate => None,
        }
    }

    pub(super) fn try_recv_meta_from_node(&self, idx: usize) -> Option<RecvMeta> {
        let node = self.machine().node(idx);
        match node.action() {
            LocalAction::Recv {
                eff_index,
                peer,
                label,
                frame_label,
                origin,
                lane,
            } => {
                let payload_kind = self
                    .machine()
                    .program_ref()
                    .event_atom_at(eff_index.dense_ordinal())
                    .payload;
                let scope = node.scope();
                let route_arm = node.route_arm();
                let (route_scope, _) = self.route_authority_at(idx, route_arm);
                Some(RecvMeta {
                    eff_index,
                    peer,
                    label,
                    payload_kind,
                    frame_label,
                    semantic: node.event_semantic(),
                    origin,
                    next: state_index_to_usize(node.next()),
                    scope,
                    route_scope,
                    route_arm,
                    choice: node.choice_mark(),
                    lane,
                })
            }
            LocalAction::Send { .. } | LocalAction::Local { .. } | LocalAction::Terminate => None,
        }
    }

    pub(super) fn try_local_meta_from_node(&self, idx: usize) -> Option<LocalMeta> {
        let node = self.machine().node(idx);
        match node.action() {
            LocalAction::Local {
                eff_index,
                label,
                frame_label,
                origin,
                lane,
            } => {
                let payload_kind = self
                    .machine()
                    .program_ref()
                    .event_atom_at(eff_index.dense_ordinal())
                    .payload;
                let scope = node.scope();
                let route_arm = node.route_arm();
                let (route_scope, _) = self.route_authority_at(idx, route_arm);
                Some(LocalMeta {
                    eff_index,
                    label,
                    payload_kind,
                    frame_label,
                    semantic: node.event_semantic(),
                    origin,
                    next: state_index_to_usize(node.next()),
                    scope,
                    route_scope,
                    route_arm,
                    lane,
                })
            }
            LocalAction::Send { .. } | LocalAction::Recv { .. } | LocalAction::Terminate => None,
        }
    }
}

fn compare(cursor: &EventCursor) {
    let old = Legacy(cursor);
    for idx in 0..=cursor.local_steps_len() {
        let action = cursor.machine().node(idx).action();
        assert_eq!(cursor.is_recv_at(idx), action.is_recv());
        assert_eq!(cursor.is_send_at(idx), action.is_send());
        assert_eq!(cursor.is_local_action_at(idx), action.is_local_action());
        let operation = match action {
            LocalAction::Send {
                eff_index, lane, ..
            }
            | LocalAction::Local {
                eff_index, lane, ..
            } => Some((eff_index, false, lane)),
            LocalAction::Recv {
                eff_index, lane, ..
            } => Some((eff_index, true, lane)),
            LocalAction::Terminate => None,
        };
        assert_eq!(
            cursor.event_lane_at(idx),
            operation.map(|(_, _, lane)| lane)
        );
        assert_eq!(
            cursor.logical_role_at(idx),
            operation.map(|(effect, receiving, _)| {
                let atom = cursor.program_ref().event_atom_at(effect.dense_ordinal());
                if receiving { atom.to } else { atom.from }
            })
        );
        for arm in [None, Some(0), Some(1)] {
            let mut observed = std::vec::Vec::new();
            let actual = cursor.event_actor_is_local(idx, &mut |scope| {
                observed.push(scope);
                arm
            });
            let mut expected_calls = std::vec::Vec::new();
            let expected = match operation {
                None => Some(false),
                Some((effect, receiving, _)) => {
                    let program = cursor.program_ref();
                    match program.actor_binding_at(effect.dense_ordinal()) {
                        None => Some(true),
                        Some(bindings) => program
                            .selected_actor(bindings[usize::from(receiving)], &mut |scope| {
                                expected_calls.push(scope);
                                arm
                            })
                            .map(|actor| actor == cursor.role()),
                    }
                }
            };
            assert_eq!(actual, expected);
            assert_eq!(observed, expected_calls);
        }
        assert_eq!(
            cursor.machine().event_conflict_for_index(idx),
            legacy_conflict(cursor, idx)
        );
        assert_eq!(
            cursor.try_send_meta_at(idx),
            old.try_send_meta_from_node(idx)
        );
        assert_eq!(
            cursor.try_recv_meta_at(idx),
            old.try_recv_meta_from_node(idx)
        );
        assert_eq!(
            cursor.try_local_meta_at(idx),
            old.try_local_meta_from_node(idx)
        );
    }
    for idx in [cursor.local_steps_len() + 1, usize::MAX] {
        assert!(catch_unwind(AssertUnwindSafe(|| cursor.is_recv_at(idx))).is_err());
        assert!(catch_unwind(AssertUnwindSafe(|| cursor.is_send_at(idx))).is_err());
        assert!(catch_unwind(AssertUnwindSafe(|| cursor.is_local_action_at(idx))).is_err());
        assert!(catch_unwind(AssertUnwindSafe(|| cursor.event_lane_at(idx))).is_err());
        assert!(catch_unwind(AssertUnwindSafe(|| cursor.logical_role_at(idx))).is_err());
        let mut called = false;
        assert!(
            catch_unwind(AssertUnwindSafe(|| cursor.event_actor_is_local(
                idx,
                &mut |_| {
                    called = true;
                    None
                }
            )))
            .is_err()
        );
        assert!(!called);
        assert!(
            catch_unwind(AssertUnwindSafe(|| cursor
                .machine()
                .event_conflict_for_index(idx)))
            .is_err()
        );
        assert!(catch_unwind(AssertUnwindSafe(|| legacy_conflict(cursor, idx))).is_err());
        assert!(catch_unwind(AssertUnwindSafe(|| cursor.try_send_meta_at(idx))).is_err());
        assert!(catch_unwind(AssertUnwindSafe(|| old.try_send_meta_from_node(idx))).is_err());
        assert!(catch_unwind(AssertUnwindSafe(|| cursor.try_recv_meta_at(idx))).is_err());
        assert!(catch_unwind(AssertUnwindSafe(|| old.try_recv_meta_from_node(idx))).is_err());
        assert!(catch_unwind(AssertUnwindSafe(|| cursor.try_local_meta_at(idx))).is_err());
        assert!(catch_unwind(AssertUnwindSafe(|| old.try_local_meta_from_node(idx))).is_err());
    }
}

impl EventCursor {
    pub(crate) fn compare_validated_metadata_image(rows: &'static RoleImageRef) {
        with_rows(rows, |cursor| {
            compare(cursor);
            // Progress changes do not affect immutable metadata authority.
            for idx in 0..cursor.local_steps_len() {
                let lane = cursor
                    .machine()
                    .event_program()
                    .local_step_lane(idx)
                    .unwrap();
                let step = cursor
                    .relocatable_resident_lane_step_at_index(idx, usize::from(lane))
                    .unwrap();
                cursor.advance_lane_to_relocatable_step(step);
            }
            compare(cursor);
        });
    }
}

fn corpus<const ROLE: u8>() {
    let graph = g::seq(
        g::route(
            g::seq(
                g::send::<0, 1, Msg<1, ()>>(),
                g::route(
                    g::seq(g::send::<0, 0, Msg<2, ()>>(), g::send::<0, 1, Msg<3, ()>>()),
                    g::send::<0, 1, Msg<4, ()>>(),
                ),
            ),
            g::send::<0, 1, Msg<5, ()>>(),
        )
        .roll(),
        g::par(g::send::<0, 1, Msg<6, ()>>(), g::send::<2, 1, Msg<7, ()>>()),
    );
    let program: RoleProgram<ROLE> = project(&graph);
    EventCursor::compare_validated_metadata_image(program.role_image_ref());
}

#[test]
fn validated_metadata_matches_frozen_legacy_nested_rolled_parallel_and_local() {
    corpus::<0>();
    corpus::<1>();
    corpus::<2>();
}

#[test]
fn validated_metadata_preserves_empty_tau_and_terminal_domains() {
    let empty: RoleProgram<1> = project(&g::send::<0, 2, Msg<1, ()>>());
    EventCursor::compare_validated_metadata_image(empty.role_image_ref());
    let tau: RoleProgram<1> = project(&g::seq(
        g::route(g::send::<0, 2, Msg<2, ()>>(), g::send::<0, 2, Msg<3, ()>>()),
        g::send::<0, 1, Msg<4, ()>>(),
    ));
    EventCursor::compare_validated_metadata_image(tau.role_image_ref());
}

#[test]
fn operation_queries_preserve_delegated_and_conditional_actor_identity() {
    type Loan = Msg<8, crate::Endpoint<'static, 10>>;
    let loan = g::seq(
        g::send::<0, 1, Loan>(),
        g::seq(
            g::send::<10, 2, Msg<9, u32>>(),
            g::seq(g::send::<2, 10, Msg<10, u32>>(), g::send::<1, 0, Loan>()),
        ),
    );
    let graph = g::route(
        g::seq(g::send::<0, 1, Msg<1, ()>>(), loan),
        g::seq(
            g::send::<0, 1, Msg<2, ()>>(),
            g::seq(
                g::send::<10, 2, Msg<9, u32>>(),
                g::send::<2, 10, Msg<10, u32>>(),
            ),
        ),
    )
    .roll();
    for rows in [
        project::<0>(&graph).role_image_ref(),
        project::<1>(&graph).role_image_ref(),
        project::<2>(&graph).role_image_ref(),
    ] {
        EventCursor::compare_validated_metadata_image(rows);
    }
}

// Test-only old/full versus filtered projection oracle over synthetic cursor
// states (completion bits varied independently of lane heads). Every query scope comes
// from a retained descriptor or a local event, never an excluded arbitrary ID.
impl EventCursor {
    pub(crate) fn assert_filtered_projection_callback_equivalence(
        old: &'static RoleImageRef,
        new: &'static RoleImageRef,
    ) {
        use std::{format, string::String, vec::Vec};
        fn observe(
            rows: &'static RoleImageRef,
            retained: &[ScopeId],
            completion: u8,
            choice: u8,
        ) -> Vec<String> {
            let mut observations = Vec::new();
            with_rows(rows, |cursor| {
                let len = rows.local_step_count();
                for idx in 0..len {
                    if completion == 2 || (completion == 1 && idx % 2 == 0) {
                        cursor.mark_local_event_done(idx);
                    }
                }
                let selected = |scope: ScopeId| match choice {
                    0 => None,
                    1 => Some(0),
                    2 => Some(1),
                    _ => Some((scope.raw() % 2) as u8),
                };
                let mut completion_calls = Vec::new();
                let complete = cursor.is_complete(&mut |scope| {
                    completion_calls.push(scope.raw());
                    selected(scope)
                });
                observations.push(format!("complete {complete:?} {completion_calls:?}"));
                for idx in 0..len {
                    let conflict = cursor.event_conflict_for_index(idx);
                    let mut calls = Vec::new();
                    let allows = cursor.event_conflict_row_allows(
                        conflict,
                        ScopeId::none(),
                        None,
                        &mut |scope| {
                            calls.push(scope.raw());
                            selected(scope)
                        },
                    );
                    observations.push(format!("conflict {idx} {allows:?} {calls:?}"));
                    calls.clear();
                    let preview = cursor.event_conflict_row_allows_with_preview(
                        conflict,
                        conflict,
                        &mut |scope| {
                            calls.push(scope.raw());
                            selected(scope)
                        },
                    );
                    observations.push(format!("preview {idx} {preview:?} {calls:?}"));
                    let lane = rows.local_step_lane(idx).unwrap();
                    let mut views = Vec::new();
                    let reentry =
                        cursor.roll_reentry_event_allows_index(idx, lane, &mut |scope, view| {
                            views.push((scope.raw(), matches!(view, EventArmView::Preview)));
                            selected(scope)
                        });
                    observations.push(format!("reentry {idx} {reentry:?} {views:?}"));
                    let step = cursor
                        .relocatable_resident_lane_step_at_index(idx, lane as usize)
                        .unwrap();
                    calls.clear();
                    let body = cursor.roll_body_reentry_scope_for_step(step, &mut |scope| {
                        calls.push(scope.raw());
                        selected(scope)
                    });
                    observations.push(format!("body {idx} {body:?} {calls:?}"));
                }
                for &scope in retained {
                    observations.push(format!(
                        "mask {} {:?}",
                        scope.raw(),
                        cursor.machine().first_recv_dispatch_arm_mask(scope)
                    ));
                    for idx in 0..len {
                        if let LocalAction::Recv {
                            peer,
                            lane,
                            frame_label,
                            ..
                        } = cursor.machine().node(idx).action()
                        {
                            let target = cursor.machine().first_recv_descendant_target_for_key(
                                scope,
                                InboundFrameKey::new(peer, lane, frame_label),
                            );
                            observations.push(format!("dispatch {} {idx} {target:?}", scope.raw()));
                        }
                    }
                }
            });
            observations
        }
        let retained: Vec<_> = (0..new.footprint().route_scope_count)
            .map(|slot| new.route_scope_by_slot(slot).unwrap())
            .collect();
        for completion in 0..3 {
            for choice in 0..4 {
                assert_eq!(
                    observe(old, &retained, completion, choice),
                    observe(new, &retained, completion, choice),
                    "projection callback/dispatch equivalence role={} completion={completion} choice={choice}",
                    old.role()
                );
            }
        }
    }
}
