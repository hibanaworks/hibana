//! Role-local program representation derived from const `EffList`.
//!
//! `RoleProgram` is the typed entry point for a role projection witness.
//! Crate-private lowering facts stay behind this module and the compiled layer.

use super::compiled::lowering::{LoweringSummary, ProgramStamp, RoleLoweringCounts};
use super::program::{BuildProgramSource, Program, validated_program_summary};
use crate::control::cap::mint::CapShot;
use crate::{
    eff::EffIndex,
    global::const_dsl::{CompactScopeId, ScopeId},
};

pub(crate) use core::primitive::usize as LaneWord;
pub(crate) const RESERVED_BINDING_LANES: usize = 2;

#[inline(always)]
pub(crate) const fn lane_word_count(lane_count: usize) -> usize {
    if lane_count == 0 {
        0
    } else {
        lane_count.div_ceil(LaneWord::BITS as usize)
    }
}

#[inline(always)]
pub(crate) const fn lane_word_index(lane: usize) -> (usize, LaneWord) {
    let bits = LaneWord::BITS as usize;
    (lane / bits, 1usize << (lane % bits))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LaneSetView {
    ptr: *const LaneWord,
    word_len: u16,
}

impl LaneSetView {
    pub(crate) const EMPTY: Self = Self {
        ptr: core::ptr::null(),
        word_len: 0,
    };

    #[inline(always)]
    pub(crate) const fn from_parts(ptr: *const LaneWord, word_len: usize) -> Self {
        if word_len > u16::MAX as usize {
            panic!("lane word count overflow");
        }
        Self {
            ptr,
            word_len: word_len as u16,
        }
    }

    #[inline(always)]
    pub(crate) const fn word_len(self) -> usize {
        self.word_len as usize
    }

    #[inline(always)]
    pub(crate) fn contains(self, lane: usize) -> bool {
        let (word_idx, bit) = lane_word_index(lane);
        if word_idx >= self.word_len() {
            return false;
        }
        unsafe { (*self.ptr.add(word_idx) & bit) != 0 }
    }

    #[inline(always)]
    pub(crate) fn is_empty(self) -> bool {
        let mut idx = 0usize;
        while idx < self.word_len() {
            if unsafe { *self.ptr.add(idx) } != 0 {
                return false;
            }
            idx += 1;
        }
        true
    }

    #[inline(always)]
    pub(crate) fn equals(self, other: Self) -> bool {
        if self.word_len() != other.word_len() {
            return false;
        }
        let mut idx = 0usize;
        while idx < self.word_len() {
            let lhs = unsafe { *self.ptr.add(idx) };
            let rhs = unsafe { *other.ptr.add(idx) };
            if lhs != rhs {
                return false;
            }
            idx += 1;
        }
        true
    }

    #[inline(always)]
    pub(crate) fn first_set(self, lane_limit: usize) -> Option<usize> {
        let mut lane = 0usize;
        while lane < lane_limit {
            if self.contains(lane) {
                return Some(lane);
            }
            lane += 1;
        }
        None
    }

    #[cfg(test)]
    #[inline(always)]
    pub(crate) fn write_lane_indices(self, lane_limit: usize, dst: &mut [u8]) -> usize {
        let mut written = 0usize;
        let mut lane = 0usize;
        while lane < lane_limit {
            if self.contains(lane) {
                assert!(
                    written < dst.len(),
                    "lane-index destination is too small for the exact lane set"
                );
                dst[written] = u8::try_from(lane).expect("lane index exceeds public lane width");
                written += 1;
            }
            lane += 1;
        }
        written
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct LaneSet {
    ptr: *mut LaneWord,
    word_len: u16,
}

impl LaneSet {
    pub(crate) const EMPTY: Self = Self {
        ptr: core::ptr::null_mut(),
        word_len: 0,
    };

    #[inline(always)]
    pub(crate) const fn from_parts(ptr: *mut LaneWord, word_len: usize) -> Self {
        if word_len > u16::MAX as usize {
            panic!("lane word count overflow");
        }
        Self {
            ptr,
            word_len: word_len as u16,
        }
    }

    #[inline(always)]
    pub(crate) unsafe fn init_from_parts(dst: *mut Self, ptr: *mut LaneWord, word_len: usize) {
        if word_len > u16::MAX as usize {
            panic!("lane word count overflow");
        }
        unsafe {
            core::ptr::addr_of_mut!((*dst).ptr).write(ptr);
            core::ptr::addr_of_mut!((*dst).word_len).write(word_len as u16);
        }
        let mut idx = 0usize;
        while idx < word_len {
            unsafe {
                ptr.add(idx).write(0);
            }
            idx += 1;
        }
    }

    #[inline(always)]
    pub(crate) const fn word_len(self) -> usize {
        self.word_len as usize
    }

    #[inline(always)]
    pub(crate) const fn view(&self) -> LaneSetView {
        LaneSetView::from_parts(self.ptr.cast_const(), self.word_len())
    }

    #[inline(always)]
    pub(crate) fn contains(&self, lane: usize) -> bool {
        self.view().contains(lane)
    }

    #[inline(always)]
    pub(crate) fn clear(&mut self) {
        let mut idx = 0usize;
        while idx < self.word_len() {
            unsafe {
                self.ptr.add(idx).write(0);
            }
            idx += 1;
        }
    }

    #[inline(always)]
    pub(crate) fn insert(&mut self, lane: usize) {
        let (word_idx, bit) = lane_word_index(lane);
        if word_idx >= self.word_len() {
            return;
        }
        unsafe {
            let word = self.ptr.add(word_idx);
            word.write(word.read() | bit);
        }
    }

    #[inline(always)]
    pub(crate) fn remove(&mut self, lane: usize) {
        let (word_idx, bit) = lane_word_index(lane);
        if word_idx >= self.word_len() {
            return;
        }
        unsafe {
            let word = self.ptr.add(word_idx);
            word.write(word.read() & !bit);
        }
    }

    #[inline(always)]
    pub(crate) fn copy_from(&mut self, src: LaneSetView) {
        self.clear();
        let len = if self.word_len() < src.word_len() {
            self.word_len()
        } else {
            src.word_len()
        };
        let mut idx = 0usize;
        while idx < len {
            unsafe {
                self.ptr.add(idx).write(*src.ptr.add(idx));
            }
            idx += 1;
        }
    }
}

#[inline(always)]
pub(crate) const fn logical_lane_count_for_role(
    active_lane_count: usize,
    endpoint_lane_slot_count: usize,
) -> usize {
    let reserved = active_lane_count.saturating_add(RESERVED_BINDING_LANES);
    if reserved > endpoint_lane_slot_count {
        reserved
    } else {
        endpoint_lane_slot_count
    }
}

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

/// Erased lowering input derived from a typed `RoleProgram` witness.
#[derive(Clone, Copy)]
pub(crate) struct RoleLoweringInput {
    image: RoleImageRef,
}

#[derive(Clone, Copy)]
struct ProjectedRoleImage {
    summary: &'static LoweringSummary,
    start: EffIndex,
    facts: RoleFacts,
}

#[derive(Clone, Copy)]
pub(crate) struct RoleFacts {
    scope_count: u16,
    eff_count: u16,
    local_step_count: u16,
    phase_count: u16,
    phase_lane_entry_count: u16,
    phase_lane_word_count: u16,
    parallel_enter_count: u16,
    route_scope_count: u16,
    passive_linger_route_scope_count: u16,
    active_lane_count: u16,
    endpoint_lane_slot_count: u16,
    logical_lane_count: u16,
}

#[derive(Clone, Copy)]
pub(crate) struct RoleImageRef {
    image: &'static ProjectedRoleImage,
}

mod private {
    #[derive(Clone, Copy)]
    pub struct RoleProgramSeal;

    pub trait RoleProgramViewSeal {}
}

pub(crate) trait RoleProgramView<const ROLE: u8>: private::RoleProgramViewSeal {
    fn stamp(&self) -> ProgramStamp;
    fn lowering_input(&self) -> RoleLoweringInput;
}

#[derive(Clone, Copy)]
pub(crate) struct RoleFootprint {
    pub(crate) scope_count: usize,
    pub(crate) eff_count: usize,
    pub(crate) phase_count: usize,
    pub(crate) phase_lane_entry_count: usize,
    pub(crate) phase_lane_word_count: usize,
    pub(crate) parallel_enter_count: usize,
    pub(crate) route_scope_count: usize,
    pub(crate) local_step_count: usize,
    pub(crate) passive_linger_route_scope_count: usize,
    pub(crate) active_lane_count: usize,
    pub(crate) endpoint_lane_slot_count: usize,
    pub(crate) logical_lane_count: usize,
    pub(crate) logical_lane_word_count: usize,
    pub(crate) max_route_stack_depth: usize,
    pub(crate) scope_evidence_count: usize,
    pub(crate) frontier_entry_count: usize,
}

impl RoleFootprint {
    #[inline(always)]
    pub(crate) const fn for_endpoint_layout(
        active_lane_count: usize,
        endpoint_lane_slot_count: usize,
        logical_lane_count: usize,
        max_route_stack_depth: usize,
        scope_evidence_count: usize,
        frontier_entry_count: usize,
    ) -> Self {
        let endpoint_lane_slot_count = if endpoint_lane_slot_count == 0 {
            1
        } else {
            endpoint_lane_slot_count
        };
        let logical_lane_seed = if logical_lane_count > endpoint_lane_slot_count {
            logical_lane_count
        } else {
            endpoint_lane_slot_count
        };
        let logical_lane_count = logical_lane_count_for_role(active_lane_count, logical_lane_seed);
        Self {
            scope_count: 0,
            eff_count: 0,
            phase_count: 0,
            phase_lane_entry_count: 0,
            phase_lane_word_count: 0,
            parallel_enter_count: 0,
            route_scope_count: 0,
            local_step_count: 0,
            passive_linger_route_scope_count: 0,
            active_lane_count,
            endpoint_lane_slot_count,
            logical_lane_count,
            logical_lane_word_count: lane_word_count(logical_lane_count),
            max_route_stack_depth,
            scope_evidence_count,
            frontier_entry_count,
        }
    }
}

impl RoleLoweringInput {
    #[inline(always)]
    pub(crate) const fn summary(&self) -> &'static LoweringSummary {
        self.image.summary()
    }

    #[inline(always)]
    pub(crate) const fn stamp(&self) -> ProgramStamp {
        self.image.stamp()
    }

    #[inline(always)]
    pub(crate) const fn start(&self) -> EffIndex {
        self.image.start()
    }

    #[cfg(test)]
    #[inline(always)]
    pub(crate) const fn eff_count(&self) -> usize {
        self.image.footprint().eff_count
    }

    #[cfg(test)]
    #[inline(always)]
    pub(crate) const fn local_step_count(&self) -> usize {
        self.image.footprint().local_step_count
    }

    #[cfg(test)]
    #[inline(always)]
    pub(crate) const fn route_scope_count(&self) -> usize {
        self.image.footprint().route_scope_count
    }

    #[cfg(test)]
    #[inline(always)]
    pub(crate) const fn passive_linger_route_scope_count(&self) -> usize {
        self.image.footprint().passive_linger_route_scope_count
    }

    #[inline(always)]
    pub(crate) const fn footprint(&self) -> RoleFootprint {
        self.image.footprint()
    }
}

impl ProjectedRoleImage {
    #[inline(always)]
    const fn new<const ROLE: u8>(summary: &'static LoweringSummary) -> Self {
        Self {
            summary,
            start: EffIndex::ZERO,
            facts: RoleFacts::from_summary::<ROLE>(summary),
        }
    }

    #[inline(always)]
    const fn summary(&self) -> &'static LoweringSummary {
        self.summary
    }

    #[inline(always)]
    const fn stamp(&self) -> ProgramStamp {
        self.summary.stamp()
    }
}

impl RoleFacts {
    #[inline(always)]
    const fn compact_count(value: usize) -> u16 {
        if value > u16::MAX as usize {
            panic!("role descriptor fact overflow");
        }
        value as u16
    }

    #[inline(always)]
    const fn from_summary<const ROLE: u8>(summary: &'static LoweringSummary) -> Self {
        Self::from_counts(summary.role_lowering_counts::<ROLE>())
    }

    #[inline(always)]
    const fn from_counts(counts: RoleLoweringCounts) -> Self {
        Self {
            scope_count: Self::compact_count(counts.scope_count),
            eff_count: Self::compact_count(counts.eff_count),
            local_step_count: Self::compact_count(counts.local_step_count),
            phase_count: Self::compact_count(counts.phase_count),
            phase_lane_entry_count: Self::compact_count(counts.phase_lane_entry_count),
            phase_lane_word_count: Self::compact_count(counts.phase_lane_word_count),
            parallel_enter_count: Self::compact_count(counts.parallel_enter_count),
            route_scope_count: Self::compact_count(counts.route_scope_count),
            passive_linger_route_scope_count: Self::compact_count(
                counts.passive_linger_route_scope_count,
            ),
            active_lane_count: Self::compact_count(counts.active_lane_count),
            endpoint_lane_slot_count: Self::compact_count(counts.endpoint_lane_slot_count),
            logical_lane_count: Self::compact_count(counts.logical_lane_count),
        }
    }

    #[inline(always)]
    const fn footprint(self) -> RoleFootprint {
        RoleFootprint {
            scope_count: self.scope_count as usize,
            eff_count: self.eff_count as usize,
            phase_count: self.phase_count as usize,
            phase_lane_entry_count: self.phase_lane_entry_count as usize,
            phase_lane_word_count: self.phase_lane_word_count as usize,
            parallel_enter_count: self.parallel_enter_count as usize,
            route_scope_count: self.route_scope_count as usize,
            local_step_count: self.local_step_count as usize,
            passive_linger_route_scope_count: self.passive_linger_route_scope_count as usize,
            active_lane_count: self.active_lane_count as usize,
            endpoint_lane_slot_count: self.endpoint_lane_slot_count as usize,
            logical_lane_count: self.logical_lane_count as usize,
            logical_lane_word_count: lane_word_count(self.logical_lane_count as usize),
            max_route_stack_depth: 0,
            scope_evidence_count: 0,
            frontier_entry_count: 0,
        }
    }
}

impl RoleImageRef {
    #[inline(always)]
    const fn new(image: &'static ProjectedRoleImage) -> Self {
        Self { image }
    }

    #[inline(always)]
    const fn start(self) -> EffIndex {
        self.image.start
    }

    #[inline(always)]
    const fn footprint(self) -> RoleFootprint {
        self.image.facts.footprint()
    }

    #[inline(always)]
    const fn summary(self) -> &'static LoweringSummary {
        self.image.summary()
    }

    #[inline(always)]
    const fn stamp(self) -> ProgramStamp {
        self.image.stamp()
    }
}

struct ValidatedRoleImage<Steps, const ROLE: u8>(core::marker::PhantomData<Steps>);

impl<Steps, const ROLE: u8> ValidatedRoleImage<Steps, ROLE>
where
    Steps: BuildProgramSource,
{
    const IMAGE: ProjectedRoleImage =
        ProjectedRoleImage::new::<ROLE>(validated_program_summary::<Steps>());
}

pub struct RoleProgram<const ROLE: u8> {
    _seal: private::RoleProgramSeal,
    image: RoleImageRef,
}

impl<const ROLE: u8> RoleProgram<ROLE> {
    const fn new(image: &'static ProjectedRoleImage) -> Self {
        Self {
            _seal: private::RoleProgramSeal,
            image: RoleImageRef::new(image),
        }
    }

    #[inline(always)]
    pub(crate) const fn stamp(&self) -> ProgramStamp {
        self.image.stamp()
    }
}

impl<const ROLE: u8> private::RoleProgramViewSeal for RoleProgram<ROLE> {}

impl<const ROLE: u8> RoleProgramView<ROLE> for RoleProgram<ROLE> {
    #[inline(always)]
    fn stamp(&self) -> ProgramStamp {
        RoleProgram::stamp(self)
    }

    #[inline(always)]
    fn lowering_input(&self) -> RoleLoweringInput {
        lowering_input(self)
    }
}

#[inline(always)]
pub(crate) const fn lowering_input<const ROLE: u8>(
    program: &RoleProgram<ROLE>,
) -> RoleLoweringInput {
    RoleLoweringInput {
        image: program.image,
    }
}

/// Project a typed program into the local view for `ROLE`.
#[expect(
    private_bounds,
    reason = "projection source reconstruction is sealed behind typed Program witnesses"
)]
pub const fn project<const ROLE: u8, Steps>(program: &Program<Steps>) -> RoleProgram<ROLE>
where
    Steps: BuildProgramSource,
{
    crate::global::validate_role_index(ROLE);
    let _ = program;
    RoleProgram::new(&ValidatedRoleImage::<Steps, ROLE>::IMAGE)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::g::{self, Msg, Role};
    use crate::global::compiled::images::CompiledRoleImage;
    use crate::global::const_dsl::{ScopeEvent, ScopeKind};
    use crate::global::steps::{self, ParSteps, RouteSteps, SeqSteps, StepCons, StepNil};

    fn with_compiled_role_image<const ROLE: u8, R>(
        program: &RoleProgram<ROLE>,
        f: impl FnOnce(&CompiledRoleImage) -> R,
    ) -> R {
        crate::global::compiled::materialize::with_compiled_role_image::<ROLE, _>(
            crate::global::lowering_input(program),
            f,
        )
    }

    fn assert_parallel_phase_shape(image: &CompiledRoleImage) {
        assert_eq!(image.phase_count(), 1);
        let phase_lane_set = image.phase_lane_set(0).expect("phase lane set");
        let mut lanes = [u8::MAX; 2];
        assert_eq!(
            phase_lane_set.write_lane_indices(image.logical_lane_count(), &mut lanes),
            2
        );
        assert_eq!(lanes, [0, 1]);
        assert_eq!(image.phase_lane_steps(0, 0).map(|steps| steps.len), Some(1));
        assert_eq!(image.phase_lane_steps(0, 1).map(|steps| steps.len), Some(1));
    }

    type ParallelLane0 = StepCons<steps::SendStep<Role<0>, Role<1>, Msg<9, ()>, 0>, StepNil>;
    type ParallelLane1 = StepCons<steps::SendStep<Role<1>, Role<0>, Msg<10, ()>, 1>, StepNil>;
    fn parallel_lane0_program() -> Program<ParallelLane0> {
        g::send::<Role<0>, Role<1>, Msg<9, ()>, 0>()
    }
    fn parallel_lane1_program() -> Program<ParallelLane1> {
        g::send::<Role<1>, Role<0>, Msg<10, ()>, 1>()
    }
    fn parallel_program() -> Program<ParSteps<ParallelLane0, ParallelLane1>> {
        g::par(parallel_lane0_program(), parallel_lane1_program())
    }

    type RouteLeft = SeqSteps<
        StepCons<steps::SendStep<Role<0>, Role<0>, Msg<14, ()>, 0>, StepNil>,
        StepCons<steps::SendStep<Role<0>, Role<1>, Msg<15, ()>, 0>, StepNil>,
    >;
    type RouteRight = SeqSteps<
        StepCons<steps::SendStep<Role<0>, Role<0>, Msg<16, ()>, 0>, StepNil>,
        StepCons<steps::SendStep<Role<0>, Role<1>, Msg<17, ()>, 0>, StepNil>,
    >;
    fn route_left_program() -> Program<RouteLeft> {
        g::seq(
            g::send::<Role<0>, Role<0>, Msg<14, ()>, 0>(),
            g::send::<Role<0>, Role<1>, Msg<15, ()>, 0>(),
        )
    }
    fn route_right_program() -> Program<RouteRight> {
        g::seq(
            g::send::<Role<0>, Role<0>, Msg<16, ()>, 0>(),
            g::send::<Role<0>, Role<1>, Msg<17, ()>, 0>(),
        )
    }
    type RouteProgramSteps = RouteSteps<RouteLeft, RouteRight>;
    fn route_program() -> Program<RouteProgramSteps> {
        g::route(route_left_program(), route_right_program())
    }
    fn parallel_route_program() -> Program<ParSteps<ParallelLane1, RouteProgramSteps>> {
        g::par(parallel_lane1_program(), route_program())
    }

    #[test]
    fn parallel_projection_keeps_phase_and_lane_split_internal() {
        let parallel_program = parallel_program();
        let client: RoleProgram<0> = project(&parallel_program);
        let server: RoleProgram<1> = project(&parallel_program);

        with_compiled_role_image(&client, assert_parallel_phase_shape);
        with_compiled_role_image(&server, assert_parallel_phase_shape);
    }

    #[test]
    fn parallel_route_projection_keeps_scope_markers_without_public_step_surface() {
        let parallel_route_program = parallel_route_program();
        let program: RoleProgram<0> = project(&parallel_route_program);
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
