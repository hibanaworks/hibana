//! Role-local program representation derived from const `EffList`.
//!
//! `RoleProgram` materialises the subset of a global effect list that is
//! relevant to a particular role. It keeps the original `EffList` reference so
//! control-plane metadata (scope/control markers) remain accessible, while also
//! providing a compact slice of `LocalStep` entries for IDE inspection and
//! lightweight runtime checks.

use super::{
    program::Program,
    steps::ProjectRole,
    typestate::{PhaseCursor, RoleTypestate},
};
use crate::control::cap::mint::{CapShot, MintConfig, MintConfigMarker};
use crate::{
    eff::{self, EffIndex, EffKind},
    global::const_dsl::{EffList, ScopeEvent, ScopeId, ScopeKind, ScopeMarker},
    global::{KnownRole, Role},
};

const MAX_STEPS: usize = eff::meta::MAX_EFF_NODES;
/// Maximum number of parallel phases in a program.
const MAX_PHASES: usize = 32;
/// Maximum number of concurrent lanes (matches RoleLaneSet::LANE_COUNT).
pub(crate) const MAX_LANES: usize = 8;

/// Steps for a single lane within a phase.
///
/// References a contiguous slice of `LocalStep` entries within the RoleProgram's
/// `local_steps` array. Empty lanes have `len == 0`.
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
pub(crate) struct ProjectedRoleData<const ROLE: u8> {
    layout: ProjectedRoleLayout,
    typestate: RoleTypestate<ROLE>,
}

impl<const ROLE: u8> ProjectedRoleData<ROLE> {
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
    lease_budget: crate::control::lease::planner::LeaseGraphBudget,
    mint: Mint,
    _local_steps: core::marker::PhantomData<LocalSteps>,
}

impl<'prog, const ROLE: u8, LocalSteps, Mint> RoleProgram<'prog, ROLE, LocalSteps, Mint>
where
    Mint: MintConfigMarker,
{
    const fn new(eff_list: &'prog EffList, mint: Mint) -> Self {
        let budget = crate::control::lease::planner::LeaseGraphBudget::from_eff_list(eff_list);
        budget.validate();

        Self {
            eff_list,
            lease_budget: budget,
            mint,
            _local_steps: core::marker::PhantomData,
        }
    }

    fn layout(&self) -> ProjectedRoleLayout {
        let (steps, len) = Self::build_local_steps(self.eff_list, self.eff_list.as_slice());
        let (phases, phase_len) = Self::build_phases(&steps, len, self.eff_list);
        ProjectedRoleLayout {
            local_steps: steps,
            local_len: len,
            phases,
            phase_len,
        }
    }

    pub(crate) fn projection(&self) -> ProjectedRoleData<ROLE> {
        ProjectedRoleData {
            layout: self.layout(),
            typestate: super::typestate::RoleTypestate::<ROLE>::from_program(self.eff_list),
        }
    }

    const fn build_local_steps(
        program: &EffList,
        slice: &[eff::EffStruct],
    ) -> ([LocalStep; MAX_STEPS], usize) {
        let mut steps = [LocalStep::EMPTY; MAX_STEPS];
        let mut len = 0usize;
        let mut idx = 0usize;

        while idx < slice.len() {
            let node = slice[idx];
            if matches!(node.kind, EffKind::Atom) {
                let atom = node.atom_data();
                let eff_index = EffIndex::from_usize(idx);
                let shot = if atom.is_control {
                    match program.control_spec_at(idx) {
                        Some(spec) => Some(spec.shot),
                        None => None,
                    }
                } else {
                    None
                };
                if atom.from == ROLE && atom.to == ROLE {
                    steps[len] = LocalStep::local(
                        eff_index,
                        ROLE,
                        atom.label,
                        atom.resource,
                        atom.is_control,
                        shot,
                        atom.lane,
                    );
                    len += 1;
                } else if atom.from == ROLE {
                    steps[len] = LocalStep::send(
                        eff_index,
                        atom.to,
                        atom.label,
                        atom.resource,
                        atom.is_control,
                        shot,
                        atom.lane,
                    );
                    len += 1;
                } else if atom.to == ROLE {
                    steps[len] = LocalStep::recv(
                        eff_index,
                        atom.from,
                        atom.label,
                        atom.resource,
                        atom.is_control,
                        shot,
                        atom.lane,
                    );
                    len += 1;
                }
            }
            idx += 1;
        }

        (steps, len)
    }

    /// Build phases from steps based on lane assignment and g::par scope boundaries.
    ///
    /// Detects `ScopeKind::Parallel` boundaries from the EffList's scope markers
    /// to create proper fork-join phases:
    ///
    /// ```text
    /// g::seq(A, g::par(B, C), D) produces:
    ///   Phase 0: Lane 0 = [A]
    ///   Phase 1: Lane 0 = [B], Lane 1 = [C]  (parallel)
    ///   Phase 2: Lane 0 = [D]
    /// ```
    ///
    /// For non-parallel programs (most common case), there is a single phase with
    /// all steps on their respective lanes.
    const fn build_phases(
        steps: &[LocalStep; MAX_STEPS],
        len: usize,
        eff_list: &EffList,
    ) -> ([Phase; MAX_PHASES], usize) {
        let phases = [Phase::EMPTY; MAX_PHASES];

        if len == 0 {
            return (phases, 0);
        }

        // Check if there are any Parallel scope markers
        let scope_markers = eff_list.scope_markers();
        let has_parallel = Self::has_parallel_scope(scope_markers);
        let route_guards = Self::build_route_guards_for_steps(steps, len, scope_markers);

        if !has_parallel {
            return Self::build_single_phase(steps, len, &route_guards);
        }

        // Complex case: detect parallel scope boundaries and create multiple phases
        Self::build_phases_with_parallel(steps, len, scope_markers, &route_guards)
    }

    /// Check if any scope marker indicates a Parallel scope.
    const fn has_parallel_scope(markers: &[ScopeMarker]) -> bool {
        let mut i = 0;
        while i < markers.len() {
            if matches!(markers[i].scope_kind, ScopeKind::Parallel) {
                return true;
            }
            i += 1;
        }
        false
    }

    /// Compute outermost route guards for each local step.
    const fn build_route_guards_for_steps(
        steps: &[LocalStep; MAX_STEPS],
        len: usize,
        scope_markers: &[ScopeMarker],
    ) -> [PhaseRouteGuard; MAX_STEPS] {
        let mut guards = [PhaseRouteGuard::EMPTY; MAX_STEPS];
        let mut route_enter_count = [0u8; ScopeId::ORDINAL_CAPACITY as usize];
        let mut marker_idx = 0usize;
        let mut route_depth = 0usize;
        let mut outer_scope = ScopeId::none();
        let mut outer_arm = 0u8;
        let mut step_idx = 0usize;
        while step_idx < len {
            let eff_index = steps[step_idx].eff_index.as_usize();
            while marker_idx < scope_markers.len() && scope_markers[marker_idx].offset <= eff_index
            {
                let marker = scope_markers[marker_idx];
                if matches!(marker.scope_kind, ScopeKind::Route) {
                    match marker.event {
                        ScopeEvent::Enter => {
                            let ordinal = marker.scope_id.local_ordinal() as usize;
                            let arm = if ordinal < route_enter_count.len() {
                                let arm = route_enter_count[ordinal];
                                if arm < 2 {
                                    route_enter_count[ordinal] = arm + 1;
                                }
                                arm
                            } else {
                                0
                            };
                            if route_depth == 0 {
                                outer_scope = marker.scope_id;
                                outer_arm = arm;
                            }
                            route_depth = route_depth.saturating_add(1);
                        }
                        ScopeEvent::Exit => {
                            if route_depth > 0 {
                                route_depth -= 1;
                                if route_depth == 0 {
                                    outer_scope = ScopeId::none();
                                    outer_arm = 0;
                                }
                            }
                        }
                    }
                }
                marker_idx += 1;
            }
            if !outer_scope.is_none() {
                guards[step_idx] = PhaseRouteGuard {
                    scope: outer_scope,
                    arm: outer_arm,
                };
            }
            step_idx += 1;
        }
        guards
    }

    /// Build a single phase with all steps grouped by lane.
    const fn build_single_phase(
        steps: &[LocalStep; MAX_STEPS],
        len: usize,
        route_guards: &[PhaseRouteGuard; MAX_STEPS],
    ) -> ([Phase; MAX_PHASES], usize) {
        let mut phases = [Phase::EMPTY; MAX_PHASES];
        let mut lane_lens = [0usize; MAX_LANES];
        let mut lane_first = [usize::MAX; MAX_LANES];

        // Count steps per lane and find first occurrence
        let mut i = 0;
        while i < len {
            let lane = steps[i].lane as usize;
            if lane < MAX_LANES {
                if lane_first[lane] == usize::MAX {
                    lane_first[lane] = i;
                }
                lane_lens[lane] += 1;
            }
            i += 1;
        }

        let mut phase = Phase::EMPTY;
        let mut lane_mask = 0u8;
        let mut min_start = usize::MAX;
        let mut lane_idx = 0;
        while lane_idx < MAX_LANES {
            if lane_lens[lane_idx] > 0 {
                let start = lane_first[lane_idx];
                phase.lanes[lane_idx] = LaneSteps {
                    start,
                    len: lane_lens[lane_idx],
                };
                lane_mask |= 1u8 << (lane_idx as u32);
                if start < min_start {
                    min_start = start;
                }
            }
            lane_idx += 1;
        }
        phase.lane_mask = lane_mask;
        phase.min_start = if lane_mask == 0 { 0 } else { min_start };
        phase.route_guard = Self::route_guard_for_range(route_guards, 0, len);

        phases[0] = phase;
        (phases, 1)
    }

    /// Build phases with parallel scope boundary detection.
    ///
    /// Scans steps and scope markers to identify:
    /// 1. Sequential sections (before/after parallel)
    /// 2. Parallel sections (inside g::par)
    ///
    /// Each parallel scope exit creates a fork-join barrier.
    const fn build_phases_with_parallel(
        steps: &[LocalStep; MAX_STEPS],
        len: usize,
        scope_markers: &[ScopeMarker],
        route_guards: &[PhaseRouteGuard; MAX_STEPS],
    ) -> ([Phase; MAX_PHASES], usize) {
        let mut phases = [Phase::EMPTY; MAX_PHASES];
        let mut phase_count = 0usize;

        // Track parallel scope boundaries by eff_index offset
        // Parallel Enter at offset X means steps with eff_index >= X are in parallel
        // Parallel Exit at offset Y means steps with eff_index >= Y are sequential again
        let mut parallel_ranges = [(0usize, 0usize); MAX_PHASES]; // (enter_offset, exit_offset)
        let mut parallel_count = 0usize;

        // Extract parallel scope ranges from markers
        let mut marker_idx = 0;
        while marker_idx < scope_markers.len() {
            let marker = scope_markers[marker_idx];
            if matches!(marker.scope_kind, ScopeKind::Parallel)
                && matches!(marker.event, ScopeEvent::Enter)
            {
                let enter_offset = marker.offset;
                // Find matching exit
                let mut exit_offset = len; // default to end
                let mut inner_idx = marker_idx + 1;
                while inner_idx < scope_markers.len() {
                    let inner = scope_markers[inner_idx];
                    if inner.scope_id.raw() == marker.scope_id.raw()
                        && matches!(inner.event, ScopeEvent::Exit)
                    {
                        exit_offset = inner.offset;
                        break;
                    }
                    inner_idx += 1;
                }
                if parallel_count < MAX_PHASES {
                    parallel_ranges[parallel_count] = (enter_offset, exit_offset);
                    parallel_count += 1;
                }
            }
            marker_idx += 1;
        }

        if parallel_count == 0 {
            return Self::build_single_phase(steps, len, route_guards);
        }

        let mut current_step = 0usize;

        let mut range_idx = 0;
        while range_idx < parallel_count {
            let (enter_eff, exit_eff) = parallel_ranges[range_idx];

            let seq_start = current_step;
            let mut seq_end = current_step;
            while seq_end < len && steps[seq_end].eff_index.as_usize() < enter_eff {
                seq_end += 1;
            }

            if seq_end > seq_start && phase_count < MAX_PHASES {
                phases[phase_count] =
                    Self::build_phase_for_range(steps, seq_start, seq_end, route_guards);
                phase_count += 1;
            }

            let par_start = seq_end;
            let mut par_end = par_start;
            while par_end < len && steps[par_end].eff_index.as_usize() < exit_eff {
                par_end += 1;
            }

            if par_end > par_start && phase_count < MAX_PHASES {
                phases[phase_count] =
                    Self::build_phase_for_range(steps, par_start, par_end, route_guards);
                phase_count += 1;
            }

            current_step = par_end;
            range_idx += 1;
        }

        if current_step < len && phase_count < MAX_PHASES {
            phases[phase_count] =
                Self::build_phase_for_range(steps, current_step, len, route_guards);
            phase_count += 1;
        }

        if phase_count == 0 {
            return Self::build_single_phase(steps, len, route_guards);
        }

        (phases, phase_count)
    }

    /// Build a phase for a specific step range.
    const fn build_phase_for_range(
        steps: &[LocalStep; MAX_STEPS],
        start: usize,
        end: usize,
        route_guards: &[PhaseRouteGuard; MAX_STEPS],
    ) -> Phase {
        let mut phase = Phase::EMPTY;
        let mut lane_lens = [0usize; MAX_LANES];
        let mut lane_first = [usize::MAX; MAX_LANES];

        let mut i = start;
        while i < end {
            let lane = steps[i].lane as usize;
            if lane < MAX_LANES {
                if lane_first[lane] == usize::MAX {
                    lane_first[lane] = i;
                }
                lane_lens[lane] += 1;
            }
            i += 1;
        }

        let mut lane_mask = 0u8;
        let mut min_start = usize::MAX;
        let mut lane_idx = 0;
        while lane_idx < MAX_LANES {
            if lane_lens[lane_idx] > 0 {
                let start = lane_first[lane_idx];
                phase.lanes[lane_idx] = LaneSteps {
                    start,
                    len: lane_lens[lane_idx],
                };
                lane_mask |= 1u8 << (lane_idx as u32);
                if start < min_start {
                    min_start = start;
                }
            }
            lane_idx += 1;
        }
        phase.lane_mask = lane_mask;
        phase.min_start = if lane_mask == 0 { 0 } else { min_start };
        phase.route_guard = Self::route_guard_for_range(route_guards, start, end);

        phase
    }

    const fn route_guard_for_range(
        route_guards: &[PhaseRouteGuard; MAX_STEPS],
        start: usize,
        end: usize,
    ) -> PhaseRouteGuard {
        if start >= end || start >= MAX_STEPS {
            return PhaseRouteGuard::EMPTY;
        }
        let guard = route_guards[start];
        let mut idx = start + 1;
        while idx < end && idx < MAX_STEPS {
            let candidate = route_guards[idx];
            if !guard.matches(candidate) {
                return PhaseRouteGuard::EMPTY;
            }
            idx += 1;
        }
        guard
    }

    /// Borrow the underlying global effect list.
    #[inline(always)]
    pub const fn eff_list(&self) -> &'prog EffList {
        self.eff_list
    }

    /// Static LeaseGraph capacity requirements inferred from policy markers.
    #[inline(always)]
    pub(crate) const fn lease_budget(&self) -> crate::control::lease::planner::LeaseGraphBudget {
        self.lease_budget
    }

    /// Returns an array indicating which lanes are active in this program.
    ///
    /// A lane is considered active if it has at least one step in any phase.
    /// This is used by `SessionCluster::enter()` to acquire all necessary lane resources.
    ///
    /// # Returns
    ///
    /// An array of booleans where `result[i]` is `true` if lane `i` is active.
    pub(crate) fn active_lanes(&self) -> [bool; MAX_LANES] {
        let layout = self.layout();
        let mut active = [false; MAX_LANES];
        for phase_idx in 0..layout.phase_count() {
            let phase = &layout.phases[phase_idx];
            for lane_idx in 0..MAX_LANES {
                if phase.lanes[lane_idx].is_active() {
                    active[lane_idx] = true;
                }
            }
        }
        active
    }

    /// Create a PhaseCursor positioned at the initial node.
    ///
    /// PhaseCursor is the unified cursor type for typestate navigation,
    /// supporting both linear and phase-driven multi-lane execution.
    #[inline(always)]
    pub(crate) fn phase_cursor(&'prog self) -> PhaseCursor<ROLE> {
        PhaseCursor::new(self)
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
    let _ = super::typestate::RoleTypestate::<ROLE>::from_program(eff);
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
        let role_zero = ROLE_ZERO.projection();
        assert_eq!(role_zero.len(), 2);
        assert!(role_zero.steps()[0].is_send());
        assert!(role_zero.steps()[1].is_recv());
        assert_eq!(role_zero.steps()[0].peer(), 1);
        assert_eq!(role_zero.steps()[1].peer(), 1);

        let role_one = ROLE_ONE.projection();
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
        let cancel_role = CANCEL_ROLE.projection();
        assert_eq!(cancel_role.len(), 1);
        let step = cancel_role.steps()[0];
        assert!(step.is_local_action());
        assert!(step.is_control);
        assert_eq!(step.shot, Some(CapShot::One));
    }

    #[test]
    fn local_action_projects_as_local_step() {
        let local_role = LOCAL_ROLE.projection();
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
        let chain_role = CHAIN_ROLE.projection();
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

        let client_projection = client.projection();
        let server_projection = server.projection();

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
