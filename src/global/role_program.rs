//! Role-local program representation derived from const `EffList`.
//!
//! `RoleProgram` is the typed entry point for a role-local projection.
//! Crate-private lowering facts stay behind this module and the compiled layer,
//! while the original `EffList` reference remains available for metadata
//! inspection.

use super::{
    compiled::{ProgramFacts, RoleMachine},
    program::Program,
    steps::ProjectRole,
};
#[cfg(test)]
use super::typestate::RoleTypestate;
use crate::control::cap::mint::{CapShot, MintConfig, MintConfigMarker};
use crate::{
    eff::{self, EffIndex},
    global::const_dsl::{EffList, ScopeId},
    global::{KnownRole, Role},
};

pub(super) const MAX_STEPS: usize = eff::meta::MAX_EFF_NODES;
/// Maximum number of parallel phases in a program.
pub(super) const MAX_PHASES: usize = 32;
/// Maximum number of concurrent lanes (matches RoleLaneSet::LANE_COUNT).
pub(crate) const MAX_LANES: usize = 8;

/// Steps for a single lane within a phase.
///
/// References a contiguous slice of `LocalStep` entries within the projected
/// role-local step array. Empty lanes have `len == 0`.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct LaneSteps {
    /// Start offset into the RoleProgram's `local_steps` array.
    pub start: usize,
    /// Number of steps in this lane.
    pub len: usize,
}

impl LaneSteps {
    pub const EMPTY: Self = Self { start: 0, len: 0 };

    /// Whether this lane has any steps.
    #[inline(always)]
    pub const fn is_active(&self) -> bool {
        self.len > 0
    }
}

/// Route arm guard for a phase (outermost route scope only).
#[derive(Clone, Copy, Debug)]
pub(crate) struct PhaseRouteGuard {
    pub scope: ScopeId,
    pub arm: u8,
}

impl PhaseRouteGuard {
    pub const EMPTY: Self = Self {
        scope: ScopeId::none(),
        arm: 0,
    };

    #[inline(always)]
    pub const fn is_empty(&self) -> bool {
        self.scope.is_none()
    }

    #[inline(always)]
    pub const fn matches(&self, other: Self) -> bool {
        self.scope.raw() == other.scope.raw() && self.arm == other.arm
    }
}

/// A phase represents a fork-join barrier in the program.
///
/// Within a phase, each active lane can proceed independently.
/// All lanes must complete before advancing to the next phase.
///
/// ```text
/// Phase 0 (Fork):
///   Lane 0: [A's steps...]  ─┬─→ Barrier
///   Lane 1: [B's steps...]  ─┘
///                              │
/// Phase 1 (Join):              ↓
///   Lane 0: [C's steps...]
/// ```
#[derive(Clone, Copy, Debug)]
pub(crate) struct Phase {
    /// Steps for each lane (up to MAX_LANES).
    /// Inactive lanes have `LaneSteps::EMPTY`.
    pub lanes: [LaneSteps; MAX_LANES],
    /// Active lanes for this phase as a bitmask.
    pub lane_mask: u8,
    /// Minimum start index across active lanes, used for phase entry.
    pub min_start: usize,
    /// Outermost route scope arm guard for this phase (if any).
    pub route_guard: PhaseRouteGuard,
}

impl Default for Phase {
    fn default() -> Self {
        Self::EMPTY
    }
}

impl Phase {
    pub const EMPTY: Self = Self {
        lanes: [LaneSteps::EMPTY; MAX_LANES],
        lane_mask: 0,
        min_start: 0,
        route_guard: PhaseRouteGuard::EMPTY,
    };
}

/// Local direction of a step in the projected program.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LocalDirection {
    /// Placeholder used for uninitialised entries.
    None,
    /// Role sends a message to another participant.
    Send,
    /// Role receives a message from another participant.
    Recv,
    /// Role performs a local action (self-send).
    Local,
}

/// Metadata describing a single local transition for a role.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LocalStep {
    eff_index: EffIndex,
    label: u8,
    peer: u8,
    resource: Option<u8>,
    direction: LocalDirection,
    is_control: bool,
    shot: Option<CapShot>,
    /// Type-level lane for parallel composition (default 0).
    lane: u8,
}

impl LocalStep {
    /// Empty placeholder used to prefill the backing array.
    pub const EMPTY: Self = Self {
        eff_index: EffIndex::ZERO,
        label: 0,
        peer: 0,
        resource: None,
        direction: LocalDirection::None,
        is_control: false,
        shot: None,
        lane: 0,
    };

    /// Construct a send step directed to `peer`.
    pub const fn send(
        eff_index: EffIndex,
        peer: u8,
        label: u8,
        resource: Option<u8>,
        is_control: bool,
        shot: Option<CapShot>,
        lane: u8,
    ) -> Self {
        Self {
            eff_index,
            label,
            peer,
            resource,
            direction: LocalDirection::Send,
            is_control,
            shot,
            lane,
        }
    }

    /// Construct a receive step originating from `peer`.
    pub const fn recv(
        eff_index: EffIndex,
        peer: u8,
        label: u8,
        resource: Option<u8>,
        is_control: bool,
        shot: Option<CapShot>,
        lane: u8,
    ) -> Self {
        Self {
            eff_index,
            label,
            peer,
            resource,
            direction: LocalDirection::Recv,
            is_control,
            shot,
            lane,
        }
    }

    /// Construct a local action step executed by the current role.
    pub const fn local(
        eff_index: EffIndex,
        peer: u8,
        label: u8,
        resource: Option<u8>,
        is_control: bool,
        shot: Option<CapShot>,
        lane: u8,
    ) -> Self {
        Self {
            eff_index,
            label,
            peer,
            resource,
            direction: LocalDirection::Local,
            is_control,
            shot,
            lane,
        }
    }

    /// Index of the originating `EffStruct` within the global program.
    #[inline(always)]
    pub const fn eff_index(&self) -> EffIndex {
        self.eff_index
    }

    /// Label associated with this transition.
    #[inline(always)]
    pub const fn label(&self) -> u8 {
        self.label
    }

    /// Remote role participating in this transition.
    #[inline(always)]
    pub const fn peer(&self) -> u8 {
        self.peer
    }

    /// True when this step is a send transition.
    #[inline(always)]
    pub const fn is_send(&self) -> bool {
        matches!(self.direction, LocalDirection::Send)
    }

    /// True when this step is a receive transition.
    #[inline(always)]
    pub const fn is_recv(&self) -> bool {
        matches!(self.direction, LocalDirection::Recv)
    }

    /// True when this step is a local action executed without transport.
    #[inline(always)]
    pub const fn is_local_action(&self) -> bool {
        matches!(self.direction, LocalDirection::Local)
    }

    /// Type-level lane for parallel composition.
    #[inline(always)]
    pub const fn lane(&self) -> u8 {
        self.lane
    }
}

/// Role-specific view over a global effect list.
///
/// ## Phased Multi-Lane Architecture
///
/// `RoleProgram` is the thin, typed owner of a role projection. Runtime
/// metadata such as local-step tables, phase splits, and typestate graphs are
/// materialized on demand so that stack-local `project(&program)` values remain
/// small and cheap to move.
#[derive(Clone, Copy)]
pub(crate) struct ProjectedRoleLayout {
    local_steps: [LocalStep; MAX_STEPS],
    local_len: usize,
    phases: [Phase; MAX_PHASES],
    phase_len: usize,
}

impl ProjectedRoleLayout {
    #[inline(always)]
    pub(super) const fn new(
        local_steps: [LocalStep; MAX_STEPS],
        local_len: usize,
        phases: [Phase; MAX_PHASES],
        phase_len: usize,
    ) -> Self {
        Self {
            local_steps,
            local_len,
            phases,
            phase_len,
        }
    }

    #[inline(always)]
    pub(crate) const fn len(&self) -> usize {
        self.local_len
    }

    #[inline(always)]
    pub(crate) fn steps(&self) -> &[LocalStep] {
        &self.local_steps[..self.local_len]
    }

    #[inline(always)]
    pub(crate) const fn phase_count(&self) -> usize {
        self.phase_len
    }

    #[inline(always)]
    pub(crate) fn phases(&self) -> &[Phase] {
        &self.phases[..self.phase_len]
    }
}

#[cfg(test)]
#[derive(Clone, Copy)]
pub(crate) struct ProjectedRoleData<const ROLE: u8> {
    layout: ProjectedRoleLayout,
    typestate: RoleTypestate<ROLE>,
}

#[cfg(test)]
impl<const ROLE: u8> ProjectedRoleData<ROLE> {
    #[inline(always)]
    pub(super) const fn new(layout: ProjectedRoleLayout, typestate: RoleTypestate<ROLE>) -> Self {
        Self { layout, typestate }
    }

    #[inline(always)]
    pub(crate) const fn len(&self) -> usize {
        self.layout.len()
    }

    #[inline(always)]
    pub(crate) fn steps(&self) -> &[LocalStep] {
        self.layout.steps()
    }

    #[inline(always)]
    pub(crate) const fn phase_count(&self) -> usize {
        self.layout.phase_count()
    }

    #[inline(always)]
    pub(crate) fn phases(&self) -> &[Phase] {
        self.layout.phases()
    }

    #[inline(always)]
    pub(crate) const fn typestate(&self) -> &RoleTypestate<ROLE> {
        &self.typestate
    }
}

#[derive(Clone, Copy)]
pub struct RoleProgram<'prog, const ROLE: u8, LocalSteps, Mint = MintConfig>
where
    Mint: MintConfigMarker,
{
    eff_list: &'prog EffList,
    mint: Mint,
    _local_steps: core::marker::PhantomData<LocalSteps>,
}

impl<'prog, const ROLE: u8, LocalSteps, Mint> RoleProgram<'prog, ROLE, LocalSteps, Mint>
where
    Mint: MintConfigMarker,
{
    const fn new(eff_list: &'prog EffList, mint: Mint) -> Self {
        Self {
            eff_list,
            mint,
            _local_steps: core::marker::PhantomData,
        }
    }

    #[inline(always)]
    pub(crate) const fn machine(&self) -> RoleMachine<ROLE> {
        RoleMachine::<ROLE>::from_eff_list(self.eff_list)
    }

    /// Borrow the underlying global effect list.
    #[inline(always)]
    pub const fn eff_list(&self) -> &'prog EffList {
        self.eff_list
    }

    /// Static LeaseGraph capacity requirements inferred from policy markers.
    #[inline(always)]
    pub(crate) const fn lease_budget(&self) -> crate::control::lease::planner::LeaseGraphBudget {
        ProgramFacts::from_eff_list(self.eff_list).lease_budget()
    }

    /// Mint configuration baked into the RoleProgram.
    #[inline(always)]
    pub(crate) const fn mint_config(&self) -> Mint {
        self.mint
    }
}

/// Project a typed program into the local view for `ROLE`.
pub const fn project<'prog, const ROLE: u8, Steps, Mint>(
    program: &'prog Program<Steps>,
) -> RoleProgram<'prog, ROLE, <Steps as ProjectRole<Role<ROLE>>>::Output, Mint>
where
    Role<ROLE>: KnownRole,
    Steps: ProjectRole<Role<ROLE>>,
    Mint: MintConfigMarker,
{
    let eff = program.eff_list();
    let _ = ProgramFacts::from_eff_list(eff);
    RoleMachine::<ROLE>::validate(eff);
    RoleProgram::new(eff, Mint::INSTANCE)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::cap::mint::{CapShot, GenericCapToken};
    use crate::control::cap::resource_kinds::CancelKind;
    use crate::g::{self, Msg, Role};
    use crate::global::CanonicalControl;
    use crate::global::const_dsl::{ScopeEvent, ScopeKind};
    use crate::global::steps::{self, ProjectRole, SeqSteps, StepConcat, StepCons, StepNil};

    const PROTOCOL: Program<
        SeqSteps<
            StepCons<steps::SendStep<Role<0>, Role<1>, Msg<1, ()>, 0>, StepNil>,
            StepCons<steps::SendStep<Role<1>, Role<0>, Msg<2, ()>, 0>, StepNil>,
        >,
    > = g::seq(
        g::send::<Role<0>, Role<1>, Msg<1, ()>, 0>(),
        g::send::<Role<1>, Role<0>, Msg<2, ()>, 0>(),
    );

    const ROLE_ZERO: RoleProgram<
        'static,
        0,
        <SeqSteps<
            StepCons<steps::SendStep<Role<0>, Role<1>, Msg<1, ()>, 0>, StepNil>,
            StepCons<steps::SendStep<Role<1>, Role<0>, Msg<2, ()>, 0>, StepNil>,
        > as ProjectRole<Role<0>>>::Output,
    > = project(&PROTOCOL);
    const ROLE_ONE: RoleProgram<
        'static,
        1,
        <SeqSteps<
            StepCons<steps::SendStep<Role<0>, Role<1>, Msg<1, ()>, 0>, StepNil>,
            StepCons<steps::SendStep<Role<1>, Role<0>, Msg<2, ()>, 0>, StepNil>,
        > as ProjectRole<Role<1>>>::Output,
    > = project(&PROTOCOL);

    // CancelMsg uses CanonicalControl which requires self-send (From == To)
    const CANCEL_PROGRAM: Program<
        StepCons<
            steps::SendStep<
                Role<0>,
                Role<0>,
                Msg<
                    { crate::runtime::consts::LABEL_CANCEL },
                    GenericCapToken<CancelKind>,
                    CanonicalControl<CancelKind>,
                >,
                0,
            >,
            StepNil,
        >,
    > = g::send::<
        Role<0>,
        Role<0>,
        Msg<
            { crate::runtime::consts::LABEL_CANCEL },
            GenericCapToken<CancelKind>,
            CanonicalControl<CancelKind>,
        >,
        0,
    >();

    const CANCEL_ROLE: RoleProgram<
        'static,
        0,
        <StepCons<
            steps::SendStep<
                Role<0>,
                Role<0>,
                Msg<
                    { crate::runtime::consts::LABEL_CANCEL },
                    GenericCapToken<CancelKind>,
                    CanonicalControl<CancelKind>,
                >,
                0,
            >,
            StepNil,
        > as ProjectRole<Role<0>>>::Output,
    > = project(&CANCEL_PROGRAM);

    const LOCAL_PROGRAM: Program<
        StepCons<steps::SendStep<Role<0>, Role<0>, Msg<5, ()>, 0>, StepNil>,
    > = g::send::<Role<0>, Role<0>, Msg<5, ()>, 0>();

    const LOCAL_ROLE: RoleProgram<
        'static,
        0,
        <StepCons<steps::SendStep<Role<0>, Role<0>, Msg<5, ()>, 0>, StepNil> as ProjectRole<
            Role<0>,
        >>::Output,
    > = project(&LOCAL_PROGRAM);

    const PREFIX_PROGRAM: Program<
        StepCons<steps::SendStep<Role<0>, Role<0>, Msg<6, ()>, 0>, StepNil>,
    > = g::send::<Role<0>, Role<0>, Msg<6, ()>, 0>();
    const MIDDLE_PROGRAM: Program<
        StepCons<steps::SendStep<Role<0>, Role<1>, Msg<7, ()>, 0>, StepNil>,
    > = g::send::<Role<0>, Role<1>, Msg<7, ()>, 0>();
    const APP_PROGRAM: Program<
        StepCons<steps::SendStep<Role<1>, Role<0>, Msg<8, ()>, 0>, StepNil>,
    > = g::send::<Role<1>, Role<0>, Msg<8, ()>, 0>();
    const CHAIN_PROGRAM: Program<
        SeqSteps<
            StepCons<steps::SendStep<Role<0>, Role<0>, Msg<6, ()>, 0>, StepNil>,
            SeqSteps<
                StepCons<steps::SendStep<Role<0>, Role<1>, Msg<7, ()>, 0>, StepNil>,
                StepCons<steps::SendStep<Role<1>, Role<0>, Msg<8, ()>, 0>, StepNil>,
            >,
        >,
    > = g::seq(PREFIX_PROGRAM, g::seq(MIDDLE_PROGRAM, APP_PROGRAM));
    const CHAIN_ROLE: RoleProgram<
        'static,
        0,
        <SeqSteps<
            StepCons<steps::SendStep<Role<0>, Role<0>, Msg<6, ()>, 0>, StepNil>,
            SeqSteps<
                StepCons<steps::SendStep<Role<0>, Role<1>, Msg<7, ()>, 0>, StepNil>,
                StepCons<steps::SendStep<Role<1>, Role<0>, Msg<8, ()>, 0>, StepNil>,
            >,
        > as ProjectRole<Role<0>>>::Output,
    > = project(&CHAIN_PROGRAM);

    const PING_PROGRAM: Program<
        StepCons<steps::SendStep<Role<0>, Role<1>, Msg<9, ()>, 0>, StepNil>,
    > = g::send::<Role<0>, Role<1>, Msg<9, ()>, 0>();
    const PONG_PROGRAM: Program<
        StepCons<steps::SendStep<Role<1>, Role<0>, Msg<10, ()>, 1>, StepNil>,
    > = g::send::<Role<1>, Role<0>, Msg<10, ()>, 1>();
    const PARALLEL_PROGRAM: Program<
        <StepCons<steps::SendStep<Role<0>, Role<1>, Msg<9, ()>, 0>, StepNil> as StepConcat<
            StepCons<steps::SendStep<Role<1>, Role<0>, Msg<10, ()>, 1>, StepNil>,
        >>::Output,
    > = g::par(PING_PROGRAM, PONG_PROGRAM);

    const LANE0_PROGRAM: Program<
        SeqSteps<
            StepCons<steps::SendStep<Role<0>, Role<1>, Msg<14, ()>, 0>, StepNil>,
            StepCons<steps::SendStep<Role<1>, Role<0>, Msg<15, ()>, 0>, StepNil>,
        >,
    > = g::seq(
        g::send::<Role<0>, Role<1>, Msg<14, ()>, 0>(),
        g::send::<Role<1>, Role<0>, Msg<15, ()>, 0>(),
    );
    const CONTINUE_ARM_PROGRAM: Program<
        SeqSteps<
            StepCons<steps::SendStep<Role<2>, Role<2>, Msg<11, ()>, 1>, StepNil>,
            StepCons<steps::SendStep<Role<2>, Role<1>, Msg<13, ()>, 1>, StepNil>,
        >,
    > = g::seq(
        g::send::<Role<2>, Role<2>, Msg<11, ()>, 1>().policy::<77>(),
        g::send::<Role<2>, Role<1>, Msg<13, ()>, 1>(),
    );
    const BREAK_ARM_PROGRAM: Program<
        StepCons<steps::SendStep<Role<2>, Role<2>, Msg<12, ()>, 1>, StepNil>,
    > = g::send::<Role<2>, Role<2>, Msg<12, ()>, 1>().policy::<77>();
    const PARALLEL_ROUTE_PROGRAM: Program<
        <SeqSteps<
            StepCons<steps::SendStep<Role<0>, Role<1>, Msg<14, ()>, 0>, StepNil>,
            StepCons<steps::SendStep<Role<1>, Role<0>, Msg<15, ()>, 0>, StepNil>,
        > as StepConcat<
            <SeqSteps<
                StepCons<steps::SendStep<Role<2>, Role<2>, Msg<11, ()>, 1>, StepNil>,
                StepCons<steps::SendStep<Role<2>, Role<1>, Msg<13, ()>, 1>, StepNil>,
            > as StepConcat<
                StepCons<steps::SendStep<Role<2>, Role<2>, Msg<12, ()>, 1>, StepNil>,
            >>::Output,
        >>::Output,
    > = g::par(
        LANE0_PROGRAM,
        g::route(CONTINUE_ARM_PROGRAM, BREAK_ARM_PROGRAM),
    );

    #[test]
    fn projection_extracts_role_view() {
        let role_zero = ROLE_ZERO.machine().into_projection();
        assert_eq!(role_zero.len(), 2);
        assert!(role_zero.steps()[0].is_send());
        assert!(role_zero.steps()[1].is_recv());
        assert_eq!(role_zero.steps()[0].peer(), 1);
        assert_eq!(role_zero.steps()[1].peer(), 1);

        let role_one = ROLE_ONE.machine().into_projection();
        assert_eq!(role_one.len(), 2);
        assert!(role_one.steps()[0].is_recv());
        assert!(role_one.steps()[1].is_send());

        assert_eq!(
            ROLE_ZERO.eff_list().control_markers().len(),
            PROTOCOL.eff_list().control_markers().len()
        );
        assert_eq!(
            ROLE_ONE.eff_list().scope_markers().len(),
            PROTOCOL.eff_list().scope_markers().len()
        );

        let ts_zero = role_zero.typestate();
        assert_eq!(ts_zero.len(), 3);
        assert!(matches!(
            ts_zero.node(0).action(),
            super::super::typestate::LocalAction::Send { .. }
        ));
        assert!(matches!(
            ts_zero.node(1).action(),
            super::super::typestate::LocalAction::Recv { .. }
        ));
        assert!(ts_zero.node(2).action().is_terminal());

        let ts_one = role_one.typestate();
        assert_eq!(ts_one.len(), 3);
        assert!(matches!(
            ts_one.node(0).action(),
            super::super::typestate::LocalAction::Recv { .. }
        ));
        assert!(matches!(
            ts_one.node(1).action(),
            super::super::typestate::LocalAction::Send { .. }
        ));
        assert!(ts_one.node(2).action().is_terminal());
    }

    #[test]
    fn control_step_carries_shot_metadata() {
        // CancelMsg is a self-send (Client→Client), which projects to LocalAction
        let cancel_role = CANCEL_ROLE.machine().into_projection();
        assert_eq!(cancel_role.len(), 1);
        let step = cancel_role.steps()[0];
        assert!(step.is_local_action());
        assert!(step.is_control);
        assert_eq!(step.shot, Some(CapShot::One));
    }

    #[test]
    fn local_action_projects_as_local_step() {
        let local_role = LOCAL_ROLE.machine().into_projection();
        assert_eq!(local_role.len(), 1);
        let step = local_role.steps()[0];
        assert!(step.is_local_action());
        let ts = local_role.typestate();
        assert_eq!(ts.len(), 2);
        assert!(matches!(
            ts.node(0).action(),
            super::super::typestate::LocalAction::Local { .. }
        ));
        assert!(ts.node(1).action().is_terminal());
    }

    #[test]
    fn chained_projection_preserves_typed_local_steps() {
        let chain_role = CHAIN_ROLE.machine().into_projection();
        assert_eq!(chain_role.len(), 3);
        assert!(chain_role.steps()[0].is_local_action());
        assert!(chain_role.steps()[1].is_send());
        assert!(chain_role.steps()[2].is_recv());
        assert_eq!(chain_role.steps()[1].peer(), 1);
        assert_eq!(chain_role.steps()[2].peer(), 1);
    }

    #[test]
    fn parallel_projection_keeps_phase_and_lane_split_internal() {
        let client: RoleProgram<
            '_,
            0,
            <<StepCons<steps::SendStep<Role<0>, Role<1>, Msg<9, ()>, 0>, StepNil> as StepConcat<
                StepCons<steps::SendStep<Role<1>, Role<0>, Msg<10, ()>, 1>, StepNil>,
            >>::Output as ProjectRole<Role<0>>>::Output,
            MintConfig,
        > = project(&PARALLEL_PROGRAM);
        let server: RoleProgram<
            '_,
            1,
            <<StepCons<steps::SendStep<Role<0>, Role<1>, Msg<9, ()>, 0>, StepNil> as StepConcat<
                StepCons<steps::SendStep<Role<1>, Role<0>, Msg<10, ()>, 1>, StepNil>,
            >>::Output as ProjectRole<Role<1>>>::Output,
            MintConfig,
        > = project(&PARALLEL_PROGRAM);

        let client_projection = client.machine().into_projection();
        let server_projection = server.machine().into_projection();

        assert_eq!(client_projection.phase_count(), 1);
        assert_eq!(server_projection.phase_count(), 1);

        let client_phase = client_projection.phases()[0];
        assert!(client_phase.lanes[0].is_active());
        assert!(client_phase.lanes[1].is_active());

        let server_phase = server_projection.phases()[0];
        assert!(server_phase.lanes[0].is_active());
        assert!(server_phase.lanes[1].is_active());

        let client_lane0 = client_projection
            .steps()
            .iter()
            .filter(|step| step.lane() == 0)
            .count();
        let client_lane1 = client_projection
            .steps()
            .iter()
            .filter(|step| step.lane() == 1)
            .count();
        assert_eq!(client_lane0, 1);
        assert_eq!(client_lane1, 1);

        let server_lane0 = server_projection
            .steps()
            .iter()
            .filter(|step| step.lane() == 0)
            .count();
        let server_lane1 = server_projection
            .steps()
            .iter()
            .filter(|step| step.lane() == 1)
            .count();
        assert_eq!(server_lane0, 1);
        assert_eq!(server_lane1, 1);
    }

    #[test]
    fn parallel_route_projection_keeps_scope_markers_without_public_step_surface() {
        let program: RoleProgram<
            '_,
            0,
            <<SeqSteps<
                StepCons<steps::SendStep<Role<0>, Role<1>, Msg<14, ()>, 0>, StepNil>,
                StepCons<steps::SendStep<Role<1>, Role<0>, Msg<15, ()>, 0>, StepNil>,
            > as StepConcat<
                <SeqSteps<
                    StepCons<steps::SendStep<Role<2>, Role<2>, Msg<11, ()>, 1>, StepNil>,
                    StepCons<steps::SendStep<Role<2>, Role<1>, Msg<13, ()>, 1>, StepNil>,
                > as StepConcat<
                    StepCons<steps::SendStep<Role<2>, Role<2>, Msg<12, ()>, 1>, StepNil>,
                >>::Output,
            >>::Output as ProjectRole<Role<0>>>::Output,
            MintConfig,
        > = project(&PARALLEL_ROUTE_PROGRAM);
        let eff_list = program.eff_list();
        let scope_markers = eff_list.scope_markers();

        assert!(
            scope_markers
                .iter()
                .any(|marker| matches!(marker.scope_kind, ScopeKind::Parallel)
                    && matches!(marker.event, ScopeEvent::Enter)),
            "parallel projection should preserve parallel enter marker"
        );
        assert!(
            scope_markers
                .iter()
                .any(|marker| matches!(marker.scope_kind, ScopeKind::Route)
                    && matches!(marker.event, ScopeEvent::Enter)),
            "parallel route projection should preserve route enter marker"
        );
    }
}
