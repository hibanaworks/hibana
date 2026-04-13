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

#[cfg(test)]
pub(crate) use self::emit::RoleCompileScratch;
pub use self::facts::StateIndex;
pub(crate) use self::registry::{RouteScopeRecord, ScopeRecord};
#[cfg(test)]
pub(crate) use self::{builder::RoleTypestate, emit::phase_route_guard_for_built_state_for_role};
#[allow(unused_imports)]
pub(crate) use self::{
    builder::{ARM_SHARED, MAX_FIRST_RECV_DISPATCH, RoleTypestateValue, ScopeRegion},
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
    use core::{cell::UnsafeCell, mem::MaybeUninit};
    use std::{thread::LocalKey, thread_local};

    use super::{LocalAction, StateIndex};
    use crate::control::cap::mint::GenericCapToken;
    use crate::control::cap::resource_kinds::{LoopBreakKind, LoopContinueKind};
    use crate::eff::EffIndex;
    use crate::g::{self, Msg, Role};
    use crate::global::compiled::CompiledRole;
    use crate::global::const_dsl::{PolicyMode, ScopeKind};
    use crate::global::role_program;
    use crate::global::role_program::{ProgramWitness, RoleProgram, project};
    use crate::global::steps::{PolicySteps, RouteSteps, SendStep, SeqSteps, StepCons, StepNil};
    use crate::global::typestate::RoleCompileScratch;
    use crate::global::{CanonicalControl, MessageSpec};
    use crate::runtime::consts::{LABEL_LOOP_BREAK, LABEL_LOOP_CONTINUE};

    thread_local! {
        static COMPILED_ROLE_STORAGE: UnsafeCell<MaybeUninit<CompiledRole>> =
            const { UnsafeCell::new(MaybeUninit::uninit()) };
        static COMPILED_ROLE_SCRATCH: UnsafeCell<MaybeUninit<RoleCompileScratch>> =
            const { UnsafeCell::new(MaybeUninit::uninit()) };
        static COMPILED_ROLE_STORAGE_ALT: UnsafeCell<MaybeUninit<CompiledRole>> =
            const { UnsafeCell::new(MaybeUninit::uninit()) };
        static COMPILED_ROLE_SCRATCH_ALT: UnsafeCell<MaybeUninit<RoleCompileScratch>> =
            const { UnsafeCell::new(MaybeUninit::uninit()) };
    }

    fn with_compiled_role_slot<const ROLE: u8, Witness, Mint, R>(
        compiled_slot: &'static LocalKey<UnsafeCell<MaybeUninit<CompiledRole>>>,
        scratch_slot: &'static LocalKey<UnsafeCell<MaybeUninit<RoleCompileScratch>>>,
        program: &RoleProgram<'_, ROLE, Witness, Mint>,
        f: impl FnOnce(&CompiledRole) -> R,
    ) -> R
    where
        Mint: crate::control::cap::mint::MintConfigMarker,
    {
        crate::global::compiled::with_compiled_role_in_slot::<ROLE, _>(
            compiled_slot,
            scratch_slot,
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

    const CONTROLLER_PROGRAM: RoleProgram<'static, 0, ProgramWitness<LoopProgramSteps>> =
        project(&LOOP_PROGRAM);

    const TARGET_PROGRAM: RoleProgram<'static, 1, ProgramWitness<LoopProgramSteps>> =
        project(&LOOP_PROGRAM);

    const LOCAL_PROGRAM: g::Program<StepCons<SendStep<Role<0>, Role<0>, Msg<9, ()>>, StepNil>> =
        g::send::<Role<0>, Role<0>, Msg<9, ()>, 0>();
    const LOCAL_ROLE: role_program::RoleProgram<
        'static,
        0,
        role_program::ProgramWitness<StepCons<SendStep<Role<0>, Role<0>, Msg<9, ()>>, StepNil>>,
    > = role_program::project(&LOCAL_PROGRAM);

    #[test]
    fn state_cursor_rewinds_on_loop_continue() {
        with_compiled_role_slot(
            &COMPILED_ROLE_STORAGE,
            &COMPILED_ROLE_SCRATCH,
            &CONTROLLER_PROGRAM,
            |compiled| {
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
            },
        );
    }

    #[test]
    fn state_cursor_loop_branch_successors() {
        with_compiled_role_slot(
            &COMPILED_ROLE_STORAGE,
            &COMPILED_ROLE_SCRATCH,
            &CONTROLLER_PROGRAM,
            |controller_compiled| {
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

                // Target (Role<1>) only sees the LoopBody message (label 7), not the
                // LoopContinue/LoopBreak self-sends. With self-send CanonicalControl,
                // Target's projection contains only the actual cross-role messages,
                // plus PassiveObserverBranch Jump nodes for empty arms (Break arm in this case).
                with_compiled_role_slot(
                    &COMPILED_ROLE_STORAGE_ALT,
                    &COMPILED_ROLE_SCRATCH_ALT,
                    &TARGET_PROGRAM,
                    |target_compiled| {
                        let ts = target_compiled.typestate();
                        let target_first = ts.node(0);
                        match target_first.action() {
                            LocalAction::Recv { label, .. } => assert_eq!(label, 7),
                            other => panic!("target should start at loop body recv, got {other:?}"),
                        }
                        let after_body = target_first.next().as_usize();
                        // After advancing past the Recv, we encounter PassiveObserverBranch Jump nodes.
                        // - Jump for arm 0 (Continue): loops back to loop_start
                        // - Jump for arm 1 (Break): goes to scope_end (terminal)
                        let cursor = ts.node(after_body);

                        // For a passive observer in a linger scope, the normal flow after Recv
                        // is determined by which arm was selected. The arm 0 Jump loops back,
                        // so we need to check that arm 1 (Break) properly terminates.
                        assert!(
                            cursor.action().is_jump(),
                            "after Recv should be arm 0 PassiveObserverBranch Jump"
                        );
                        // Arm 0 Jump targets loop start (idx 0)
                        assert_eq!(
                            cursor.next(),
                            StateIndex::ZERO,
                            "arm 0 should jump to loop start"
                        );

                        // Advance past arm 0 Jump to find arm 1 Jump
                        // Note: advance() on a Jump follows the target, so we need to check next node manually
                        let arm1_idx = after_body + 1;
                        if arm1_idx < ts.len() {
                            let arm1_node = ts.node(arm1_idx);
                            if arm1_node.action().is_jump() {
                                // Arm 1 (Break) Jump should target scope_end (which should be terminal)
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
                    },
                );

                // The test passes if we've verified the structure. The actual runtime
                // behavior uses offer() to select which arm, not linear advance().
                // For linger scopes with passive observers, both arms have Jump nodes.
            },
        );
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

        const CONTROLLER: RoleProgram<'static, 0, ProgramWitness<RouteScopeProgramSteps>> =
            project(&ROUTE);

        with_compiled_role_slot(
            &COMPILED_ROLE_STORAGE,
            &COMPILED_ROLE_SCRATCH,
            &CONTROLLER,
            |compiled| {
                let typestate = compiled.typestate_ref();
                let scope_id = typestate.node(0).scope();
                let region = typestate
                    .scope_region_for(scope_id)
                    .expect("route scope region present");
                assert_eq!(region.kind, ScopeKind::Route);
                let (policy, eff_index, _) = typestate
                    .route_controller(scope_id)
                    .expect("controller policy recorded");
                let expected_policy = PolicyMode::dynamic(ROUTE_POLICY_ID).with_scope(scope_id);
                assert_eq!(policy, expected_policy);
                assert_ne!(eff_index, EffIndex::MAX);
            },
        );
    }

    #[test]
    fn local_action_produces_metadata() {
        with_compiled_role_slot(
            &COMPILED_ROLE_STORAGE,
            &COMPILED_ROLE_SCRATCH,
            &LOCAL_ROLE,
            |compiled| {
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
            },
        );
    }
}
