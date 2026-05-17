//! Role-local program representation derived from const `EffList`.
//!
//! `RoleProgram` is the typed entry point for a role projection witness.
//! Crate-private lowering facts stay behind this module and the compiled layer.

use super::compiled::lowering::{CompiledProgramImage, ProgramStamp, RoleCompiledCounts};
use super::program::{BuildProgramSource, Program, validated_program_image};
use crate::global::const_dsl::{CompactScopeId, ScopeId};

pub(crate) use core::primitive::usize as LaneWord;
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DenseLaneOrdinal(u16);

impl DenseLaneOrdinal {
    pub(crate) const ZERO: Self = Self(0);
    pub(crate) const NONE: Self = Self(u16::MAX);

    pub(crate) const fn new(index: usize) -> Option<Self> {
        if index < u16::MAX as usize {
            Some(Self(index as u16))
        } else {
            None
        }
    }

    pub(crate) const fn get(self) -> usize {
        self.0 as usize
    }
}

pub(crate) const LANE_DOMAIN_SIZE: usize = u8::MAX as usize + 1;
pub(crate) const DENSE_LANE_NONE: DenseLaneOrdinal = DenseLaneOrdinal::NONE;
pub(crate) const RESERVED_BINDING_LANES: usize = 2;
const LANE_SET_VIEW_WORDS: usize = lane_word_count(LANE_DOMAIN_SIZE);

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
    words: [LaneWord; LANE_SET_VIEW_WORDS],
    word_len: u16,
}

impl LaneSetView {
    pub(crate) const EMPTY: Self = Self {
        words: [0; LANE_SET_VIEW_WORDS],
        word_len: 0,
    };

    #[inline]
    pub(crate) fn from_parts(ptr: *const LaneWord, word_len: usize) -> Self {
        if word_len > u16::MAX as usize {
            panic!("lane word count overflow");
        }
        if word_len > LANE_SET_VIEW_WORDS {
            panic!("lane word count exceeds lane-domain storage");
        }
        let mut words = [0; LANE_SET_VIEW_WORDS];
        let mut idx = 0usize;
        while idx < word_len {
            words[idx] = unsafe { *ptr.add(idx) };
            idx += 1;
        }
        Self {
            words,
            word_len: word_len as u16,
        }
    }

    #[inline(always)]
    pub(crate) fn from_lane_count(lane_count: usize) -> Self {
        let word_len = lane_word_count(lane_count);
        if word_len > LANE_SET_VIEW_WORDS {
            panic!("lane word count exceeds lane-domain storage");
        }
        Self {
            words: [0; LANE_SET_VIEW_WORDS],
            word_len: word_len as u16,
        }
    }

    #[inline(always)]
    pub(crate) fn insert(&mut self, lane: usize) {
        let (word_idx, bit) = lane_word_index(lane);
        if word_idx < self.word_len() {
            self.words[word_idx] |= bit;
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
        (self.words[word_idx] & bit) != 0
    }

    #[inline(always)]
    pub(crate) fn is_empty(self) -> bool {
        let mut idx = 0usize;
        while idx < self.word_len() {
            if self.words[idx] != 0 {
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
            let lhs = self.words[idx];
            let rhs = other.words[idx];
            if lhs != rhs {
                return false;
            }
            idx += 1;
        }
        true
    }

    #[inline(always)]
    fn word_at(self, word_idx: usize) -> LaneWord {
        if word_idx >= self.word_len() {
            0
        } else {
            self.words[word_idx]
        }
    }

    #[inline(always)]
    fn lane_limit_mask(word_idx: usize, lane_limit: usize) -> LaneWord {
        let bits = LaneWord::BITS as usize;
        let word_start = word_idx.saturating_mul(bits);
        if word_start >= lane_limit {
            return 0;
        }
        let remaining = lane_limit - word_start;
        if remaining >= bits {
            LaneWord::MAX
        } else {
            (1usize << remaining) - 1
        }
    }

    #[inline(always)]
    fn equals_until_with_ignored_lane(
        self,
        other: Self,
        lane_limit: usize,
        ignored_lane: Option<usize>,
    ) -> bool {
        let word_limit = lane_word_count(lane_limit);
        let mut word_idx = 0usize;
        while word_idx < word_limit {
            let mut mask = Self::lane_limit_mask(word_idx, lane_limit);
            if let Some(lane) = ignored_lane
                && lane < lane_limit
            {
                let (ignored_word, ignored_bit) = lane_word_index(lane);
                if ignored_word == word_idx {
                    mask &= !ignored_bit;
                }
            }
            if (self.word_at(word_idx) & mask) != (other.word_at(word_idx) & mask) {
                return false;
            }
            word_idx += 1;
        }
        true
    }

    #[inline(always)]
    pub(crate) fn equals_until(self, other: Self, lane_limit: usize) -> bool {
        self.equals_until_with_ignored_lane(other, lane_limit, None)
    }

    #[inline(always)]
    pub(crate) fn equals_until_except_lane(
        self,
        other: Self,
        lane_limit: usize,
        ignored_lane: usize,
    ) -> bool {
        self.equals_until_with_ignored_lane(other, lane_limit, Some(ignored_lane))
    }

    #[inline(always)]
    pub(crate) fn first_set(self, lane_limit: usize) -> Option<usize> {
        self.next_set_from(0, lane_limit)
    }

    #[inline(always)]
    pub(crate) fn next_set_from(self, start: usize, lane_limit: usize) -> Option<usize> {
        if start >= lane_limit {
            return None;
        }
        let bits = LaneWord::BITS as usize;
        let mut word_idx = start / bits;
        let mut bit_offset = start % bits;
        while word_idx < self.word_len() && word_idx.saturating_mul(bits) < lane_limit {
            let mut word = self.words[word_idx];
            word &= LaneWord::MAX << bit_offset;
            while word != 0 {
                let lane = word_idx
                    .saturating_mul(bits)
                    .saturating_add(word.trailing_zeros() as usize);
                if lane < lane_limit {
                    return Some(lane);
                }
                return None;
            }
            word_idx += 1;
            bit_offset = 0;
        }
        None
    }

    #[cfg(test)]
    #[inline(always)]
    pub(crate) fn write_lane_indices(self, lane_limit: usize, dst: &mut [u8]) -> usize {
        let mut written = 0usize;
        let mut next = self.first_set(lane_limit);
        while let Some(lane) = next {
            assert!(
                written < dst.len(),
                "lane-index destination is too small for the exact lane set"
            );
            dst[written] = u8::try_from(lane).expect("lane index exceeds public lane width");
            written += 1;
            next = self.next_set_from(lane.saturating_add(1), lane_limit);
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
    pub(crate) fn view(&self) -> LaneSetView {
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
                self.ptr.add(idx).write(src.word_at(idx));
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
    let requested = if reserved > endpoint_lane_slot_count {
        reserved
    } else {
        endpoint_lane_slot_count
    };
    if requested > LANE_DOMAIN_SIZE {
        LANE_DOMAIN_SIZE
    } else {
        requested
    }
}

/// Steps for a single lane within the resident role descriptor.
///
/// The resident descriptor computes local nodes directly from the compiled
/// choreography image, so this is only a compact lane range cursor.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct LaneSteps {
    /// Start offset into the RoleProgram's `local_steps` array.
    pub start: u16,
    /// Number of steps in this lane.
    pub len: u16,
}

impl LaneSteps {
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
    #[inline(always)]
    pub const fn is_empty(&self) -> bool {
        self.scope.is_none()
    }

    #[inline(always)]
    pub(crate) const fn scope(self) -> ScopeId {
        self.scope.to_scope_id()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LabelUniverseViolation {
    pub(crate) max: u8,
    pub(crate) actual: u8,
}

#[derive(Clone, Copy)]
struct RoleImage {
    facts: RoleFacts,
    source: RoleImageSource,
}

#[derive(Clone, Copy)]
pub(crate) struct RoleFacts {
    words: [u16; 14],
}

#[derive(Clone, Copy)]
pub(crate) struct RoleImageRef {
    image: &'static RoleImage,
}

#[derive(Clone, Copy)]
pub(crate) struct RoleImageSource {
    program_image: fn() -> &'static CompiledProgramImage,
}

impl RoleImageSource {
    #[inline(always)]
    const fn new(program_image: fn() -> &'static CompiledProgramImage) -> Self {
        Self { program_image }
    }

    #[inline(always)]
    pub(crate) fn program_image(self) -> &'static CompiledProgramImage {
        (self.program_image)()
    }
}

mod private {
    pub trait RoleProgramViewSeal {}
}

pub(crate) trait RoleProgramView<const ROLE: u8>: private::RoleProgramViewSeal {
    fn compiled_role_image(&self) -> &'static crate::global::compiled::images::CompiledRoleImage;
}

#[derive(Clone, Copy)]
pub(crate) struct RoleFootprint {
    #[cfg(test)]
    pub(crate) scope_count: usize,
    #[cfg(test)]
    pub(crate) max_active_scope_depth: usize,
    #[cfg(test)]
    pub(crate) eff_count: usize,
    #[cfg(test)]
    pub(crate) phase_count: usize,
    #[cfg(test)]
    pub(crate) phase_lane_entry_count: usize,
    #[cfg(test)]
    pub(crate) phase_lane_word_count: usize,
    #[cfg(test)]
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
    pub(crate) const fn frontier_entry_count_for_route_depth(route_depth: usize) -> usize {
        if route_depth == 0 {
            1
        } else {
            let doubled = route_depth.saturating_mul(2);
            if doubled > u8::BITS as usize {
                u8::BITS as usize
            } else if doubled == 0 {
                1
            } else {
                doubled
            }
        }
    }

    #[cfg(test)]
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
            #[cfg(test)]
            scope_count: 0,
            #[cfg(test)]
            max_active_scope_depth: 0,
            #[cfg(test)]
            eff_count: 0,
            #[cfg(test)]
            phase_count: 0,
            #[cfg(test)]
            phase_lane_entry_count: 0,
            #[cfg(test)]
            phase_lane_word_count: 0,
            #[cfg(test)]
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

impl RoleImage {
    #[inline(always)]
    const fn new(facts: RoleFacts, source: RoleImageSource) -> Self {
        Self { facts, source }
    }
}

impl RoleFacts {
    #[cfg(test)]
    const SCOPE_COUNT: usize = 0;
    #[cfg(test)]
    const MAX_ACTIVE_SCOPE_DEPTH: usize = 1;
    const MAX_ROUTE_STACK_DEPTH: usize = 2;
    #[cfg(test)]
    const EFF_COUNT: usize = 3;
    const LOCAL_STEP_COUNT: usize = 4;
    #[cfg(test)]
    const PHASE_COUNT: usize = 5;
    #[cfg(test)]
    const PHASE_LANE_ENTRY_COUNT: usize = 6;
    #[cfg(test)]
    const PHASE_LANE_WORD_COUNT: usize = 7;
    #[cfg(test)]
    const PARALLEL_ENTER_COUNT: usize = 8;
    const ROUTE_SCOPE_COUNT: usize = 9;
    const PASSIVE_LINGER_ROUTE_SCOPE_COUNT: usize = 10;
    const ACTIVE_LANE_COUNT: usize = 11;
    const ENDPOINT_LANE_SLOT_COUNT: usize = 12;
    const LOGICAL_LANE_COUNT: usize = 13;

    #[inline(always)]
    const fn compact_count(value: usize) -> u16 {
        if value > u16::MAX as usize {
            panic!("role descriptor fact overflow");
        }
        value as u16
    }

    #[inline(always)]
    const fn from_counts(counts: RoleCompiledCounts) -> Self {
        Self {
            words: [
                Self::compact_count(counts.scope_count),
                Self::compact_count(counts.max_active_scope_depth),
                Self::compact_count(counts.max_route_stack_depth),
                Self::compact_count(counts.eff_count),
                Self::compact_count(counts.local_step_count),
                Self::compact_count(counts.phase_count),
                Self::compact_count(counts.phase_lane_entry_count),
                Self::compact_count(counts.phase_lane_word_count),
                Self::compact_count(counts.parallel_enter_count),
                Self::compact_count(counts.route_scope_count),
                Self::compact_count(counts.passive_linger_route_scope_count),
                Self::compact_count(counts.active_lane_count),
                Self::compact_count(counts.endpoint_lane_slot_count),
                Self::compact_count(counts.logical_lane_count),
            ],
        }
    }

    #[inline(always)]
    const fn footprint(self) -> RoleFootprint {
        RoleFootprint {
            #[cfg(test)]
            scope_count: self.words[Self::SCOPE_COUNT] as usize,
            #[cfg(test)]
            max_active_scope_depth: self.words[Self::MAX_ACTIVE_SCOPE_DEPTH] as usize,
            max_route_stack_depth: self.words[Self::MAX_ROUTE_STACK_DEPTH] as usize,
            #[cfg(test)]
            eff_count: self.words[Self::EFF_COUNT] as usize,
            #[cfg(test)]
            phase_count: self.words[Self::PHASE_COUNT] as usize,
            #[cfg(test)]
            phase_lane_entry_count: self.words[Self::PHASE_LANE_ENTRY_COUNT] as usize,
            #[cfg(test)]
            phase_lane_word_count: self.words[Self::PHASE_LANE_WORD_COUNT] as usize,
            #[cfg(test)]
            parallel_enter_count: self.words[Self::PARALLEL_ENTER_COUNT] as usize,
            route_scope_count: self.words[Self::ROUTE_SCOPE_COUNT] as usize,
            local_step_count: self.words[Self::LOCAL_STEP_COUNT] as usize,
            passive_linger_route_scope_count: self.words[Self::PASSIVE_LINGER_ROUTE_SCOPE_COUNT]
                as usize,
            active_lane_count: self.words[Self::ACTIVE_LANE_COUNT] as usize,
            endpoint_lane_slot_count: self.words[Self::ENDPOINT_LANE_SLOT_COUNT] as usize,
            logical_lane_count: self.words[Self::LOGICAL_LANE_COUNT] as usize,
            logical_lane_word_count: lane_word_count(self.words[Self::LOGICAL_LANE_COUNT] as usize),
            scope_evidence_count: self.words[Self::ROUTE_SCOPE_COUNT] as usize,
            frontier_entry_count: RoleFootprint::frontier_entry_count_for_route_depth(
                self.words[Self::MAX_ROUTE_STACK_DEPTH] as usize,
            ),
        }
    }
}

impl RoleImageRef {
    #[inline(always)]
    const fn new(image: &'static RoleImage) -> Self {
        Self { image }
    }

    #[inline(always)]
    pub(crate) const fn footprint(self) -> RoleFootprint {
        self.image.facts.footprint()
    }

    #[inline(always)]
    pub(crate) fn program_image(self) -> &'static CompiledProgramImage {
        self.image.source.program_image()
    }
}

struct ValidatedRoleImage<Steps, const ROLE: u8>(core::marker::PhantomData<Steps>);

impl<Steps, const ROLE: u8> ValidatedRoleImage<Steps, ROLE>
where
    Steps: BuildProgramSource,
{
    fn program_image() -> &'static CompiledProgramImage {
        validated_program_image::<Steps>()
    }

    const STAMP: ProgramStamp = validated_program_image::<Steps>().stamp();
    const FACTS: RoleFacts =
        RoleFacts::from_counts(validated_program_image::<Steps>().role_lowering_counts::<ROLE>());
    const IMAGE: RoleImage = RoleImage::new(Self::FACTS, RoleImageSource::new(Self::program_image));
    const COMPILED_IMAGE: crate::global::compiled::images::CompiledRoleImage =
        crate::global::compiled::images::CompiledRoleImage::new(
            crate::global::compiled::images::CompiledProgramRef::resident(
                Self::STAMP,
                validated_program_image::<Steps>(),
            ),
            ROLE,
            RoleImageRef::new(&Self::IMAGE),
        );
}

pub struct RoleProgram<const ROLE: u8> {
    image: &'static crate::global::compiled::images::CompiledRoleImage,
}

impl<const ROLE: u8> RoleProgram<ROLE> {
    const fn new(image: &'static crate::global::compiled::images::CompiledRoleImage) -> Self {
        Self { image }
    }

    #[inline(always)]
    pub(crate) const fn compiled_role_image(
        &self,
    ) -> &'static crate::global::compiled::images::CompiledRoleImage {
        self.image
    }
}

impl<const ROLE: u8> private::RoleProgramViewSeal for RoleProgram<ROLE> {}

impl<const ROLE: u8> RoleProgramView<ROLE> for RoleProgram<ROLE> {
    #[inline(always)]
    fn compiled_role_image(&self) -> &'static crate::global::compiled::images::CompiledRoleImage {
        RoleProgram::compiled_role_image(self)
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
    RoleProgram::new(&ValidatedRoleImage::<Steps, ROLE>::COMPILED_IMAGE)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::g::{self, Msg, Role};
    use crate::global::compiled::images::RoleDescriptorRef;
    use crate::global::steps::{self, ParSteps, RouteSteps, SeqSteps, StepCons, StepNil};

    fn with_role_descriptor<const ROLE: u8, R>(
        program: &RoleProgram<ROLE>,
        f: impl FnOnce(RoleDescriptorRef) -> R,
    ) -> R {
        f(RoleDescriptorRef::from_resident(
            program.compiled_role_image(),
        ))
    }

    #[test]
    fn logical_lane_count_stays_inside_wire_lane_domain() {
        assert_eq!(logical_lane_count_for_role(0, 1), RESERVED_BINDING_LANES);
        assert_eq!(logical_lane_count_for_role(254, 255), LANE_DOMAIN_SIZE);
        assert_eq!(logical_lane_count_for_role(255, 256), LANE_DOMAIN_SIZE);
        assert_eq!(logical_lane_count_for_role(256, 256), LANE_DOMAIN_SIZE);
    }

    #[test]
    fn lane_set_view_iterates_set_bits_without_empty_lane_scan() {
        let mut words = [0usize; 4];
        let (word, bit) = lane_word_index(3);
        words[word] |= bit;
        let (word, bit) = lane_word_index(usize::BITS as usize + 5);
        words[word] |= bit;
        let (word, bit) = lane_word_index(usize::BITS as usize * 2 + 1);
        words[word] |= bit;
        let view = LaneSetView::from_parts(words.as_ptr(), words.len());

        assert_eq!(view.first_set(256), Some(3));
        assert_eq!(view.next_set_from(4, 256), Some(usize::BITS as usize + 5));
        assert_eq!(
            view.next_set_from(usize::BITS as usize + 6, 256),
            Some(usize::BITS as usize * 2 + 1),
        );
        assert_eq!(view.next_set_from(usize::BITS as usize * 2 + 2, 256), None,);
        assert_eq!(view.next_set_from(usize::BITS as usize + 6, 65), None);
    }

    #[test]
    fn lane_set_view_word_compare_can_ignore_one_lane_without_empty_lane_scan() {
        let mut lhs = [0usize; 4];
        let mut rhs = [0usize; 4];
        let (word, bit) = lane_word_index(3);
        lhs[word] |= bit;
        rhs[word] |= bit;
        let (word, bit) = lane_word_index(usize::BITS as usize + 5);
        lhs[word] |= bit;
        rhs[word] |= bit;
        let (word, bit) = lane_word_index(usize::BITS as usize + 9);
        lhs[word] |= bit;
        let (word, bit) = lane_word_index(usize::BITS as usize * 3 + 7);
        rhs[word] |= bit;

        let lhs = LaneSetView::from_parts(lhs.as_ptr(), lhs.len());
        let rhs = LaneSetView::from_parts(rhs.as_ptr(), rhs.len());

        assert!(!lhs.equals_until(rhs, usize::BITS as usize * 2));
        assert!(lhs.equals_until_except_lane(
            rhs,
            usize::BITS as usize * 2,
            usize::BITS as usize + 9
        ));
        assert!(
            lhs.equals_until_except_lane(rhs, usize::BITS as usize * 3, usize::BITS as usize + 9),
            "bits beyond the active lane limit are not semantic lane state"
        );
    }

    fn assert_parallel_phase_shape(image: RoleDescriptorRef) {
        let phase_lane_set = image.phase_lane_set(0).expect("phase lane set");
        let mut lanes = [u8::MAX; 2];
        assert_eq!(
            phase_lane_set.write_lane_indices(image.logical_lane_count(), &mut lanes),
            2
        );
        assert_eq!(lanes, [0, 1]);
        assert_eq!(image.phase_lane_steps(0, 0).map(|steps| steps.len), Some(1));
        assert_eq!(image.phase_lane_steps(0, 1).map(|steps| steps.len), Some(1));
        assert!(image.phase_lane_set(1).is_none());
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

        with_role_descriptor(&client, assert_parallel_phase_shape);
        with_role_descriptor(&server, assert_parallel_phase_shape);
    }

    #[test]
    fn parallel_route_projection_keeps_resident_descriptor_without_public_step_surface() {
        let parallel_route_program = parallel_route_program();
        let program: RoleProgram<0> = project(&parallel_route_program);
        with_role_descriptor(&program, |descriptor| {
            assert!(
                descriptor.phase_lane_set(0).is_some(),
                "parallel projection should preserve resident phase lane facts"
            );
            assert!(
                descriptor.route_scope_count() > 0,
                "route projection should preserve resident route scope facts"
            );
        });
    }
}
