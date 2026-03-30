//! Role-local program representation derived from const `EffList`.
//!
//! `RoleProgram` is the typed entry point for a role-local projection.
//! Crate-private lowering facts stay behind this module and the compiled layer.

use core::ptr;

use super::{
    compiled::{ProgramStamp, ProjectionSeal},
    program::Program,
    steps::ProjectRole,
};
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
#[derive(Clone, Debug)]
pub(crate) struct ProjectedRoleLayout {
    local_steps: [LocalStep; MAX_STEPS],
    local_len: usize,
    phases: [Phase; MAX_PHASES],
    phase_len: usize,
}

impl ProjectedRoleLayout {
    #[inline(always)]
    pub(super) unsafe fn init_from_refs(
        dst: *mut Self,
        local_steps: &[LocalStep; MAX_STEPS],
        local_len: usize,
        phases: &[Phase; MAX_PHASES],
        phase_len: usize,
    ) {
        unsafe {
            ptr::copy_nonoverlapping(
                local_steps.as_ptr(),
                ptr::addr_of_mut!((*dst).local_steps).cast::<LocalStep>(),
                MAX_STEPS,
            );
            ptr::addr_of_mut!((*dst).local_len).write(local_len);
            ptr::copy_nonoverlapping(
                phases.as_ptr(),
                ptr::addr_of_mut!((*dst).phases).cast::<Phase>(),
                MAX_PHASES,
            );
            ptr::addr_of_mut!((*dst).phase_len).write(phase_len);
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

#[derive(Clone, Copy)]
pub struct RoleProgram<'prog, const ROLE: u8, LocalSteps, Mint = MintConfig>
where
    Mint: MintConfigMarker,
{
    eff_list: &'prog EffList,
    mint: Mint,
    stamp: ProgramStamp,
    _local_steps: core::marker::PhantomData<LocalSteps>,
}

impl<'prog, const ROLE: u8, LocalSteps, Mint> RoleProgram<'prog, ROLE, LocalSteps, Mint>
where
    Mint: MintConfigMarker,
{
    const fn new(eff_list: &'prog EffList, mint: Mint, stamp: ProgramStamp) -> Self {
        Self {
            eff_list,
            mint,
            stamp,
            _local_steps: core::marker::PhantomData,
        }
    }

    #[inline(always)]
    pub(crate) const fn stamp(&self) -> ProgramStamp {
        self.stamp
    }

    #[inline(always)]
    #[cfg(test)]
    pub(crate) fn borrow_id(&self) -> usize {
        self.eff_list as *const EffList as usize
    }

    #[inline(always)]
    pub(crate) const fn lowering_input(&self) -> &'prog EffList {
        self.eff_list
    }

    #[cfg(test)]
    #[inline(always)]
    pub(crate) const fn eff_list_ref(&self) -> &'prog EffList {
        self.eff_list
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
    let stamp = ProjectionSeal::<ROLE>::validate_and_stamp::<Steps, Mint>(program);
    RoleProgram::new(program.eff_list(), Mint::INSTANCE, stamp)
}

#[cfg(test)]
mod tests {
    use std::boxed::Box;

    use super::*;
    use crate::control::cap::mint::{CapShot, GenericCapToken};
    use crate::control::cap::resource_kinds::CancelKind;
    use crate::g::{self, Msg, Role};
    use crate::global::CanonicalControl;
    use crate::global::compiled::{CompiledRole, LoweringSummary};
    use crate::global::const_dsl::{ScopeEvent, ScopeKind};
    use crate::global::steps::{self, ProjectRole, SeqSteps, StepConcat, StepCons, StepNil};
    use crate::global::typestate::RoleCompileScratch;

    fn compile_role_boxed<const ROLE: u8, LocalSteps>(
        program: &RoleProgram<'_, ROLE, LocalSteps, MintConfig>,
    ) -> Box<CompiledRole> {
        let summary = LoweringSummary::scan_const(program.lowering_input());
        let mut compiled = Box::<CompiledRole>::new_uninit();
        let mut scratch = Box::new(RoleCompileScratch::new());
        unsafe {
            CompiledRole::init_from_summary::<ROLE>(
                compiled.as_mut_ptr(),
                &summary,
                scratch.as_mut(),
            );
            compiled.assume_init()
        }
    }

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
        let role_zero = compile_role_boxed(&ROLE_ZERO);
        let role_zero_layout = role_zero.layout();
        assert_eq!(role_zero_layout.len(), 2);
        assert!(role_zero_layout.steps()[0].is_send());
        assert!(role_zero_layout.steps()[1].is_recv());
        assert_eq!(role_zero_layout.steps()[0].peer(), 1);
        assert_eq!(role_zero_layout.steps()[1].peer(), 1);

        let role_one = compile_role_boxed(&ROLE_ONE);
        let role_one_layout = role_one.layout();
        assert_eq!(role_one_layout.len(), 2);
        assert!(role_one_layout.steps()[0].is_recv());
        assert!(role_one_layout.steps()[1].is_send());

        assert_eq!(
            ROLE_ZERO.eff_list_ref().control_markers().len(),
            PROTOCOL.eff_list().control_markers().len()
        );
        assert_eq!(
            ROLE_ONE.eff_list_ref().scope_markers().len(),
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
        let cancel_role = compile_role_boxed(&CANCEL_ROLE);
        let cancel_layout = cancel_role.layout();
        assert_eq!(cancel_layout.len(), 1);
        let step = cancel_layout.steps()[0];
        assert!(step.is_local_action());
        assert!(step.is_control);
        assert_eq!(step.shot, Some(CapShot::One));
    }

    #[test]
    fn local_action_projects_as_local_step() {
        let local_role = compile_role_boxed(&LOCAL_ROLE);
        let local_layout = local_role.layout();
        assert_eq!(local_layout.len(), 1);
        let step = local_layout.steps()[0];
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
        let chain_role = compile_role_boxed(&CHAIN_ROLE);
        let chain_layout = chain_role.layout();
        assert_eq!(chain_layout.len(), 3);
        assert!(chain_layout.steps()[0].is_local_action());
        assert!(chain_layout.steps()[1].is_send());
        assert!(chain_layout.steps()[2].is_recv());
        assert_eq!(chain_layout.steps()[1].peer(), 1);
        assert_eq!(chain_layout.steps()[2].peer(), 1);
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

        let client_projection = compile_role_boxed(&client);
        let server_projection = compile_role_boxed(&server);

        assert_eq!(client_projection.layout().phase_count(), 1);
        assert_eq!(server_projection.layout().phase_count(), 1);

        let client_phase = client_projection.layout().phases()[0];
        assert!(client_phase.lanes[0].is_active());
        assert!(client_phase.lanes[1].is_active());

        let server_phase = server_projection.layout().phases()[0];
        assert!(server_phase.lanes[0].is_active());
        assert!(server_phase.lanes[1].is_active());

        let client_lane0 = client_projection
            .layout()
            .steps()
            .iter()
            .filter(|step| step.lane() == 0)
            .count();
        let client_lane1 = client_projection
            .layout()
            .steps()
            .iter()
            .filter(|step| step.lane() == 1)
            .count();
        assert_eq!(client_lane0, 1);
        assert_eq!(client_lane1, 1);

        let server_lane0 = server_projection
            .layout()
            .steps()
            .iter()
            .filter(|step| step.lane() == 0)
            .count();
        let server_lane1 = server_projection
            .layout()
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
        let eff_list = program.eff_list_ref();
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
