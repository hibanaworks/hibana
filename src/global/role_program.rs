//! Role-local program representation derived from const `EffList`.
//!
//! `RoleProgram` is the typed entry point for a role projection witness.
//! Crate-private lowering facts stay behind this module and the compiled layer.

#[cfg(test)]
use core::ptr;

use super::compiled::ProgramStamp;
use super::{
    program::{BuildProgramSource, Program},
    steps::ProjectRole,
};
use crate::control::cap::mint::{CapShot, MintConfig, MintConfigMarker};
use crate::{
    eff::{self, EffIndex},
    global::const_dsl::{CompactScopeId, EffList, ScopeEvent, ScopeId, ScopeKind},
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

#[derive(Clone, Copy)]
struct RoleLoweringCounts {
    eff_count: usize,
    parallel_enter_count: usize,
    route_scope_count: usize,
    passive_linger_route_scope_count: usize,
}

/// Erased lowering input derived from a typed `RoleProgram` witness.
#[derive(Clone, Copy)]
pub(crate) struct RoleLoweringInput<'prog> {
    eff_list: &'prog EffList,
    stamp: ProgramStamp,
    counts: RoleLoweringCounts,
}

impl<'prog> RoleLoweringInput<'prog> {
    #[inline(always)]
    pub(crate) const fn eff_list(&self) -> &'prog EffList {
        self.eff_list
    }

    #[inline(always)]
    pub(crate) const fn stamp(&self) -> ProgramStamp {
        self.stamp
    }

    #[inline(always)]
    pub(crate) const fn eff_count(&self) -> usize {
        self.counts.eff_count
    }

    #[inline(always)]
    pub(crate) const fn parallel_enter_count(&self) -> usize {
        self.counts.parallel_enter_count
    }

    #[inline(always)]
    pub(crate) const fn route_scope_count(&self) -> usize {
        self.counts.route_scope_count
    }

    #[inline(always)]
    pub(crate) const fn passive_linger_route_scope_count(&self) -> usize {
        self.counts.passive_linger_route_scope_count
    }
}

pub struct RoleProgram<'prog, const ROLE: u8, GlobalSteps, Mint = MintConfig>
where
    Mint: MintConfigMarker,
{
    eff_list: &'prog EffList,
    mint: Mint,
    stamp: ProgramStamp,
    _global_steps: core::marker::PhantomData<GlobalSteps>,
}

impl<'prog, const ROLE: u8, GlobalSteps, Mint> RoleProgram<'prog, ROLE, GlobalSteps, Mint>
where
    Mint: MintConfigMarker,
{
    const fn new(eff_list: &'prog EffList, mint: Mint, stamp: ProgramStamp) -> Self {
        Self {
            eff_list,
            mint,
            stamp,
            _global_steps: core::marker::PhantomData,
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

    /// Mint configuration baked into the RoleProgram.
    #[inline(always)]
    pub(crate) const fn mint_config(&self) -> Mint {
        self.mint
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
        eff_list: program.eff_list,
        stamp: program.stamp,
        counts: role_lowering_counts::<ROLE>(program.eff_list),
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
    RoleProgram::new(program.eff_list(), Mint::INSTANCE, program.stamp())
}

#[inline(always)]
const fn role_lowering_counts<const ROLE: u8>(eff_list: &EffList) -> RoleLoweringCounts {
    let scope_markers = eff_list.scope_markers();
    let mut parallel_enter_count = 0usize;
    let mut route_scope_count = 0usize;
    let mut passive_linger_route_scope_count = 0usize;
    let mut route_scope_ordinals = [0u64; 8];
    let mut marker_idx = 0usize;
    while marker_idx < scope_markers.len() {
        let marker = scope_markers[marker_idx];
        if matches!(marker.event, ScopeEvent::Enter) {
            if matches!(marker.scope_kind, ScopeKind::Parallel) {
                parallel_enter_count = parallel_enter_count.saturating_add(1);
            } else if matches!(marker.scope_kind, ScopeKind::Route) {
                let ordinal = marker.scope_id.local_ordinal() as usize;
                let word = ordinal / 64;
                let bit = ordinal % 64;
                if word >= route_scope_ordinals.len() {
                    panic!("route scope ordinal overflow");
                }
                let mask = 1u64 << bit;
                if (route_scope_ordinals[word] & mask) == 0 {
                    route_scope_ordinals[word] |= mask;
                    route_scope_count = route_scope_count.saturating_add(1);
                    if marker.linger
                        && matches!(marker.controller_role, Some(controller_role) if controller_role != ROLE)
                    {
                        passive_linger_route_scope_count =
                            passive_linger_route_scope_count.saturating_add(1);
                    }
                }
            }
        }
        marker_idx += 1;
    }
    RoleLoweringCounts {
        eff_count: eff_list.len(),
        parallel_enter_count,
        route_scope_count,
        passive_linger_route_scope_count,
    }
}

#[cfg(test)]
mod tests {
    use core::{cell::UnsafeCell, mem::MaybeUninit};
    use std::{thread::LocalKey, thread_local};

    use super::*;
    use crate::control::cap::mint::{CapShot, GenericCapToken};
    use crate::control::cap::resource_kinds::CancelKind;
    use crate::g::{self, Msg, Role};
    use crate::global::CanonicalControl;
    use crate::global::compiled::CompiledRole;
    use crate::global::const_dsl::{ScopeEvent, ScopeKind};
    use crate::global::steps::{self, ParSteps, PolicySteps, RouteSteps, SeqSteps, StepCons, StepNil};
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

    fn with_compiled_role_in_slot<const ROLE: u8, GlobalSteps, R>(
        compiled_slot: &'static LocalKey<UnsafeCell<MaybeUninit<CompiledRole>>>,
        scratch_slot: &'static LocalKey<UnsafeCell<MaybeUninit<RoleCompileScratch>>>,
        program: &RoleProgram<'_, ROLE, GlobalSteps, MintConfig>,
        f: impl FnOnce(&CompiledRole) -> R,
    ) -> R {
        crate::global::compiled::with_compiled_role_in_slot::<ROLE, _>(
            compiled_slot,
            scratch_slot,
            super::lowering_input(program),
            f,
        )
    }

    fn with_compiled_role<const ROLE: u8, GlobalSteps, R>(
        program: &RoleProgram<'_, ROLE, GlobalSteps, MintConfig>,
        f: impl FnOnce(&CompiledRole) -> R,
    ) -> R {
        with_compiled_role_in_slot(
            &COMPILED_ROLE_STORAGE_A,
            &COMPILED_ROLE_SCRATCH_A,
            program,
            f,
        )
    }

    fn with_compiled_roles<const LEFT_ROLE: u8, LeftSteps, const RIGHT_ROLE: u8, RightSteps, R>(
        left: &RoleProgram<'_, LEFT_ROLE, LeftSteps, MintConfig>,
        right: &RoleProgram<'_, RIGHT_ROLE, RightSteps, MintConfig>,
        f: impl FnOnce(&CompiledRole, &CompiledRole) -> R,
    ) -> R {
        with_compiled_role_in_slot(
            &COMPILED_ROLE_STORAGE_A,
            &COMPILED_ROLE_SCRATCH_A,
            left,
            |left_compiled| {
                with_compiled_role_in_slot(
                    &COMPILED_ROLE_STORAGE_B,
                    &COMPILED_ROLE_SCRATCH_B,
                    right,
                    |right_compiled| f(left_compiled, right_compiled),
                )
            },
        )
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
        SeqSteps<
            StepCons<steps::SendStep<Role<0>, Role<1>, Msg<1, ()>, 0>, StepNil>,
            StepCons<steps::SendStep<Role<1>, Role<0>, Msg<2, ()>, 0>, StepNil>,
        >,
    > = project(&PROTOCOL);
    const ROLE_ONE: RoleProgram<
        'static,
        1,
        SeqSteps<
            StepCons<steps::SendStep<Role<0>, Role<1>, Msg<1, ()>, 0>, StepNil>,
            StepCons<steps::SendStep<Role<1>, Role<0>, Msg<2, ()>, 0>, StepNil>,
        >,
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
    > = project(&CANCEL_PROGRAM);

    const LOCAL_PROGRAM: Program<
        StepCons<steps::SendStep<Role<0>, Role<0>, Msg<5, ()>, 0>, StepNil>,
    > = g::send::<Role<0>, Role<0>, Msg<5, ()>, 0>();

    const LOCAL_ROLE: RoleProgram<
        'static,
        0,
        StepCons<steps::SendStep<Role<0>, Role<0>, Msg<5, ()>, 0>, StepNil>,
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
        SeqSteps<
            StepCons<steps::SendStep<Role<0>, Role<0>, Msg<6, ()>, 0>, StepNil>,
            SeqSteps<
                StepCons<steps::SendStep<Role<0>, Role<1>, Msg<7, ()>, 0>, StepNil>,
                StepCons<steps::SendStep<Role<1>, Role<0>, Msg<8, ()>, 0>, StepNil>,
            >,
        >,
    > = project(&CHAIN_PROGRAM);

    const PING_PROGRAM: Program<
        StepCons<steps::SendStep<Role<0>, Role<1>, Msg<9, ()>, 0>, StepNil>,
    > = g::send::<Role<0>, Role<1>, Msg<9, ()>, 0>();
    const PONG_PROGRAM: Program<
        StepCons<steps::SendStep<Role<1>, Role<0>, Msg<10, ()>, 1>, StepNil>,
    > = g::send::<Role<1>, Role<0>, Msg<10, ()>, 1>();
    const PARALLEL_PROGRAM: Program<
        ParSteps<
            StepCons<steps::SendStep<Role<0>, Role<1>, Msg<9, ()>, 0>, StepNil>,
            StepCons<steps::SendStep<Role<1>, Role<0>, Msg<10, ()>, 1>, StepNil>,
        >,
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
    type ContinueHeadProgram = PolicySteps<
        StepCons<steps::SendStep<Role<2>, Role<2>, Msg<11, ()>, 1>, StepNil>,
        77,
    >;
    type BreakHeadProgram = PolicySteps<
        StepCons<steps::SendStep<Role<2>, Role<2>, Msg<12, ()>, 1>, StepNil>,
        77,
    >;
    const CONTINUE_ARM_PROGRAM: Program<
        SeqSteps<
            ContinueHeadProgram,
            StepCons<steps::SendStep<Role<2>, Role<1>, Msg<13, ()>, 1>, StepNil>,
        >,
    > = g::seq(
        g::send::<Role<2>, Role<2>, Msg<11, ()>, 1>().policy::<77>(),
        g::send::<Role<2>, Role<1>, Msg<13, ()>, 1>(),
    );
    const BREAK_ARM_PROGRAM: Program<BreakHeadProgram> =
        g::send::<Role<2>, Role<2>, Msg<12, ()>, 1>().policy::<77>();
    const PARALLEL_ROUTE_PROGRAM: Program<
        ParSteps<
            SeqSteps<
                StepCons<steps::SendStep<Role<0>, Role<1>, Msg<14, ()>, 0>, StepNil>,
                StepCons<steps::SendStep<Role<1>, Role<0>, Msg<15, ()>, 0>, StepNil>,
            >,
            RouteSteps<
                SeqSteps<
                    ContinueHeadProgram,
                    StepCons<steps::SendStep<Role<2>, Role<1>, Msg<13, ()>, 1>, StepNil>,
                >,
                BreakHeadProgram,
            >,
        >,
    > = g::par(
        LANE0_PROGRAM,
        g::route(CONTINUE_ARM_PROGRAM, BREAK_ARM_PROGRAM),
    );

    #[test]
    fn projection_extracts_role_view() {
        with_compiled_roles(&ROLE_ZERO, &ROLE_ONE, |role_zero, role_one| {
            let role_zero_layout = role_zero.layout();
            assert_eq!(role_zero_layout.len(), 2);
            assert!(role_zero_layout.steps()[0].is_send());
            assert!(role_zero_layout.steps()[1].is_recv());
            assert_eq!(role_zero_layout.steps()[0].peer(), 1);
            assert_eq!(role_zero_layout.steps()[1].peer(), 1);

            let role_one_layout = role_one.layout();
            assert_eq!(role_one_layout.len(), 2);
            assert!(role_one_layout.steps()[0].is_recv());
            assert!(role_one_layout.steps()[1].is_send());

            assert_eq!(
                super::lowering_input(&ROLE_ZERO)
                    .eff_list()
                    .control_markers()
                    .len(),
                PROTOCOL.eff_list().control_markers().len()
            );
            assert_eq!(
                super::lowering_input(&ROLE_ONE)
                    .eff_list()
                    .scope_markers()
                    .len(),
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
        });
    }

    #[test]
    fn control_step_carries_shot_metadata() {
        // CancelMsg is a self-send (Client→Client), which projects to LocalAction
        with_compiled_role(&CANCEL_ROLE, |cancel_role| {
            let cancel_layout = cancel_role.layout();
            assert_eq!(cancel_layout.len(), 1);
            let step = cancel_layout.steps()[0];
            assert!(step.is_local_action());
            assert!(step.is_control);
            assert_eq!(step.shot, Some(CapShot::One));
        });
    }

    #[test]
    fn local_action_projects_as_local_step() {
        with_compiled_role(&LOCAL_ROLE, |local_role| {
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
        });
    }

    #[test]
    fn chained_projection_preserves_typed_local_steps() {
        with_compiled_role(&CHAIN_ROLE, |chain_role| {
            let chain_layout = chain_role.layout();
            assert_eq!(chain_layout.len(), 3);
            assert!(chain_layout.steps()[0].is_local_action());
            assert!(chain_layout.steps()[1].is_send());
            assert!(chain_layout.steps()[2].is_recv());
            assert_eq!(chain_layout.steps()[1].peer(), 1);
            assert_eq!(chain_layout.steps()[2].peer(), 1);
        });
    }

    #[test]
    fn parallel_projection_keeps_phase_and_lane_split_internal() {
        let parallel_program = PARALLEL_PROGRAM;
        let client: RoleProgram<
            '_,
            0,
            ParSteps<
                StepCons<steps::SendStep<Role<0>, Role<1>, Msg<9, ()>, 0>, StepNil>,
                StepCons<steps::SendStep<Role<1>, Role<0>, Msg<10, ()>, 1>, StepNil>,
            >,
            MintConfig,
        > = project(&parallel_program);
        let server: RoleProgram<
            '_,
            1,
            ParSteps<
                StepCons<steps::SendStep<Role<0>, Role<1>, Msg<9, ()>, 0>, StepNil>,
                StepCons<steps::SendStep<Role<1>, Role<0>, Msg<10, ()>, 1>, StepNil>,
            >,
            MintConfig,
        > = project(&parallel_program);

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
        let program: RoleProgram<
            '_,
            0,
            ParSteps<
                SeqSteps<
                    StepCons<steps::SendStep<Role<0>, Role<1>, Msg<14, ()>, 0>, StepNil>,
                    StepCons<steps::SendStep<Role<1>, Role<0>, Msg<15, ()>, 0>, StepNil>,
                >,
                RouteSteps<
                    SeqSteps<
                        ContinueHeadProgram,
                        StepCons<steps::SendStep<Role<2>, Role<1>, Msg<13, ()>, 1>, StepNil>,
                    >,
                    BreakHeadProgram,
                >,
            >,
            MintConfig,
        > = project(&parallel_route_program);
        let eff_list = super::lowering_input(&program).eff_list();
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
