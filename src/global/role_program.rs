//! Role-local program representation derived from const `EffList`.
//!
//! `RoleProgram` is the typed entry point for a role projection witness.
//! Crate-private lowering facts stay behind this module and the compiled layer.

use core::marker::PhantomData;

use super::compiled::lowering::{ProgramStamp, RoleLoweringCounts};
use super::{
    program::{BuildProgramSource, Program},
    steps::ProjectRole,
};
use crate::control::cap::mint::{CapShot, MintConfig, MintConfigMarker};
use crate::{
    eff::EffIndex,
    global::const_dsl::{CompactScopeId, EffList, ScopeId},
    global::{KnownRole, Role},
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
pub(crate) struct RoleLoweringInput<'prog> {
    _borrow: PhantomData<&'prog EffList>,
    summary: &'static crate::global::compiled::lowering::LoweringSummary,
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

impl<'prog> RoleLoweringInput<'prog> {
    #[inline(always)]
    pub(crate) const fn summary(
        &self,
    ) -> &'static crate::global::compiled::lowering::LoweringSummary {
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
            phase_count: self.counts.phase_count,
            phase_lane_entry_count: self.counts.phase_lane_entry_count,
            phase_lane_word_count: self.counts.phase_lane_word_count,
            parallel_enter_count: self.counts.parallel_enter_count,
            route_scope_count: self.counts.route_scope_count,
            local_step_count: self.counts.local_step_count,
            passive_linger_route_scope_count: self.counts.passive_linger_route_scope_count,
            active_lane_count: self.counts.active_lane_count,
            endpoint_lane_slot_count: self.counts.endpoint_lane_slot_count,
            logical_lane_count: self.counts.logical_lane_count,
            logical_lane_word_count: self.counts.logical_lane_word_count,
            max_route_stack_depth: 0,
            scope_evidence_count: 0,
            frontier_entry_count: 0,
        }
    }
}

pub struct RoleProgram<'prog, const ROLE: u8, Mint = MintConfig>
where
    Mint: MintConfigMarker,
{
    _borrow: PhantomData<&'prog EffList>,
    _seal: private::RoleProgramSeal,
    summary: &'static crate::global::compiled::lowering::LoweringSummary,
    mint: Mint,
    stamp: ProgramStamp,
}

impl<'prog, const ROLE: u8, Mint> RoleProgram<'prog, ROLE, Mint>
where
    Mint: MintConfigMarker,
{
    const fn new(
        summary: &'static crate::global::compiled::lowering::LoweringSummary,
        mint: Mint,
        stamp: ProgramStamp,
    ) -> Self {
        Self {
            _borrow: PhantomData,
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
        self.summary as *const crate::global::compiled::lowering::LoweringSummary as usize
    }

    /// Mint configuration baked into the RoleProgram.
    #[inline(always)]
    pub(crate) const fn mint_config(&self) -> Mint {
        self.mint
    }
}

impl<'prog, const ROLE: u8, Mint> private::RoleProgramViewSeal for RoleProgram<'prog, ROLE, Mint> where
    Mint: MintConfigMarker
{
}

impl<'prog, const ROLE: u8, Mint> RoleProgramView<'prog, ROLE, Mint>
    for RoleProgram<'prog, ROLE, Mint>
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
pub(crate) const fn lowering_input<'prog, const ROLE: u8, Mint>(
    program: &RoleProgram<'prog, ROLE, Mint>,
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
) -> RoleProgram<'prog, ROLE, Mint>
where
    Role<ROLE>: KnownRole,
    Steps: BuildProgramSource + ProjectRole<Role<ROLE>>,
    Mint: MintConfigMarker,
{
    RoleProgram::new(program.summary(), Mint::INSTANCE, program.stamp())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::g::{self, Msg, Role};
    use crate::global::compiled::images::CompiledRoleImage;
    use crate::global::const_dsl::{ScopeEvent, ScopeKind};
    use crate::global::steps::{self, ParSteps, RouteSteps, SeqSteps, StepCons, StepNil};

    fn with_compiled_role_image<const ROLE: u8, R>(
        program: &RoleProgram<'_, ROLE>,
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
        let client: RoleProgram<'_, 0> = project(&parallel_program);
        let server: RoleProgram<'_, 1> = project(&parallel_program);

        with_compiled_role_image(&client, assert_parallel_phase_shape);
        with_compiled_role_image(&server, assert_parallel_phase_shape);
    }

    #[test]
    fn parallel_route_projection_keeps_scope_markers_without_public_step_surface() {
        let parallel_route_program = PARALLEL_ROUTE_PROGRAM;
        let program: RoleProgram<'_, 0> = project(&parallel_route_program);
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
