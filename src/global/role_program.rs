//! Role-local program representation derived from const `EffList`.
//!
//! `RoleProgram` is the typed entry point for a role projection witness.
//! Crate-private lowering facts stay behind this module and the compiled layer.

use core::marker::PhantomData;
#[cfg(test)]
use core::ptr;

use super::compiled::{ProgramStamp, RoleLoweringCounts};
use super::{
    program::{BuildProgramSource, Program},
    steps::ProjectRole,
};
use crate::control::cap::mint::{CapShot, MintConfig, MintConfigMarker};
#[cfg(test)]
use crate::eff;
use crate::{
    eff::EffIndex,
    global::const_dsl::{CompactScopeId, EffList, ScopeId},
    global::{KnownRole, Role},
};

#[cfg(test)]
pub(super) const MAX_STEPS: usize = eff::meta::MAX_EFF_NODES;
/// Maximum number of parallel phases in a program.
#[cfg(test)]
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
    pub start: u16,
    /// Number of steps in this lane.
    pub len: u16,
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
    scope: CompactScopeId,
    pub arm: u8,
}

impl PhaseRouteGuard {
    pub const EMPTY: Self = Self {
        scope: CompactScopeId::none(),
        arm: 0,
    };

    #[inline(always)]
    pub(crate) const fn new(scope: ScopeId, arm: u8) -> Self {
        Self {
            scope: CompactScopeId::from_scope_id(scope),
            arm,
        }
    }

    #[inline(always)]
    pub const fn is_empty(&self) -> bool {
        self.scope.is_none()
    }

    #[inline(always)]
    pub(crate) const fn scope(self) -> ScopeId {
        self.scope.to_scope_id()
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
    pub min_start: u16,
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
/// `RoleProgram` is the thin, typed owner of a role projection witness.
/// Runtime metadata such as local-step tables, phase splits, and typestate
/// graphs are materialized on demand so that frozen `Program` tokens stay small
/// and cheap to move.
#[cfg(test)]
#[derive(Clone, Debug)]
pub(crate) struct ProjectedRoleLayout {
    #[cfg(test)]
    local_steps: [LocalStep; MAX_STEPS],
    phases: [Phase; MAX_PHASES],
}

#[cfg(test)]
impl ProjectedRoleLayout {
    #[inline(always)]
    pub(super) unsafe fn init_from_refs(
        dst: *mut Self,
        local_steps: &[LocalStep; MAX_STEPS],
        _local_len: usize,
        phases: &[Phase; MAX_PHASES],
        _phase_len: usize,
    ) {
        #[cfg(not(test))]
        let _ = local_steps;
        unsafe {
            #[cfg(test)]
            ptr::copy_nonoverlapping(
                local_steps.as_ptr(),
                ptr::addr_of_mut!((*dst).local_steps).cast::<LocalStep>(),
                MAX_STEPS,
            );
            ptr::copy_nonoverlapping(
                phases.as_ptr(),
                ptr::addr_of_mut!((*dst).phases).cast::<Phase>(),
                MAX_PHASES,
            );
        }
    }

    #[inline(always)]
    pub(crate) fn len(&self) -> usize {
        let mut len = 0usize;
        while len < MAX_STEPS {
            if matches!(self.local_steps[len].direction, LocalDirection::None) {
                break;
            }
            len += 1;
        }
        len
    }

    #[cfg(test)]
    #[inline(always)]
    pub(crate) fn steps(&self) -> &[LocalStep] {
        &self.local_steps[..self.len()]
    }

    #[cfg(test)]
    #[inline(always)]
    pub(crate) fn phase_count(&self) -> usize {
        let mut len = 0usize;
        while len < MAX_PHASES {
            if self.phases[len].lane_mask == 0 {
                break;
            }
            len += 1;
        }
        len
    }

    #[inline(always)]
    pub(crate) fn phases(&self) -> &[Phase] {
        &self.phases[..self.phase_count()]
    }
}

/// Erased lowering input derived from a typed `RoleProgram` witness.
#[derive(Clone, Copy)]
pub(crate) struct RoleLoweringInput<'prog> {
    _borrow: PhantomData<&'prog EffList>,
    summary: &'static crate::global::compiled::LoweringSummary,
    stamp: ProgramStamp,
    counts: RoleLoweringCounts,
}

mod private {
    #[derive(Clone, Copy)]
    pub struct RoleProgramSeal;

    pub trait RoleProgramViewSeal {}
}

pub(crate) trait RoleProgramView<'prog, const ROLE: u8, Mint>:
    private::RoleProgramViewSeal
where
    Mint: MintConfigMarker,
{
    fn stamp(&self) -> ProgramStamp;
    fn mint_config(&self) -> Mint;
    fn lowering_input(&self) -> RoleLoweringInput<'prog>;
}

#[derive(Clone, Copy)]
pub(crate) struct RoleFootprint {
    pub(crate) scope_count: usize,
    pub(crate) eff_count: usize,
    pub(crate) parallel_enter_count: usize,
    pub(crate) route_scope_count: usize,
    pub(crate) local_step_count: usize,
    pub(crate) passive_linger_route_scope_count: usize,
    pub(crate) active_lane_count: usize,
    pub(crate) logical_lane_count: usize,
    pub(crate) max_route_stack_depth: usize,
    pub(crate) scope_evidence_count: usize,
    pub(crate) frontier_entry_count: usize,
}

impl RoleFootprint {
    #[inline(always)]
    pub(crate) const fn for_endpoint_layout(
        active_lane_count: usize,
        logical_lane_count: usize,
        max_route_stack_depth: usize,
        scope_evidence_count: usize,
        frontier_entry_count: usize,
    ) -> Self {
        Self {
            scope_count: 0,
            eff_count: 0,
            parallel_enter_count: 0,
            route_scope_count: 0,
            local_step_count: 0,
            passive_linger_route_scope_count: 0,
            active_lane_count,
            logical_lane_count,
            max_route_stack_depth,
            scope_evidence_count,
            frontier_entry_count,
        }
    }

    #[inline(always)]
    pub(crate) const fn phase_upper_bound(self) -> usize {
        if self.local_step_count == 0 {
            0
        } else {
            let derived = self
                .parallel_enter_count
                .saturating_mul(2)
                .saturating_add(1);
            if derived < self.local_step_count {
                derived
            } else {
                self.local_step_count
            }
        }
    }
}

impl<'prog> RoleLoweringInput<'prog> {
    #[inline(always)]
    pub(crate) const fn summary(&self) -> &'static crate::global::compiled::LoweringSummary {
        self.summary
    }

    #[inline(always)]
    pub(crate) const fn stamp(&self) -> ProgramStamp {
        self.stamp
    }

    #[cfg(test)]
    #[inline(always)]
    pub(crate) const fn eff_count(&self) -> usize {
        self.counts.eff_count
    }

    #[cfg(test)]
    #[inline(always)]
    pub(crate) const fn local_step_count(&self) -> usize {
        self.counts.local_step_count
    }

    #[cfg(test)]
    #[inline(always)]
    pub(crate) const fn route_scope_count(&self) -> usize {
        self.counts.route_scope_count
    }

    #[cfg(test)]
    #[inline(always)]
    pub(crate) const fn passive_linger_route_scope_count(&self) -> usize {
        self.counts.passive_linger_route_scope_count
    }

    #[inline(always)]
    pub(crate) const fn footprint(&self) -> RoleFootprint {
        RoleFootprint {
            scope_count: self.counts.scope_count,
            eff_count: self.counts.eff_count,
            parallel_enter_count: self.counts.parallel_enter_count,
            route_scope_count: self.counts.route_scope_count,
            local_step_count: self.counts.local_step_count,
            passive_linger_route_scope_count: self.counts.passive_linger_route_scope_count,
            active_lane_count: 0,
            logical_lane_count: 0,
            max_route_stack_depth: 0,
            scope_evidence_count: 0,
            frontier_entry_count: 0,
        }
    }
}

pub struct RoleProgram<'prog, const ROLE: u8, GlobalSteps, Mint = MintConfig>
where
    Mint: MintConfigMarker,
{
    _borrow: PhantomData<&'prog EffList>,
    _global_steps: PhantomData<fn() -> GlobalSteps>,
    _seal: private::RoleProgramSeal,
    summary: &'static crate::global::compiled::LoweringSummary,
    mint: Mint,
    stamp: ProgramStamp,
}

impl<'prog, const ROLE: u8, GlobalSteps, Mint> RoleProgram<'prog, ROLE, GlobalSteps, Mint>
where
    Mint: MintConfigMarker,
{
    const fn new(
        summary: &'static crate::global::compiled::LoweringSummary,
        mint: Mint,
        stamp: ProgramStamp,
    ) -> Self {
        Self {
            _borrow: PhantomData,
            _global_steps: PhantomData,
            _seal: private::RoleProgramSeal,
            summary,
            mint,
            stamp,
        }
    }

    #[inline(always)]
    pub(crate) const fn stamp(&self) -> ProgramStamp {
        self.stamp
    }

    #[inline(always)]
    #[cfg(test)]
    pub(crate) fn borrow_id(&self) -> usize {
        self.summary as *const crate::global::compiled::LoweringSummary as usize
    }

    /// Mint configuration baked into the RoleProgram.
    #[inline(always)]
    pub(crate) const fn mint_config(&self) -> Mint {
        self.mint
    }
}

impl<'prog, const ROLE: u8, GlobalSteps, Mint> private::RoleProgramViewSeal
    for RoleProgram<'prog, ROLE, GlobalSteps, Mint>
where
    Mint: MintConfigMarker,
{
}

impl<'prog, const ROLE: u8, GlobalSteps, Mint> RoleProgramView<'prog, ROLE, Mint>
    for RoleProgram<'prog, ROLE, GlobalSteps, Mint>
where
    Mint: MintConfigMarker,
{
    #[inline(always)]
    fn stamp(&self) -> ProgramStamp {
        RoleProgram::stamp(self)
    }

    #[inline(always)]
    fn mint_config(&self) -> Mint {
        RoleProgram::mint_config(self)
    }

    #[inline(always)]
    fn lowering_input(&self) -> RoleLoweringInput<'prog> {
        lowering_input(self)
    }
}

#[inline(always)]
pub(crate) const fn lowering_input<'prog, const ROLE: u8, GlobalSteps, Mint>(
    program: &RoleProgram<'prog, ROLE, GlobalSteps, Mint>,
) -> RoleLoweringInput<'prog>
where
    Mint: MintConfigMarker,
{
    RoleLoweringInput {
        _borrow: PhantomData,
        summary: program.summary,
        stamp: program.stamp,
        counts: program.summary.role_lowering_counts::<ROLE>(),
    }
}

/// Project a typed program into the local view for `ROLE`.
#[allow(private_bounds)]
pub const fn project<'prog, const ROLE: u8, Steps, Mint>(
    program: &'prog Program<Steps>,
) -> RoleProgram<'prog, ROLE, Steps, Mint>
where
    Role<ROLE>: KnownRole,
    Steps: BuildProgramSource + ProjectRole<Role<ROLE>>,
    Mint: MintConfigMarker,
{
    RoleProgram::new(program.summary(), Mint::INSTANCE, program.stamp())
}

#[cfg(test)]
mod tests {
    use core::{cell::UnsafeCell, mem::MaybeUninit};
    use std::{thread::LocalKey, thread_local};

    use super::*;
    use crate::g::{self, Msg, Role};
    use crate::global::compiled::CompiledRole;
    use crate::global::const_dsl::{ScopeEvent, ScopeKind};
    use crate::global::steps::{self, ParSteps, RouteSteps, SeqSteps, StepCons, StepNil};
    use crate::global::typestate::RoleCompileScratch;

    thread_local! {
        static COMPILED_ROLE_STORAGE_A: UnsafeCell<MaybeUninit<CompiledRole>> =
            const { UnsafeCell::new(MaybeUninit::uninit()) };
        static COMPILED_ROLE_SCRATCH_A: UnsafeCell<MaybeUninit<RoleCompileScratch>> =
            const { UnsafeCell::new(MaybeUninit::uninit()) };
        static COMPILED_ROLE_STORAGE_B: UnsafeCell<MaybeUninit<CompiledRole>> =
            const { UnsafeCell::new(MaybeUninit::uninit()) };
        static COMPILED_ROLE_SCRATCH_B: UnsafeCell<MaybeUninit<RoleCompileScratch>> =
            const { UnsafeCell::new(MaybeUninit::uninit()) };
    }

    fn with_compiled_role_in_slot<const ROLE: u8, Steps, R>(
        compiled_slot: &'static LocalKey<UnsafeCell<MaybeUninit<CompiledRole>>>,
        scratch_slot: &'static LocalKey<UnsafeCell<MaybeUninit<RoleCompileScratch>>>,
        program: &RoleProgram<'_, ROLE, Steps, MintConfig>,
        f: impl FnOnce(&CompiledRole) -> R,
    ) -> R {
        crate::global::compiled::with_compiled_role_in_slot::<ROLE, _>(
            compiled_slot,
            scratch_slot,
            crate::global::lowering_input(program),
            f,
        )
    }

    fn with_compiled_roles<const LEFT_ROLE: u8, const RIGHT_ROLE: u8, LeftSteps, RightSteps, R>(
        left: &RoleProgram<'_, LEFT_ROLE, LeftSteps, MintConfig>,
        right: &RoleProgram<'_, RIGHT_ROLE, RightSteps, MintConfig>,
        f: impl FnOnce(&CompiledRole, &CompiledRole) -> R,
    ) -> R {
        with_compiled_role_in_slot::<LEFT_ROLE, LeftSteps, _>(
            &COMPILED_ROLE_STORAGE_A,
            &COMPILED_ROLE_SCRATCH_A,
            left,
            |left_projection| {
                with_compiled_role_in_slot::<RIGHT_ROLE, RightSteps, _>(
                    &COMPILED_ROLE_STORAGE_B,
                    &COMPILED_ROLE_SCRATCH_B,
                    right,
                    |right_projection| f(left_projection, right_projection),
                )
            },
        )
    }

    type ParallelLane0 = StepCons<steps::SendStep<Role<0>, Role<1>, Msg<9, ()>, 0>, StepNil>;
    type ParallelLane1 = StepCons<steps::SendStep<Role<1>, Role<0>, Msg<10, ()>, 1>, StepNil>;
    const PARALLEL_LANE0: Program<ParallelLane0> = g::send::<Role<0>, Role<1>, Msg<9, ()>, 0>();
    const PARALLEL_LANE1: Program<ParallelLane1> = g::send::<Role<1>, Role<0>, Msg<10, ()>, 1>();
    const PARALLEL_PROGRAM: Program<ParSteps<ParallelLane0, ParallelLane1>> =
        g::par(PARALLEL_LANE0, PARALLEL_LANE1);

    type RouteLeft = SeqSteps<
        StepCons<steps::SendStep<Role<0>, Role<0>, Msg<14, ()>, 0>, StepNil>,
        StepCons<steps::SendStep<Role<0>, Role<1>, Msg<15, ()>, 0>, StepNil>,
    >;
    type RouteRight = SeqSteps<
        StepCons<steps::SendStep<Role<0>, Role<0>, Msg<16, ()>, 0>, StepNil>,
        StepCons<steps::SendStep<Role<0>, Role<1>, Msg<17, ()>, 0>, StepNil>,
    >;
    const ROUTE_LEFT_PROGRAM: Program<RouteLeft> = g::seq(
        g::send::<Role<0>, Role<0>, Msg<14, ()>, 0>(),
        g::send::<Role<0>, Role<1>, Msg<15, ()>, 0>(),
    );
    const ROUTE_RIGHT_PROGRAM: Program<RouteRight> = g::seq(
        g::send::<Role<0>, Role<0>, Msg<16, ()>, 0>(),
        g::send::<Role<0>, Role<1>, Msg<17, ()>, 0>(),
    );
    type RouteProgramSteps = RouteSteps<RouteLeft, RouteRight>;
    const ROUTE_PROGRAM: Program<RouteProgramSteps> =
        g::route(ROUTE_LEFT_PROGRAM, ROUTE_RIGHT_PROGRAM);
    const PARALLEL_ROUTE_PROGRAM: Program<ParSteps<ParallelLane1, RouteProgramSteps>> =
        g::par(PARALLEL_LANE1, ROUTE_PROGRAM);

    #[test]
    fn parallel_projection_keeps_phase_and_lane_split_internal() {
        let parallel_program = PARALLEL_PROGRAM;
        let client = project::<0, _, MintConfig>(&parallel_program);
        let server = project::<1, _, MintConfig>(&parallel_program);

        with_compiled_roles(&client, &server, |client_projection, server_projection| {
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
        });
    }

    #[test]
    fn parallel_route_projection_keeps_scope_markers_without_public_step_surface() {
        let parallel_route_program = PARALLEL_ROUTE_PROGRAM;
        let program = project::<0, _, MintConfig>(&parallel_route_program);
        let scope_markers = super::lowering_input(&program)
            .summary()
            .view()
            .scope_markers();

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
