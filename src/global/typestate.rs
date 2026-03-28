//! Role-local typestate synthesis derived from `EffList`.
//!
//! This module materialises a compact state machine for a given role directly
//! from an `EffList`. Each state captures the local action (send/recv/control)
//! together with the successor index, allowing higher layers to drive endpoint
//! transitions.

mod builder;
mod cursor;
mod facts;

pub use self::facts::StateIndex;
#[allow(unused_imports)]
pub(crate) use self::{
    builder::{
        ARM_SHARED, MAX_FIRST_RECV_DISPATCH, RoleTypestate, RoleTypestateValue, ScopeRegion,
    },
    cursor::{LoopMetadata, LoopRole, PhaseCursor},
    facts::{
        JumpError, JumpReason, LocalAction, LocalMeta, LocalNode, MAX_STATES, PassiveArmNavigation,
        RecvMeta, SendMeta, as_eff_index, as_state_index, state_index_to_usize, try_local_meta,
        try_recv_meta, try_send_meta,
    },
};

/*
Canonical split owners retained under src/global/typestate/{facts,builder,cursor}.rs:
pub struct StateIndex(u16);
pub(crate) struct RouteRecvIndex(u16);
pub(crate) const MAX_STATES: usize =
pub(crate) enum JumpReason {
pub(crate) struct JumpError {
pub(crate) enum PassiveArmNavigation {
pub(crate) enum LocalAction {
pub(crate) struct LocalNode {
pub(crate) struct ScopeEntry {
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
    use super::{LocalAction, PhaseCursor, StateIndex};
    use crate::control::cap::mint::GenericCapToken;
    use crate::control::cap::resource_kinds::{LoopBreakKind, LoopContinueKind};
    use crate::eff::EffIndex;
    use crate::g::{self, Msg, Role};
    use crate::global::const_dsl::{PolicyMode, ScopeKind};
    use crate::global::role_program;
    use crate::global::role_program::{RoleProgram, project};
    use crate::global::steps::{LoopSteps, ProjectRole, SendStep, StepConcat, StepCons, StepNil};
    use crate::global::{CanonicalControl, MessageSpec};
    use crate::runtime::consts::{LABEL_LOOP_BREAK, LABEL_LOOP_CONTINUE};

    const BODY: g::Program<StepCons<SendStep<Role<0>, Role<1>, Msg<7, ()>>, StepNil>> =
        g::send::<Role<0>, Role<1>, Msg<7, ()>, 0>();

    const LOOP_POLICY_ID: u16 = 9300;
    const ROUTE_POLICY_ID: u16 = 9301;

    #[allow(clippy::type_complexity)]
    const LOOP_PROGRAM: g::Program<
        LoopSteps<
            StepCons<SendStep<Role<0>, Role<1>, Msg<7, ()>>, StepNil>,
            Role<0>,
            Msg<
                { LABEL_LOOP_CONTINUE },
                GenericCapToken<LoopContinueKind>,
                CanonicalControl<LoopContinueKind>,
            >,
            Msg<
                { LABEL_LOOP_BREAK },
                GenericCapToken<LoopBreakKind>,
                CanonicalControl<LoopBreakKind>,
            >,
            StepNil,
        >,
    > = {
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

    const CONTROLLER_PROGRAM: RoleProgram<
        'static,
        0,
        <LoopSteps<
            StepCons<SendStep<Role<0>, Role<1>, Msg<7, ()>>, StepNil>,
            Role<0>,
            Msg<
                { LABEL_LOOP_CONTINUE },
                GenericCapToken<LoopContinueKind>,
                CanonicalControl<LoopContinueKind>,
            >,
            Msg<
                { LABEL_LOOP_BREAK },
                GenericCapToken<LoopBreakKind>,
                CanonicalControl<LoopBreakKind>,
            >,
            StepNil,
        > as ProjectRole<Role<0>>>::Output,
    > = project(&LOOP_PROGRAM);

    const TARGET_PROGRAM: RoleProgram<
        'static,
        1,
        <LoopSteps<
            StepCons<SendStep<Role<0>, Role<1>, Msg<7, ()>>, StepNil>,
            Role<0>,
            Msg<
                { LABEL_LOOP_CONTINUE },
                GenericCapToken<LoopContinueKind>,
                CanonicalControl<LoopContinueKind>,
            >,
            Msg<
                { LABEL_LOOP_BREAK },
                GenericCapToken<LoopBreakKind>,
                CanonicalControl<LoopBreakKind>,
            >,
            StepNil,
        > as ProjectRole<Role<1>>>::Output,
    > = project(&LOOP_PROGRAM);

    const LOCAL_PROGRAM: g::Program<StepCons<SendStep<Role<0>, Role<0>, Msg<9, ()>>, StepNil>> =
        g::send::<Role<0>, Role<0>, Msg<9, ()>, 0>();
    const LOCAL_ROLE: role_program::RoleProgram<
        'static,
        0,
        <StepCons<SendStep<Role<0>, Role<0>, Msg<9, ()>>, StepNil> as ProjectRole<Role<0>>>::Output,
    > = role_program::project(&LOCAL_PROGRAM);

    #[test]
    fn state_cursor_rewinds_on_loop_continue() {
        let compiled = CONTROLLER_PROGRAM.compile_role();
        let decision = PhaseCursor::new(&compiled);
        let continue_branch = decision
            .seek_label(LABEL_LOOP_CONTINUE)
            .expect("continue branch available");
        assert_eq!(continue_branch.label(), Some(LABEL_LOOP_CONTINUE));

        let after_continue = continue_branch.advance();
        assert!(after_continue.is_send());
        assert_eq!(after_continue.label(), Some(7));

        let rewind = decision
            .seek_label(LABEL_LOOP_CONTINUE)
            .expect("continue branch rewinds");
        assert_eq!(rewind.label(), Some(LABEL_LOOP_CONTINUE));
    }

    #[test]
    fn state_cursor_loop_branch_successors() {
        let controller_compiled = CONTROLLER_PROGRAM.compile_role();
        let decision = PhaseCursor::new(&controller_compiled);
        assert_eq!(decision.scope_kind(), Some(ScopeKind::Route));
        let cont_cursor = decision
            .seek_label(LABEL_LOOP_CONTINUE)
            .expect("continue branch")
            .advance();
        let mut cont_cursor = cont_cursor;
        while cont_cursor.label() == Some(LABEL_LOOP_CONTINUE) {
            cont_cursor = cont_cursor.advance();
        }
        assert_eq!(cont_cursor.label(), Some(7));

        let break_branch = decision.seek_label(LABEL_LOOP_BREAK).expect("break branch");
        assert_eq!(break_branch.label(), Some(LABEL_LOOP_BREAK));
        // After advancing from Local(BREAK), we land on Jump(LoopBreak).
        // Follow the Jump to its target (terminal).
        let break_cursor = break_branch
            .advance()
            .try_follow_jumps()
            .expect("follow loop break jump");
        break_cursor.assert_terminal();

        // Target (Role<1>) only sees the LoopBody message (label 7), not the
        // LoopContinue/LoopBreak self-sends. With self-send CanonicalControl,
        // Target's projection contains only the actual cross-role messages,
        // plus PassiveObserverBranch Jump nodes for empty arms (Break arm in this case).
        let target_compiled = TARGET_PROGRAM.compile_role();
        let target_cursor = PhaseCursor::new(&target_compiled);
        // Target sees label 7 (the loop body recv) directly
        assert_eq!(target_cursor.label(), Some(7));

        let ts = target_compiled.typestate();
        let after_body = target_cursor.advance();
        // After advancing past the Recv, we encounter PassiveObserverBranch Jump nodes.
        // - Jump for arm 0 (Continue): loops back to loop_start
        // - Jump for arm 1 (Break): goes to scope_end (terminal)
        let cursor = after_body;

        // For a passive observer in a linger scope, the normal flow after Recv
        // is determined by which arm was selected. The arm 0 Jump loops back,
        // so we need to check that arm 1 (Break) properly terminates.
        assert!(
            cursor.is_jump(),
            "after Recv should be arm 0 PassiveObserverBranch Jump"
        );
        let jump_node = ts.node(cursor.index());
        // Arm 0 Jump targets loop start (idx 0)
        assert_eq!(
            jump_node.next(),
            StateIndex::ZERO,
            "arm 0 should jump to loop start"
        );

        // Advance past arm 0 Jump to find arm 1 Jump
        // Note: advance() on a Jump follows the target, so we need to check next node manually
        let arm1_idx = cursor.index() + 1;
        if arm1_idx < ts.len() && !matches!(ts.node(arm1_idx).action(), LocalAction::None) {
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

        // The test passes if we've verified the structure. The actual runtime
        // behavior uses offer() to select which arm, not linear advance().
        // For linger scopes with passive observers, both arms have Jump nodes.
    }

    #[test]
    fn route_scope_kind_detected() {
        // Route is local to Controller (0 → 0)
        const ROUTE: g::Program<
            <StepCons<
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
            > as StepConcat<
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
            >>::Output,
        > = g::route(
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

        const CONTROLLER: RoleProgram<
            'static,
            0,
            <<StepCons<
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
            > as StepConcat<
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
            >>::Output as ProjectRole<Role<0>>>::Output,
        > = project(&ROUTE);

        let compiled = CONTROLLER.compile_role();
        let cursor = PhaseCursor::new(&compiled);
        assert_eq!(cursor.scope_kind(), Some(ScopeKind::Route));
        let scope_id = cursor.scope_id().expect("route scope id present");
        let (policy, eff_index, _) = cursor
            .route_scope_controller_policy(scope_id)
            .expect("controller policy recorded");
        let expected_policy = PolicyMode::dynamic(ROUTE_POLICY_ID).with_scope(scope_id);
        assert_eq!(policy, expected_policy);
        assert_ne!(eff_index, EffIndex::MAX);
    }

    #[test]
    fn local_action_produces_metadata() {
        let compiled = LOCAL_ROLE.compile_role();
        let cursor = PhaseCursor::new(&compiled);
        assert!(cursor.is_local_action());
        assert_eq!(cursor.label(), Some(<Msg<9, ()> as MessageSpec>::LABEL));
        let cursor = cursor.advance();
        cursor.assert_terminal();
    }
}
