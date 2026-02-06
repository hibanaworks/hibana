//! Role-local program representation derived from const `EffList`.
//!
//! `RoleProgram` materialises the subset of a global effect list that is
//! relevant to a particular role. It keeps the original `EffList` reference so
//! control-plane metadata (scope/control markers) remain accessible, while also
//! providing a compact slice of `LocalStep` entries for IDE inspection and
//! lightweight runtime checks.

use super::{
    program::Program,
    steps::{self, ProjectRole},
    typestate::{PhaseCursor, RoleTypestate, ScopeAtlasView, ScopeRegionIter},
};
use crate::control::cap::{CapShot, DefaultMintConfig, MintConfigMarker};
use crate::g::{KnownRole, Role};
use crate::{
    eff::{self, EffIndex, EffKind},
    global::const_dsl::{
        ControlMarker, ControlPlanMarker, EffList, HandlePlan, ScopeEvent, ScopeId, ScopeKind,
        ScopeMarker,
    },
    observe::ScopeTrace,
};

const MAX_STEPS: usize = eff::meta::MAX_EFF_NODES;
/// Maximum number of parallel phases in a program.
const MAX_PHASES: usize = 32;
/// Maximum number of concurrent lanes (matches RoleLaneSet::LANE_COUNT).
pub const MAX_LANES: usize = 8;

/// Steps for a single lane within a phase.
///
/// References a contiguous slice of `LocalStep` entries within the RoleProgram's
/// `local_steps` array. Empty lanes have `len == 0`.
#[derive(Clone, Copy, Debug, Default)]
pub struct LaneSteps {
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
pub struct PhaseRouteGuard {
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
pub struct Phase {
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

    /// Whether this phase has any active lanes.
    #[inline(always)]
    pub const fn is_empty(&self) -> bool {
        self.lane_mask == 0
    }

    /// Count of active lanes in this phase.
    #[inline]
    pub const fn active_lane_count(&self) -> usize {
        let mut mask = self.lane_mask;
        let mut count = 0usize;
        while mask != 0 {
            count += (mask & 1) as usize;
            mask >>= 1;
        }
        count
    }

    /// Get steps for a specific lane.
    #[inline(always)]
    pub const fn lane(&self, lane: u8) -> LaneSteps {
        self.lanes[lane as usize]
    }
}

/// Local direction of a step in the projected program.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LocalDirection {
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
pub struct LocalStep {
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
        eff_index: 0,
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

    /// Direction (send/recv) carried by this step.
    #[inline(always)]
    pub const fn direction(&self) -> LocalDirection {
        self.direction
    }

    /// Whether the step corresponds to a control-plane label.
    #[inline(always)]
    pub const fn is_control(&self) -> bool {
        self.is_control
    }

    /// Shot discipline attached to the control message (if any).
    #[inline(always)]
    pub const fn shot(&self) -> Option<CapShot> {
        self.shot
    }

    /// Optional resource identifier for control-plane coordination.
    #[inline(always)]
    pub const fn resource(&self) -> Option<u8> {
        self.resource
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

    /// Lightweight view of this step for label/flag reconstruction.
    #[inline(always)]
    pub const fn meta(&self) -> LocalStepMeta {
        LocalStepMeta {
            eff_index: self.eff_index,
            label: self.label,
            peer: self.peer,
            direction: self.direction,
            is_control: self.is_control,
            lane: self.lane,
        }
    }
}

/// Compact metadata for label/flags reconstruction without carrying full steps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LocalStepMeta {
    pub eff_index: EffIndex,
    pub label: u8,
    pub peer: u8,
    pub direction: LocalDirection,
    pub is_control: bool,
    pub lane: u8,
}

impl LocalStepMeta {
    pub const EMPTY: Self = Self {
        eff_index: 0,
        label: 0,
        peer: 0,
        direction: LocalDirection::None,
        is_control: false,
        lane: 0,
    };
}

/// Role-specific view over a global effect list.
///
/// ## Phased Multi-Lane Architecture
///
/// `RoleProgram` supports parallel execution through phases:
/// - **phases**: Array of `Phase` structures, each representing a fork-join barrier
/// - **local_steps**: Flat array of all steps; phases index into this array
///
/// For non-parallel programs (most common case), there is a single phase with
/// all steps on Lane 0.
///
/// For `g::par` programs, steps are distributed across phases and lanes according
/// to the choreography structure.
#[derive(Clone, Copy)]
pub struct RoleProgram<'prog, const ROLE: u8, LocalSteps = steps::StepNil, Mint = DefaultMintConfig>
where
    Mint: MintConfigMarker,
{
    eff_list: &'prog EffList,
    lease_budget: crate::control::LeaseGraphBudget,
    local_steps: [LocalStep; MAX_STEPS],
    local_metas: [LocalStepMeta; MAX_STEPS],
    meta_by_eff: [LocalStepMeta; MAX_STEPS],
    local_len: usize,
    /// Phased structure for multi-lane parallel execution.
    phases: [Phase; MAX_PHASES],
    phase_len: usize,
    typestate: RoleTypestate<ROLE>,
    mint: Mint,
    _local_steps: core::marker::PhantomData<LocalSteps>,
}

/// Control plan metadata exposed to runtime initialisation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ControlPlanInfo {
    pub eff_index: EffIndex,
    pub label: u8,
    pub plan: HandlePlan,
    pub scope_id: ScopeId,
    /// Packed `{range,nest}` trace derived from the typestate atlas (if any).
    pub scope_trace: Option<ScopeTrace>,
    /// Canonical control resource tag attached to the send atom (if any).
    pub resource_tag: Option<u8>,
}

pub struct ControlPlanIter<'prog, const ROLE: u8> {
    eff_list: &'prog EffList,
    markers: &'prog [ControlPlanMarker],
    idx: usize,
    cursor: PhaseCursor<ROLE>,
}

impl<'prog, const ROLE: u8> ControlPlanIter<'prog, ROLE> {
    fn new(eff_list: &'prog EffList, cursor: PhaseCursor<ROLE>) -> Self {
        Self {
            eff_list,
            markers: eff_list.control_plans(),
            idx: 0,
            cursor,
        }
    }
}

impl<'prog, const ROLE: u8> Iterator for ControlPlanIter<'prog, ROLE> {
    type Item = ControlPlanInfo;

    fn next(&mut self) -> Option<Self::Item> {
        if self.idx >= self.markers.len() {
            return None;
        }
        let marker = self.markers[self.idx];
        self.idx += 1;

        let offset = marker.offset;
        let eff_index = offset as EffIndex;
        let node = self
            .eff_list
            .as_slice()
            .get(offset)
            .unwrap_or_else(|| panic!("control plan offset {} out of bounds", offset));

        debug_assert!(
            matches!(node.kind, eff::EffKind::Atom),
            "control plan offset must reference an atom"
        );

        let atom = node.atom_data();
        let label = atom.label;
        let resource_tag = atom.resource;
        let scope_id = self
            .eff_list
            .scope_id_for_offset(offset)
            .unwrap_or_else(ScopeId::none);
        let scope_trace = self
            .cursor
            .scope_region_by_id(scope_id)
            .map(|region| ScopeTrace::new(region.range, region.nest));
        Some(ControlPlanInfo {
            eff_index,
            label,
            plan: marker.plan.with_scope(scope_id),
            scope_id,
            scope_trace,
            resource_tag,
        })
    }
}

impl<'prog, const ROLE: u8, LocalSteps, Mint> RoleProgram<'prog, ROLE, LocalSteps, Mint>
where
    Mint: MintConfigMarker,
{
    const fn new(
        eff_list: &'prog EffList,
        steps: [LocalStep; MAX_STEPS],
        len: usize,
        typestate: RoleTypestate<ROLE>,
        mint: Mint,
    ) -> Self {
        let mut metas = [LocalStepMeta::EMPTY; MAX_STEPS];
        let mut meta_idx = 0usize;
        while meta_idx < len {
            metas[meta_idx] = steps[meta_idx].meta();
            meta_idx += 1;
        }
        let mut meta_by_eff = [LocalStepMeta::EMPTY; MAX_STEPS];
        let mut by_eff_idx = 0usize;
        while by_eff_idx < len {
            let meta = metas[by_eff_idx];
            let eff_idx = meta.eff_index as usize;
            if eff_idx >= MAX_STEPS {
                panic!("eff index overflow");
            }
            if !matches!(meta_by_eff[eff_idx].direction, LocalDirection::None) {
                panic!("duplicate eff index in local steps");
            }
            meta_by_eff[eff_idx] = meta;
            by_eff_idx += 1;
        }
        let budget = crate::control::LeaseGraphBudget::from_eff_list(eff_list);
        budget.validate();

        // Build phases from steps based on lane assignment and g::par scope boundaries
        let (phases, phase_len) = Self::build_phases(&steps, len, eff_list);

        Self {
            eff_list,
            lease_budget: budget,
            local_steps: steps,
            local_metas: metas,
            meta_by_eff,
            local_len: len,
            phases,
            phase_len,
            typestate,
            mint,
            _local_steps: core::marker::PhantomData,
        }
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
            let eff_index = steps[step_idx].eff_index as usize;
            while marker_idx < scope_markers.len()
                && scope_markers[marker_idx].offset <= eff_index
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
            while seq_end < len && (steps[seq_end].eff_index as usize) < enter_eff {
                seq_end += 1;
            }

            if seq_end > seq_start && phase_count < MAX_PHASES {
                phases[phase_count] =
                    Self::build_phase_for_range(steps, seq_start, seq_end, route_guards);
                phase_count += 1;
            }

            let par_start = seq_end;
            let mut par_end = par_start;
            while par_end < len && (steps[par_end].eff_index as usize) < exit_eff {
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

    /// Static LeaseGraph capacity requirements inferred from control plans.
    #[inline(always)]
    pub const fn lease_budget(&self) -> crate::control::LeaseGraphBudget {
        self.lease_budget
    }

    /// Number of local steps participating in this program.
    #[inline(always)]
    pub const fn len(&self) -> usize {
        self.local_len
    }

    /// Number of phases in this program.
    #[inline(always)]
    pub const fn phase_count(&self) -> usize {
        self.phase_len
    }

    /// Access a specific phase by index.
    #[inline(always)]
    pub const fn phase(&self, index: usize) -> &Phase {
        &self.phases[index]
    }

    /// Borrow the phases slice.
    #[inline(always)]
    pub fn phases(&self) -> &[Phase] {
        &self.phases[..self.phase_len]
    }

    /// Returns an array indicating which lanes are active in this program.
    ///
    /// A lane is considered active if it has at least one step in any phase.
    /// This is used by `attach_cursor()` to acquire all necessary lane resources.
    ///
    /// # Returns
    ///
    /// An array of booleans where `result[i]` is `true` if lane `i` is active.
    pub fn active_lanes(&self) -> [bool; MAX_LANES] {
        let mut active = [false; MAX_LANES];
        for phase_idx in 0..self.phase_len {
            let phase = &self.phases[phase_idx];
            for lane_idx in 0..MAX_LANES {
                if phase.lanes[lane_idx].is_active() {
                    active[lane_idx] = true;
                }
            }
        }
        active
    }

    /// Borrow the projected local steps as a slice.
    #[inline(always)]
    pub fn steps(&self) -> &[LocalStep] {
        &self.local_steps[..self.local_len]
    }

    /// Lookup minimal metadata for an eff_index (O(1) table lookup).
    #[inline]
    pub fn step_meta_for(&self, eff_index: EffIndex) -> Option<LocalStepMeta> {
        let idx = eff_index as usize;
        if idx >= MAX_STEPS {
            return None;
        }
        let meta = self.meta_by_eff[idx];
        if meta.direction == LocalDirection::None {
            None
        } else {
            Some(meta)
        }
    }

    /// Iterate over metadata views for all local steps.
    #[inline]
    pub fn step_metas(&self) -> impl Iterator<Item = LocalStepMeta> + '_ {
        self.local_metas[..self.local_len].iter().copied()
    }

    /// Borrow a table view over local metadata for quick lookup.
    #[inline(always)]
    pub fn meta_table(&self) -> LocalMetaTable<'_> {
        LocalMetaTable {
            metas: &self.local_metas[..self.local_len],
            meta_by_eff: &self.meta_by_eff,
        }
    }

    /// Iterate over scope regions recorded in the typestate atlas.
    #[inline]
    pub fn scope_regions(&self) -> ScopeRegionIter<'_> {
        self.typestate.scope_regions()
    }

    /// Borrow a view over the typestate scope atlas for canonical lookups.
    #[inline(always)]
    pub fn scope_atlas_view(&self) -> ScopeAtlasView<'_> {
        self.typestate.scope_atlas_view()
    }

    /// Whether the projected program is empty for the role.
    #[inline(always)]
    pub const fn is_empty(&self) -> bool {
        self.local_len == 0
    }

    /// Iterate over scope markers defined at the global level.
    #[inline(always)]
    pub fn scope_markers(&self) -> &'prog [ScopeMarker] {
        self.eff_list.scope_markers()
    }

    /// Iterate over control markers defined at the global level.
    #[inline(always)]
    pub fn control_markers(&self) -> &'prog [ControlMarker] {
        self.eff_list.control_markers()
    }

    /// Iterate over control plan metadata (eff_index + plan).
    #[inline(always)]
    pub fn control_plans(&self) -> ControlPlanIter<'prog, ROLE> {
        ControlPlanIter::new(self.eff_list, self.phase_cursor())
    }

    /// Borrow the synthesized typestate graph.
    #[inline(always)]
    pub(crate) const fn typestate(&self) -> &RoleTypestate<ROLE> {
        &self.typestate
    }

    /// Create a PhaseCursor positioned at the initial node.
    ///
    /// PhaseCursor is the unified cursor type for typestate navigation,
    /// supporting both linear and phase-driven multi-lane execution.
    #[cfg(feature = "test-utils")]
    #[inline(always)]
    pub fn phase_cursor(&'prog self) -> PhaseCursor<ROLE> {
        PhaseCursor::new(self)
    }

    /// Create a PhaseCursor positioned at the initial node.
    #[cfg(not(feature = "test-utils"))]
    #[inline(always)]
    pub(crate) fn phase_cursor(&'prog self) -> PhaseCursor<ROLE> {
        PhaseCursor::new(self)
    }

    /// Total number of local steps tracked for this role.
    #[inline(always)]
    pub const fn local_len(&self) -> usize {
        self.local_len
    }

    /// Mint configuration baked into the RoleProgram.
    #[inline(always)]
    pub const fn mint_config(&self) -> Mint {
        self.mint
    }
}

/// Immutable view over local step metadata for a role.
pub struct LocalMetaTable<'a> {
    metas: &'a [LocalStepMeta],
    meta_by_eff: &'a [LocalStepMeta; MAX_STEPS],
}

impl<'a> LocalMetaTable<'a> {
    /// Iterate over all metadata entries.
    #[inline]
    pub fn iter(&self) -> impl Iterator<Item = LocalStepMeta> + '_ {
        self.metas.iter().copied()
    }

    /// Lookup metadata by eff_index.
    #[inline]
    pub fn get(&self, eff_index: EffIndex) -> Option<LocalStepMeta> {
        let idx = eff_index as usize;
        if idx >= MAX_STEPS {
            return None;
        }
        let meta = self.meta_by_eff[idx];
        if meta.direction == LocalDirection::None {
            None
        } else {
            Some(meta)
        }
    }
}

impl<const ROLE: u8, LocalSteps, Mint> RoleProgram<'static, ROLE, LocalSteps, Mint>
where
    Mint: MintConfigMarker,
{
    /// Const-friendly accessor for local step metadata.
    #[inline(always)]
    pub const fn step_meta(&'static self, idx: usize) -> LocalStep {
        if idx >= self.local_len {
            panic!("step index out of bounds");
        }
        self.local_steps[idx]
    }

    /// Accessor for the synthesized typestate graph with `'static` lifetime.
    #[inline(always)]
    pub const fn step_graph(&'static self) -> &'static RoleTypestate<ROLE> {
        &self.typestate
    }
}

impl<'prog, const ROLE: u8, LocalSteps, Mint> core::ops::Deref
    for RoleProgram<'prog, ROLE, LocalSteps, Mint>
where
    Mint: MintConfigMarker,
{
    type Target = [LocalStep];

    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        &self.local_steps[..self.local_len]
    }
}

impl<'prog, const ROLE: u8, LocalSteps, Mint> AsRef<[LocalStep]>
    for RoleProgram<'prog, ROLE, LocalSteps, Mint>
where
    Mint: MintConfigMarker,
{
    #[inline(always)]
    fn as_ref(&self) -> &[LocalStep] {
        &self.local_steps[..self.local_len]
    }
}

/// Internal helper shared by const and runtime projection paths.
const fn build_from_slice_with_mint<const ROLE: u8, LocalSteps, Mint>(
    program: &'static EffList,
    slice: &'static [eff::EffStruct],
    mint: Mint,
) -> RoleProgram<'static, ROLE, LocalSteps, Mint>
where
    Mint: MintConfigMarker,
{
    let mut steps = [LocalStep::EMPTY; MAX_STEPS];
    let mut len = 0usize;
    let mut idx = 0usize;

    while idx < slice.len() {
        let node = slice[idx];
        if matches!(node.kind, EffKind::Atom) {
            let atom = node.atom_data();
            let eff_index = idx as EffIndex;
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

    let typestate = super::typestate::RoleTypestate::<ROLE>::from_static(program);

    RoleProgram::new(program, steps, len, typestate, mint)
}

/// Project a typed program into the local view for `ROLE` (const context).
pub const fn project<const ROLE: u8, Steps, Mint>(
    program: &'static Program<Steps>,
) -> RoleProgram<'static, ROLE, <Steps as ProjectRole<Role<ROLE>>>::Output, Mint>
where
    Role<ROLE>: KnownRole,
    Steps: ProjectRole<Role<ROLE>>,
    Mint: MintConfigMarker,
{
    let eff = program.eff_list();
    build_from_slice_with_mint::<ROLE, <Steps as ProjectRole<Role<ROLE>>>::Output, Mint>(
        eff,
        eff.as_static_slice(),
        Mint::INSTANCE,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::cap::{CapShot, GenericCapToken, resource_kinds::CancelKind};
    use crate::g::{self, LocalProgram, Msg, Role};
    use crate::global::steps::{self, StepCons, StepNil};

    type Client = Role<0>;
    type Server = Role<1>;

    type StepsGlobal = StepCons<
        steps::SendStep<Client, Server, Msg<1, ()>, 0>,
        StepCons<steps::SendStep<Server, Client, Msg<2, ()>, 0>, StepNil>,
    >;

    const PROTOCOL: Program<StepsGlobal> = g::seq(
        g::send::<Client, Server, Msg<1, ()>, 0>(),
        g::send::<Server, Client, Msg<2, ()>, 0>(),
    );

    type ClientLocal = LocalProgram<Client, StepsGlobal>;
    type ServerLocal = LocalProgram<Server, StepsGlobal>;

    const ROLE_ZERO: RoleProgram<'static, 0, ClientLocal> = project::<0, StepsGlobal, _>(&PROTOCOL);
    const ROLE_ONE: RoleProgram<'static, 1, ServerLocal> = project::<1, StepsGlobal, _>(&PROTOCOL);

    // CancelMsg uses CanonicalControl which requires self-send (From == To)
    type CancelMsg = Msg<
        { crate::runtime::consts::LABEL_CANCEL },
        GenericCapToken<CancelKind>,
        crate::g::CanonicalControl<CancelKind>,
    >;

    // Self-send for CanonicalControl (Client→Client)
    type CancelSteps = StepCons<steps::SendStep<Client, Client, CancelMsg, 0>, StepNil>;

    const CANCEL_PROGRAM: Program<CancelSteps> = g::send::<Client, Client, CancelMsg, 0>();

    type CancelLocal = LocalProgram<Client, CancelSteps>;

    const CANCEL_ROLE: RoleProgram<'static, 0, CancelLocal> =
        project::<0, CancelSteps, _>(&CANCEL_PROGRAM);

    type LocalMsg = Msg<5, ()>;
    type LocalSteps = StepCons<steps::SendStep<Client, Client, LocalMsg, 0>, StepNil>;

    const LOCAL_PROGRAM: Program<LocalSteps> = g::send::<Client, Client, LocalMsg, 0>();

    type LocalProjection = LocalProgram<Client, LocalSteps>;

    const LOCAL_ROLE: RoleProgram<'static, 0, LocalProjection> =
        project::<0, LocalSteps, _>(&LOCAL_PROGRAM);

    #[test]
    fn projection_extracts_role_view() {
        assert_eq!(ROLE_ZERO.len(), 2);
        assert!(!ROLE_ZERO.is_empty());
        assert!(ROLE_ZERO[0].is_send());
        assert!(ROLE_ZERO[1].is_recv());
        assert_eq!(ROLE_ZERO[0].peer(), 1);
        assert_eq!(ROLE_ZERO[1].peer(), 1);

        assert_eq!(ROLE_ONE.len(), 2);
        assert!(ROLE_ONE[0].is_recv());
        assert!(ROLE_ONE[1].is_send());

        assert_eq!(
            ROLE_ZERO.control_markers().len(),
            PROTOCOL.eff_list().control_markers().len()
        );
        assert_eq!(
            ROLE_ONE.scope_markers().len(),
            PROTOCOL.eff_list().scope_markers().len()
        );

        let ts_zero = ROLE_ZERO.typestate();
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

        let ts_one = ROLE_ONE.typestate();
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
        assert_eq!(CANCEL_ROLE.len(), 1);
        let step = CANCEL_ROLE[0];
        assert!(step.is_local_action());
        assert!(step.is_control());
        assert_eq!(step.shot(), Some(CapShot::One));
    }

    #[test]
    fn local_action_projects_as_local_step() {
        assert_eq!(LOCAL_ROLE.len(), 1);
        let step = LOCAL_ROLE[0];
        assert!(step.is_local_action());
        let ts = LOCAL_ROLE.typestate();
        assert_eq!(ts.len(), 2);
        assert!(matches!(
            ts.node(0).action(),
            super::super::typestate::LocalAction::Local { .. }
        ));
        assert!(ts.node(1).action().is_terminal());
    }
}
