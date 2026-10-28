use super::*;
use crate::{
    endpoint::{
        carrier::OperationRegion,
        kernel::{
            ResidentActor, core::commit_delta::CommitDeltaApplyPermit,
            decision_state::SelectedRouteCommitRow,
        },
    },
    g::{self, Msg},
    global::{role_program::RoleProgram, typestate::PackedEventConflict},
    runtime::{SessionKitStorage, ids::SessionId, program::project},
    test_support::transport::TestTransport,
};

fn check_scope_queries(program: &RoleProgram<0>, sparse: bool) {
    let image = program.role_image_ref();
    assert_eq!(image.footprint().route_scope_count, 2);
    let scopes = [
        image.route_scope_by_slot(0).unwrap(),
        image.route_scope_by_slot(1).unwrap(),
    ];
    assert_eq!(
        scopes
            .iter()
            .enumerate()
            .any(|(slot, scope)| usize::from(scope.local_ordinal()) != slot),
        sparse,
    );
    let mut slab = [0; 16 * 1024];
    let mut storage = SessionKitStorage::<TestTransport>::uninit();
    let kit = storage.init();
    let rv = kit.rendezvous(&mut slab, TestTransport::new()).unwrap();
    let owner = rv.enter(SessionId::new(907), program).unwrap();
    // SAFETY: the affine owner pins this actor. The operation lease below
    // protects the short progress borrow; no public operation is active.
    let actor = unsafe {
        owner
            .owner
            .ptr
            .as_ref()
            .actor()
            .cast::<ResidentActor<'_, TestTransport>>()
            .as_ref()
    };
    let mut lease = actor.context.try_operation_lease().unwrap();
    actor.with_kernel(
        &mut lease,
        actor.views.get(0).unwrap(),
        OperationRegion::WHOLE,
        |kernel| {
            let absent = ScopeId::route(ScopeId::LOCAL_CAPACITY - 1);
            assert_eq!(kernel.scope_slot_for_route(absent), None);
            let queries = [
                scopes[0],
                scopes[1],
                absent,
                ScopeId::none(),
                ScopeId::roll_scope(0),
            ];
            {
                let mut preview = kernel.preview_queries();
                assert!(
                    core::mem::size_of_val(&preview)
                        <= ROLE_IMAGE_ROUTE_SCOPE_CAPACITY.div_ceil(4)
                            + core::mem::size_of::<usize>(),
                    "query storage must contain the local-slot memo and its shared kernel borrow",
                );
                for _ in 0..3 {
                    for scope in queries {
                        assert_eq!(
                            preview(scope),
                            kernel.preview_live_selected_arm_for_scope(scope)
                        );
                    }
                }
            }
            kernel.mark_scope_ready_arm(scopes[0], Arm::LEFT);
            kernel.mark_scope_ready_arm(scopes[1], Arm::RIGHT);
            {
                let mut preview = kernel.preview_queries();
                for _ in 0..3 {
                    assert_eq!(preview(scopes[1]), Some(1));
                    assert_eq!(preview(scopes[0]), Some(0));
                    for scope in queries {
                        assert_eq!(
                            preview(scope),
                            kernel.preview_live_selected_arm_for_scope(scope)
                        );
                    }
                }
            }
            kernel.clear_scope_evidence(scopes[0]);
            kernel.mark_scope_ready_arm(scopes[0], Arm::RIGHT);
            let mut preview = kernel.preview_queries();
            assert_eq!(preview(scopes[0]), Some(1));
            assert_eq!(preview(scopes[1]), Some(1));
        },
    );
}

#[test]
fn dense_scope_queries_keep_distinct_answers_and_fresh_phases() {
    let global = g::seq(
        g::route(g::send::<0, 1, Msg<1, ()>>(), g::send::<0, 1, Msg<2, ()>>()),
        g::route(g::send::<0, 1, Msg<3, ()>>(), g::send::<0, 1, Msg<4, ()>>()),
    );
    check_scope_queries(&project::<0>(&global), false);
}

#[test]
fn sparse_scope_queries_preserve_absent_and_nonroute_answers() {
    let global = g::seq(
        g::route(g::send::<2, 3, Msg<5, ()>>(), g::send::<2, 3, Msg<6, ()>>()),
        g::seq(
            g::route(g::send::<0, 1, Msg<1, ()>>(), g::send::<0, 1, Msg<2, ()>>()),
            g::seq(
                g::route(g::send::<2, 3, Msg<7, ()>>(), g::send::<2, 3, Msg<8, ()>>()),
                g::route(g::send::<0, 1, Msg<3, ()>>(), g::send::<0, 1, Msg<4, ()>>()),
            ),
        ),
    );
    check_scope_queries(&project::<0>(&global), true);
}

#[test]
fn a_fresh_query_masks_completed_committed_visit_without_rewriting_it() {
    let global = g::route(g::send::<0, 1, Msg<1, ()>>(), g::send::<0, 1, Msg<2, ()>>()).roll();
    let program = project::<0>(&global);
    let scope = program.role_image_ref().route_scope_by_slot(0).unwrap();
    let mut slab = [0; 16 * 1024];
    let mut storage = SessionKitStorage::<TestTransport>::uninit();
    let kit = storage.init();
    let rv = kit.rendezvous(&mut slab, TestTransport::new()).unwrap();
    let owner = rv.enter(SessionId::new(908), &program).unwrap();
    // SAFETY: this owner keeps the actor stationary, and the operation lease
    // excludes another kernel during the synthetic committed-progress cuts.
    let actor = unsafe {
        owner
            .owner
            .ptr
            .as_ref()
            .actor()
            .cast::<ResidentActor<'_, TestTransport>>()
            .as_ref()
    };
    let mut lease = actor.context.try_operation_lease().unwrap();
    actor.with_kernel(
        &mut lease,
        actor.views.get(0).unwrap(),
        OperationRegion::WHOLE,
        |kernel| {
            let slot = kernel.scope_slot_for_route(scope).unwrap();
            let row = SelectedRouteCommitRow::from_resident_conflict(
                PackedEventConflict::route_arm(scope, 0),
            )
            .unwrap();
            kernel
                .progress
                .decision_state
                .apply_prepared_route_selection(0, slot, row, CommitDeltaApplyPermit::new());
            {
                let mut preview = kernel.preview_queries();
                assert_eq!(preview(scope), Some(0));
                assert_eq!(preview(scope), Some(0));
            }
            let step = kernel
                .progress
                .cursor
                .relocatable_resident_lane_step_at_index(0, 0)
                .unwrap();
            kernel
                .progress
                .cursor
                .advance_lane_to_relocatable_step(step);
            assert_eq!(kernel.selected_arm_for_scope(scope), Some(0));
            {
                let mut preview = kernel.preview_queries();
                assert_eq!(preview(scope), None);
                assert_eq!(preview(scope), None);
            }
            kernel.mark_scope_ready_arm(scope, Arm::RIGHT);
            let mut preview = kernel.preview_queries();
            assert_eq!(preview(scope), Some(1));
            assert_eq!(preview(scope), Some(1));
            assert_eq!(kernel.selected_arm_for_scope(scope), Some(0));
        },
    );
}
