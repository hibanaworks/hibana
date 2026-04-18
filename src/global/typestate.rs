//! Role-local typestate synthesis derived from `EffList`.
//!
//! This module materialises a compact state machine for a given role directly
//! from an `EffList`. Each state captures the local action (send/recv/control)
//! together with the successor index, allowing higher layers to drive endpoint
//! transitions.

mod builder;
mod cursor;
mod emit;
mod emit_route;
mod emit_scope;
mod emit_walk;
mod facts;
mod registry;
mod route_facts;

pub use self::facts::StateIndex;
pub(crate) use self::registry::{
    RouteDispatchEntry, RouteDispatchShape, RouteScopeRecord, ScopeRecord,
};
#[cfg(test)]
pub(crate) use self::{builder::RoleTypestate, emit::phase_route_guard_for_built_state_for_role};
#[allow(unused_imports)]
pub(crate) use self::{
    builder::{
        ARM_SHARED, MAX_FIRST_RECV_DISPATCH, RoleTypestateInitStorage, RoleTypestateValue,
        ScopeRegion,
    },
    cursor::{LoopMetadata, LoopRole, PhaseCursor, PhaseCursorState},
    emit::{init_value_from_summary_for_role, phase_route_guard_for_state_for_role},
    emit_walk::RoleTypestateBuildScratch,
    facts::{
        JumpError, JumpReason, LocalAction, LocalMeta, LocalNode, MAX_STATES, PassiveArmNavigation,
        RecvMeta, SendMeta, as_eff_index, as_state_index, state_index_to_usize,
    },
};

/*
Canonical split owners retained under src/global/typestate/{facts,builder,cursor}.rs:
pub struct StateIndex(u16);
pub(crate) const MAX_STATES: usize =
pub(crate) enum JumpReason {
pub(crate) struct JumpError {
pub(crate) enum PassiveArmNavigation {
pub(crate) enum LocalAction {
pub(crate) struct LocalNode {
pub(crate) struct ScopeRegion {
pub(crate) struct ScopeRecord {
pub struct SendMeta {
pub(crate) struct RecvMeta {
pub(crate) struct LocalMeta {
pub(crate) const fn state_index_to_usize(
pub(crate) const fn node(&self, index: usize) -> LocalNode {
pub(crate) fn first_recv_target(
pub(crate) enum LoopRole {
pub(crate) struct LoopMetadata {
pub(crate) struct PhaseCursor {
pub(crate) fn typestate_node(&self, index: usize) -> LocalNode {
pub(crate) fn scope_region(&self) -> Option<ScopeRegion> {
pub(crate) fn scope_region_by_id(&self, scope_id: ScopeId) -> Option<ScopeRegion> {
pub(crate) fn route_scope_controller_policy(
pub(crate) fn try_send_meta(&self) -> Option<SendMeta>
pub(crate) fn try_recv_meta(&self) -> Option<RecvMeta>
pub(crate) fn try_local_meta(&self) -> Option<LocalMeta>
#[cfg(test)]
    pub(crate) fn assert_terminal(&self) {
*/

#[cfg(test)]
mod tests {
    use core::mem::MaybeUninit;

    use super::{LocalAction, StateIndex};
    use crate::control::cap::mint::GenericCapToken;
    use crate::control::cap::resource_kinds::{LoopBreakKind, LoopContinueKind};
    use crate::eff::EffIndex;
    use crate::g::{self, Msg, Role};
    use crate::global::compiled::{images::CompiledRoleImage, lowering::CompiledProgram};
    use crate::global::const_dsl::{PolicyMode, ScopeKind};
    use crate::global::role_program;
    use crate::global::role_program::{RoleProgram, project};
    use crate::global::steps::{
        ParSteps, PolicySteps, RouteSteps, SendStep, SeqSteps, StepCons, StepNil,
    };
    use crate::global::{CanonicalControl, MessageSpec};
    use crate::runtime::consts::{LABEL_LOOP_BREAK, LABEL_LOOP_CONTINUE};

    #[test]
    fn typed_typestate_shell_items_remain_reachable_for_internal_guards() {
        let _ = MaybeUninit::<crate::global::typestate::RoleTypestate<0>>::uninit();
        let _ = crate::global::typestate::RoleTypestate::<0>::len;
        let _ = crate::global::typestate::RoleTypestate::<0>::node;
        let _ = crate::global::typestate::RoleTypestate::<0>::scope_region_for;
        let _ = crate::global::typestate::RoleTypestate::<0>::first_recv_dispatch_entry;
        let _ = crate::global::typestate::RoleTypestate::<0>::controller_arm_entry_by_arm;
        let _ = crate::global::typestate::RoleTypestate::<0>::has_parallel_phase_scope;
        let _ = crate::global::typestate::RoleTypestate::<0>::parallel_phase_range_at;
        let _ = crate::global::typestate::RoleTypestate::<0>::init_value_from_summary;
        let _ = crate::global::typestate::phase_route_guard_for_built_state_for_role::<0>;
    }

    fn with_compiled_role_image<const ROLE: u8, Mint, R>(
        program: &RoleProgram<'_, ROLE, Mint>,
        f: impl FnOnce(&CompiledRoleImage) -> R,
    ) -> R
    where
        Mint: crate::control::cap::mint::MintConfigMarker,
    {
        crate::global::compiled::materialize::with_compiled_role_image::<ROLE, _>(
            crate::global::lowering_input(program),
            f,
        )
    }

    const BODY: g::Program<StepCons<SendStep<Role<0>, Role<1>, Msg<7, ()>>, StepNil>> =
        g::send::<Role<0>, Role<1>, Msg<7, ()>, 0>();

    const LOOP_POLICY_ID: u16 = 9300;
    const ROUTE_POLICY_ID: u16 = 9301;
    type LoopContinueHead = PolicySteps<
        StepCons<
            SendStep<
                Role<0>,
                Role<0>,
                Msg<
                    { LABEL_LOOP_CONTINUE },
                    GenericCapToken<LoopContinueKind>,
                    CanonicalControl<LoopContinueKind>,
                >,
            >,
            StepNil,
        >,
        LOOP_POLICY_ID,
    >;
    type LoopBreakHead = PolicySteps<
        StepCons<
            SendStep<
                Role<0>,
                Role<0>,
                Msg<
                    { LABEL_LOOP_BREAK },
                    GenericCapToken<LoopBreakKind>,
                    CanonicalControl<LoopBreakKind>,
                >,
            >,
            StepNil,
        >,
        LOOP_POLICY_ID,
    >;
    type LoopProgramSteps = RouteSteps<
        SeqSteps<LoopContinueHead, StepCons<SendStep<Role<0>, Role<1>, Msg<7, ()>>, StepNil>>,
        LoopBreakHead,
    >;
    type RouteScopeContinueHead = PolicySteps<
        StepCons<
            SendStep<
                Role<0>,
                Role<0>,
                Msg<
                    { LABEL_LOOP_CONTINUE },
                    GenericCapToken<LoopContinueKind>,
                    CanonicalControl<LoopContinueKind>,
                >,
            >,
            StepNil,
        >,
        ROUTE_POLICY_ID,
    >;
    type RouteScopeBreakHead = PolicySteps<
        StepCons<
            SendStep<
                Role<0>,
                Role<0>,
                Msg<
                    { LABEL_LOOP_BREAK },
                    GenericCapToken<LoopBreakKind>,
                    CanonicalControl<LoopBreakKind>,
                >,
            >,
            StepNil,
        >,
        ROUTE_POLICY_ID,
    >;
    type RouteScopeProgramSteps = RouteSteps<RouteScopeContinueHead, RouteScopeBreakHead>;

    #[allow(clippy::type_complexity)]
    const LOOP_PROGRAM: g::Program<LoopProgramSteps> = {
        // Self-send for CanonicalControl: Controller → Controller
        let continue_control = g::send::<
            Role<0>,
            Role<0>, // self-send
            Msg<
                { LABEL_LOOP_CONTINUE },
                GenericCapToken<LoopContinueKind>,
                CanonicalControl<LoopContinueKind>,
            >,
            0,
        >()
        .policy::<LOOP_POLICY_ID>();
        let continue_arm = g::seq(continue_control, BODY);
        let break_arm = g::send::<
            Role<0>,
            Role<0>, // self-send
            Msg<
                { LABEL_LOOP_BREAK },
                GenericCapToken<LoopBreakKind>,
                CanonicalControl<LoopBreakKind>,
            >,
            0,
        >()
        .policy::<LOOP_POLICY_ID>();
        // Route decision is local to Controller (0 → 0)
        g::route(continue_arm, break_arm)
    };

    const CONTROLLER_PROGRAM: RoleProgram<'static, 0> = project(&LOOP_PROGRAM);

    const TARGET_PROGRAM: RoleProgram<'static, 1> = project(&LOOP_PROGRAM);

    const LOCAL_PROGRAM: g::Program<StepCons<SendStep<Role<0>, Role<0>, Msg<9, ()>>, StepNil>> =
        g::send::<Role<0>, Role<0>, Msg<9, ()>, 0>();
    const LOCAL_ROLE: role_program::RoleProgram<'static, 0> = role_program::project(&LOCAL_PROGRAM);

    #[test]
    fn state_cursor_rewinds_on_loop_continue() {
        with_compiled_role_image(&CONTROLLER_PROGRAM, |compiled| {
            let typestate = compiled.typestate_ref();
            let scope_id = typestate.node(0).scope();
            let (continue_entry_idx, continue_label) = compiled
                .controller_arm_entry_by_arm(scope_id, 0)
                .expect("continue arm entry");
            assert_eq!(continue_label, LABEL_LOOP_CONTINUE);
            let continue_entry = typestate.node(continue_entry_idx.as_usize());
            match continue_entry.action() {
                LocalAction::Local { label, .. } => assert_eq!(label, LABEL_LOOP_CONTINUE),
                other => panic!("expected continue local action, got {other:?}"),
            }

            let after_continue = typestate.node(continue_entry.next().as_usize());
            match after_continue.action() {
                LocalAction::Send { label, .. } => assert_eq!(label, 7),
                other => panic!("expected loop body send after continue, got {other:?}"),
            }

            let (rewind_entry_idx, rewind_label) = compiled
                .controller_arm_entry_by_arm(scope_id, 0)
                .expect("continue branch rewinds");
            assert_eq!(rewind_label, LABEL_LOOP_CONTINUE);
            let rewind_entry = typestate.node(rewind_entry_idx.as_usize());
            match rewind_entry.action() {
                LocalAction::Local { label, .. } => assert_eq!(label, LABEL_LOOP_CONTINUE),
                other => panic!("expected rewound continue local action, got {other:?}"),
            }
        });
    }

    #[test]
    fn state_cursor_loop_branch_successors() {
        with_compiled_role_image(&CONTROLLER_PROGRAM, |controller_compiled| {
            let typestate = controller_compiled.typestate_ref();
            let scope_id = typestate.node(0).scope();
            let region = typestate
                .scope_region_for(scope_id)
                .expect("controller route scope");
            assert_eq!(region.kind, ScopeKind::Route);

            let (continue_entry_idx, continue_label) = controller_compiled
                .controller_arm_entry_by_arm(scope_id, 0)
                .expect("continue arm entry");
            assert_eq!(continue_label, LABEL_LOOP_CONTINUE);
            let continue_entry = typestate.node(continue_entry_idx.as_usize());
            match continue_entry.action() {
                LocalAction::Local { label, .. } => assert_eq!(label, LABEL_LOOP_CONTINUE),
                other => panic!("expected continue local action, got {other:?}"),
            }
            let continue_after = typestate.node(continue_entry.next().as_usize());
            match continue_after.action() {
                LocalAction::Send { label, .. } => assert_eq!(label, 7),
                other => panic!("expected loop body send after continue, got {other:?}"),
            }

            let (break_entry_idx, break_label) = controller_compiled
                .controller_arm_entry_by_arm(scope_id, 1)
                .expect("break arm entry");
            assert_eq!(break_label, LABEL_LOOP_BREAK);
            let break_entry = typestate.node(break_entry_idx.as_usize());
            match break_entry.action() {
                LocalAction::Local { label, .. } => assert_eq!(label, LABEL_LOOP_BREAK),
                other => panic!("expected break local action, got {other:?}"),
            }
            let break_jump = typestate.node(break_entry.next().as_usize());
            assert!(
                break_jump.action().is_jump(),
                "break branch should advance into LoopBreak jump"
            );
            let break_terminal = typestate.node(break_jump.next().as_usize());
            assert!(
                break_terminal.action().is_terminal(),
                "LoopBreak jump should reach terminal"
            );
        });

        with_compiled_role_image(&TARGET_PROGRAM, |target_compiled| {
            let ts = target_compiled.typestate_ref();
            let target_first = ts.node(0);
            match target_first.action() {
                LocalAction::Recv { label, .. } => assert_eq!(label, 7),
                other => panic!("target should start at loop body recv, got {other:?}"),
            }
            let after_body = target_first.next().as_usize();
            let cursor = ts.node(after_body);

            assert!(
                cursor.action().is_jump(),
                "after Recv should be arm 0 PassiveObserverBranch Jump"
            );
            assert_eq!(
                cursor.next(),
                StateIndex::ZERO,
                "arm 0 should jump to loop start"
            );

            let arm1_idx = after_body + 1;
            if arm1_idx < ts.len() {
                let arm1_node = ts.node(arm1_idx);
                if arm1_node.action().is_jump() {
                    let arm1_target = arm1_node.next();
                    let target_idx = arm1_target.as_usize();
                    if target_idx < ts.len() {
                        let terminal_node = ts.node(target_idx);
                        assert!(
                            terminal_node.action().is_terminal(),
                            "arm 1 Break Jump should reach terminal"
                        );
                    }
                }
            }
        });
    }

    #[test]
    fn route_scope_kind_detected() {
        // Route is local to Controller (0 → 0)
        const ROUTE: g::Program<RouteScopeProgramSteps> = g::route(
            g::send::<
                Role<0>,
                Role<0>,
                Msg<
                    { LABEL_LOOP_CONTINUE },
                    GenericCapToken<LoopContinueKind>,
                    CanonicalControl<LoopContinueKind>,
                >,
                0,
            >()
            .policy::<ROUTE_POLICY_ID>(),
            g::send::<
                Role<0>,
                Role<0>,
                Msg<
                    { LABEL_LOOP_BREAK },
                    GenericCapToken<LoopBreakKind>,
                    CanonicalControl<LoopBreakKind>,
                >,
                0,
            >()
            .policy::<ROUTE_POLICY_ID>(),
        );

        const CONTROLLER: RoleProgram<'static, 0> = project(&ROUTE);

        with_compiled_role_image(&CONTROLLER, |compiled| {
            let summary = ROUTE.summary();
            let compiled_program = CompiledProgram::from_summary(&summary);
            let typestate = compiled.typestate_ref();
            let scope_id = typestate.node(0).scope();
            let region = typestate
                .scope_region_for(scope_id)
                .expect("route scope region present");
            assert_eq!(region.kind, ScopeKind::Route);
            let (policy, eff_index, _) = compiled_program
                .route_controller(scope_id)
                .expect("controller policy recorded");
            let expected_policy = PolicyMode::dynamic(ROUTE_POLICY_ID).with_scope(scope_id);
            assert_eq!(policy, expected_policy);
            assert_ne!(eff_index, EffIndex::MAX);
        });
    }

    #[test]
    fn nested_route_scope_caches_parallel_root() {
        type ParallelRouteProgramSteps = ParSteps<
            RouteScopeProgramSteps,
            StepCons<SendStep<Role<1>, Role<1>, Msg<11, ()>>, StepNil>,
        >;

        const PARALLEL_ROUTE_PROGRAM: g::Program<ParallelRouteProgramSteps> = g::par(
            g::route(
                g::send::<
                    Role<0>,
                    Role<0>,
                    Msg<
                        { LABEL_LOOP_CONTINUE },
                        GenericCapToken<LoopContinueKind>,
                        CanonicalControl<LoopContinueKind>,
                    >,
                    0,
                >()
                .policy::<ROUTE_POLICY_ID>(),
                g::send::<
                    Role<0>,
                    Role<0>,
                    Msg<
                        { LABEL_LOOP_BREAK },
                        GenericCapToken<LoopBreakKind>,
                        CanonicalControl<LoopBreakKind>,
                    >,
                    0,
                >()
                .policy::<ROUTE_POLICY_ID>(),
            ),
            g::send::<Role<1>, Role<1>, Msg<11, ()>, 0>(),
        );
        const PARALLEL_CONTROLLER: RoleProgram<'static, 0> = project(&PARALLEL_ROUTE_PROGRAM);

        with_compiled_role_image(&PARALLEL_CONTROLLER, |compiled| {
            let typestate = compiled.typestate_ref();
            let mut route_scope = None;
            let mut idx = 0usize;
            while idx < typestate.len() {
                let scope = typestate.node(idx).scope();
                if !scope.is_none()
                    && let Some(region) = typestate.scope_region_for(scope)
                    && region.kind == ScopeKind::Route
                    && typestate.parallel_root(scope).is_some()
                {
                    route_scope = Some(scope);
                    break;
                }
                idx += 1;
            }

            let route_scope = route_scope.expect("nested route scope under parallel must exist");
            let parallel_scope = typestate
                .parallel_root(route_scope)
                .expect("parallel root must be cached on nested route scope");
            let parallel_region = typestate
                .scope_region_for(parallel_scope)
                .expect("parallel scope region present");

            assert_eq!(parallel_region.kind, ScopeKind::Parallel);
            assert_eq!(typestate.scope_parent(route_scope), Some(parallel_scope));
        });
    }

    #[test]
    fn nested_route_scope_caches_enclosing_loop() {
        type NestedLoopProgramSteps =
            SeqSteps<StepCons<SendStep<Role<0>, Role<1>, Msg<13, ()>>, StepNil>, LoopProgramSteps>;

        const NESTED_LOOP_PROGRAM: g::Program<NestedLoopProgramSteps> =
            g::seq(g::send::<Role<0>, Role<1>, Msg<13, ()>, 0>(), LOOP_PROGRAM);
        const NESTED_LOOP_CONTROLLER: RoleProgram<'static, 0> = project(&NESTED_LOOP_PROGRAM);

        with_compiled_role_image(&NESTED_LOOP_CONTROLLER, |compiled| {
            let typestate = compiled.typestate_ref();
            let mut nested_route_scope = None;
            let mut idx = 0usize;
            while idx < typestate.len() {
                let scope = typestate.node(idx).scope();
                if !scope.is_none()
                    && let Some(region) = typestate.scope_region_for(scope)
                    && region.kind == ScopeKind::Route
                    && let Some(parent) = typestate.scope_parent(scope)
                    && let Some(parent_region) = typestate.scope_region_for(parent)
                    && parent_region.kind == ScopeKind::Loop
                {
                    nested_route_scope = Some((scope, parent));
                    break;
                }
                idx += 1;
            }

            let (route_scope, loop_scope) =
                nested_route_scope.expect("nested route scope under loop must exist");
            assert_eq!(typestate.enclosing_loop(route_scope), Some(loop_scope));
            assert_eq!(typestate.control_parent(route_scope), Some(loop_scope));
        });
    }

    #[test]
    fn nested_route_scope_caches_route_parent_arm() {
        let outer_route_program = {
            let inner = g::route(
                g::send::<
                    Role<0>,
                    Role<0>,
                    Msg<
                        { LABEL_LOOP_CONTINUE },
                        GenericCapToken<LoopContinueKind>,
                        CanonicalControl<LoopContinueKind>,
                    >,
                    0,
                >()
                .policy::<ROUTE_POLICY_ID>(),
                g::send::<
                    Role<0>,
                    Role<0>,
                    Msg<
                        { LABEL_LOOP_BREAK },
                        GenericCapToken<LoopBreakKind>,
                        CanonicalControl<LoopBreakKind>,
                    >,
                    0,
                >()
                .policy::<ROUTE_POLICY_ID>(),
            );
            let left = g::seq(g::send::<Role<0>, Role<0>, Msg<20, ()>, 0>(), inner);
            let right = g::send::<Role<0>, Role<0>, Msg<21, ()>, 0>();
            g::route(left, right)
        };
        let outer_controller: RoleProgram<'_, 0> = project(&outer_route_program);

        with_compiled_role_image(&outer_controller, |compiled| {
            let typestate = compiled.typestate_ref();
            let mut nested_route_scope = None;
            let mut idx = 0usize;
            while idx < typestate.len() {
                let scope = typestate.node(idx).scope();
                if !scope.is_none()
                    && let Some(parent) = typestate.route_parent(scope)
                {
                    nested_route_scope = Some((scope, parent));
                    break;
                }
                idx += 1;
            }

            let (nested_scope, outer_scope) =
                nested_route_scope.expect("nested route scope under parent route must exist");
            assert_eq!(typestate.route_parent(nested_scope), Some(outer_scope));
            assert_eq!(typestate.route_parent_arm(nested_scope), Some(0));
            assert!(typestate.control_parent(nested_scope).is_some());
        });
    }

    #[test]
    fn nested_route_first_recv_dispatch_summary_is_compiled() {
        let dispatch_program = {
            let inner = g::route(
                g::seq(
                    g::send::<Role<0>, Role<0>, Msg<0x61, ()>, 1>(),
                    g::send::<Role<0>, Role<1>, Msg<0x71, ()>, 1>(),
                ),
                g::seq(
                    g::send::<Role<0>, Role<0>, Msg<0x62, ()>, 0>(),
                    g::send::<Role<0>, Role<1>, Msg<0x72, ()>, 0>(),
                ),
            );
            g::route(
                g::seq(
                    g::send::<Role<0>, Role<0>, Msg<0x50, ()>, 0>(),
                    g::send::<Role<0>, Role<1>, Msg<0x53, ()>, 0>(),
                ),
                g::seq(g::send::<Role<0>, Role<0>, Msg<0x51, ()>, 0>(), inner),
            )
        };
        let dispatch_worker: RoleProgram<'_, 1> = project(&dispatch_program);

        with_compiled_role_image(&dispatch_worker, |compiled| {
            let typestate = compiled.typestate_ref();
            let outer_scope = typestate.node(0).scope();
            assert!(
                !outer_scope.is_none(),
                "worker must enter at outer route scope"
            );

            let mut nested_scope = None;
            let mut idx = 0usize;
            while idx < typestate.len() {
                let scope = typestate.node(idx).scope();
                if !scope.is_none() && typestate.route_parent(scope) == Some(outer_scope) {
                    nested_scope = Some(scope);
                    break;
                }
                idx += 1;
            }
            let nested_scope = nested_scope.expect("worker must see nested route scope");

            assert_eq!(
                typestate
                    .first_recv_dispatch_target_for_label(outer_scope, 0x53)
                    .map(|(arm, _)| arm),
                Some(0)
            );
            assert_eq!(
                typestate
                    .first_recv_dispatch_target_for_label(outer_scope, 0x71)
                    .map(|(arm, _)| arm),
                Some(1)
            );
            assert_eq!(typestate.first_recv_dispatch_arm_mask(outer_scope), 0b11);
            assert_eq!(
                typestate.first_recv_dispatch_lane_mask(outer_scope, 0),
                1u8 << 0
            );
            assert_eq!(
                typestate.first_recv_dispatch_lane_mask(outer_scope, 1),
                (1u8 << 0) | (1u8 << 1)
            );
            assert_eq!(
                typestate.first_recv_dispatch_arm_label_mask(outer_scope, 0),
                1u128 << 0x53
            );
            assert_eq!(
                typestate.first_recv_dispatch_arm_label_mask(outer_scope, 1),
                (1u128 << 0x71) | (1u128 << 0x72)
            );
            assert_eq!(
                typestate
                    .first_recv_dispatch_target_for_label(nested_scope, 0x71)
                    .map(|(arm, _)| arm),
                Some(0)
            );
            assert_eq!(typestate.first_recv_dispatch_arm_mask(nested_scope), 0b11);
        });
    }

    #[test]
    fn local_action_produces_metadata() {
        with_compiled_role_image(&LOCAL_ROLE, |compiled| {
            let typestate = compiled.typestate_ref();
            let first = typestate.node(0);
            assert!(first.action().is_local_action());
            match first.action() {
                LocalAction::Local { label, .. } => {
                    assert_eq!(label, <Msg<9, ()> as MessageSpec>::LABEL);
                }
                other => panic!("expected local action, got {other:?}"),
            }

            let next = super::state_index_to_usize(first.next());
            assert!(matches!(
                typestate.node(next).action(),
                LocalAction::Terminate
            ));
        });
    }
}
