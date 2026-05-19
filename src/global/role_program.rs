//! Role-local program representation derived from const `EffList`.
//!
//! `RoleProgram` is the typed entry point for a role projection witness.
//! Crate-private lowering facts stay behind this module and the compiled layer.

use super::compiled::lowering::{CompiledProgramImage, ProgramStamp, RoleCompiledCounts};
use super::program::{BuildProgramSource, Program, validated_program_image};
use crate::global::const_dsl::{CompactScopeId, ScopeEvent, ScopeId, ScopeKind, ScopeMarker};
use core::marker::PhantomData;

pub(crate) use core::primitive::usize as LaneWord;
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DenseLaneOrdinal(u16);

impl DenseLaneOrdinal {
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
pub(crate) const LANE_SET_VIEW_WORDS: usize = lane_word_count(LANE_DOMAIN_SIZE);
const LANE_DOMAIN_BYTES: usize = lane_byte_count(LANE_DOMAIN_SIZE);

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

#[inline(always)]
const fn lane_byte_count(lane_count: usize) -> usize {
    if lane_count == 0 {
        0
    } else {
        lane_count.div_ceil(u8::BITS as usize)
    }
}

#[inline(always)]
const fn lane_byte_index(lane: usize) -> (usize, u8) {
    let bits = u8::BITS as usize;
    (lane / bits, 1u8 << (lane % bits))
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct LaneSetView<'a> {
    ptr: *const u8,
    word_len: u16,
    byte_len: u16,
    _marker: PhantomData<&'a [LaneWord]>,
}

impl<'a> LaneSetView<'a> {
    const WORD_MODE: u16 = u16::MAX;
    pub(crate) const EMPTY: Self = Self {
        ptr: core::ptr::null(),
        word_len: 0,
        byte_len: 0,
        _marker: PhantomData,
    };

    #[inline]
    pub(crate) const fn from_parts(ptr: *const LaneWord, word_len: usize) -> Self {
        if word_len > u16::MAX as usize {
            panic!("lane word count overflow");
        }
        if word_len > LANE_SET_VIEW_WORDS {
            panic!("lane word count exceeds lane-domain storage");
        }
        Self {
            ptr: ptr.cast::<u8>(),
            word_len: word_len as u16,
            byte_len: Self::WORD_MODE,
            _marker: PhantomData,
        }
    }

    #[inline]
    pub(crate) const fn from_bytes(ptr: *const u8, byte_len: usize, word_len: usize) -> Self {
        if byte_len > u16::MAX as usize || word_len > u16::MAX as usize {
            panic!("lane set byte count overflow");
        }
        if word_len > LANE_SET_VIEW_WORDS {
            panic!("lane word count exceeds lane-domain storage");
        }
        Self {
            ptr,
            word_len: word_len as u16,
            byte_len: byte_len as u16,
            _marker: PhantomData,
        }
    }

    #[inline(always)]
    const fn is_word_mode(self) -> bool {
        self.byte_len == Self::WORD_MODE
    }

    #[inline(always)]
    const fn byte_len(self) -> usize {
        if self.is_word_mode() {
            0
        } else {
            self.byte_len as usize
        }
    }

    #[inline(always)]
    pub(crate) const fn word_len(self) -> usize {
        self.word_len as usize
    }

    #[inline(always)]
    pub(crate) fn contains(self, lane: usize) -> bool {
        if !self.is_word_mode() {
            let (byte_idx, bit) = lane_byte_index(lane);
            return byte_idx < self.byte_len() && (self.byte_at(byte_idx) & bit) != 0;
        }
        let (word_idx, bit) = lane_word_index(lane);
        if word_idx >= self.word_len() {
            return false;
        }
        (self.word_at(word_idx) & bit) != 0
    }

    #[inline(always)]
    pub(crate) fn is_empty(self) -> bool {
        if !self.is_word_mode() {
            let mut idx = 0usize;
            while idx < self.byte_len() {
                if self.byte_at(idx) != 0 {
                    return false;
                }
                idx += 1;
            }
            return true;
        }
        let mut idx = 0usize;
        while idx < self.word_len() {
            if self.word_at(idx) != 0 {
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
            let lhs = self.word_at(idx);
            let rhs = other.word_at(idx);
            if lhs != rhs {
                return false;
            }
            idx += 1;
        }
        true
    }

    #[inline(always)]
    pub(crate) fn word_at(self, word_idx: usize) -> LaneWord {
        if word_idx >= self.word_len() || self.ptr.is_null() {
            return 0;
        }
        if self.is_word_mode() {
            unsafe { *self.ptr.cast::<LaneWord>().add(word_idx) }
        } else {
            let bits = LaneWord::BITS as usize;
            let word_start = word_idx.saturating_mul(bits);
            let word_end = word_start.saturating_add(bits);
            let mut word = 0usize;
            let mut byte_idx = word_start / (u8::BITS as usize);
            while byte_idx < self.byte_len()
                && byte_idx.saturating_mul(u8::BITS as usize) < word_end
            {
                let lane_start = byte_idx.saturating_mul(u8::BITS as usize);
                if lane_start >= word_start {
                    word |= (self.byte_at(byte_idx) as LaneWord) << (lane_start - word_start);
                }
                byte_idx += 1;
            }
            word
        }
    }

    #[inline(always)]
    fn byte_at(self, idx: usize) -> u8 {
        if self.ptr.is_null() || idx >= self.byte_len() {
            0
        } else {
            unsafe { *self.ptr.add(idx) }
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
        if !self.is_word_mode() {
            let bits = u8::BITS as usize;
            let mut byte_idx = start / bits;
            let mut bit_offset = start % bits;
            while byte_idx < self.byte_len() && byte_idx.saturating_mul(bits) < lane_limit {
                let mut byte = self.byte_at(byte_idx);
                byte &= u8::MAX << bit_offset;
                while byte != 0 {
                    let lane = byte_idx
                        .saturating_mul(bits)
                        .saturating_add(byte.trailing_zeros() as usize);
                    if lane < lane_limit {
                        return Some(lane);
                    }
                    return None;
                }
                byte_idx += 1;
                bit_offset = 0;
            }
            return None;
        }
        let bits = LaneWord::BITS as usize;
        let mut word_idx = start / bits;
        let mut bit_offset = start % bits;
        while word_idx < self.word_len() && word_idx.saturating_mul(bits) < lane_limit {
            let mut word = self.word_at(word_idx);
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

impl<'a, 'b> PartialEq<LaneSetView<'b>> for LaneSetView<'a> {
    #[inline(always)]
    fn eq(&self, other: &LaneSetView<'b>) -> bool {
        (*self).equals(*other)
    }
}

impl Eq for LaneSetView<'_> {}

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
    pub(crate) fn view(&self) -> LaneSetView<'_> {
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
    pub(crate) fn copy_from(&mut self, src: LaneSetView<'_>) {
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
    /// First offset into the RoleProgram's local step stream for contiguous rows.
    pub start: u16,
    /// Number of steps in this lane.
    pub len: u16,
    /// True when this lane's steps are not contiguous within the phase row.
    pub sparse: bool,
}

impl LaneSteps {
    /// Whether this lane has any steps.
    #[inline(always)]
    pub const fn is_active(&self) -> bool {
        self.len > 0
    }

    #[inline(always)]
    pub const fn is_contiguous(&self) -> bool {
        !self.sparse
    }
}

const MAX_PHASE_LANE_ROWS: usize = u8::MAX as usize + 1;
const MAX_PHASE_BOUNDARY_ROWS: usize = MAX_PHASE_LANE_ROWS + 1;
const MAX_LOCAL_STEP_LANES: usize = crate::eff::meta::MAX_EFF_NODES;
const MAX_ROUTE_SCOPE_LANE_ROWS: usize = crate::eff::meta::MAX_EFF_NODES / 2;
const MAX_ROUTE_ARM_LANE_ROWS: usize = MAX_ROUTE_SCOPE_LANE_ROWS * 2;
const MAX_RESIDENT_LANE_BIT_BYTES: usize = LANE_DOMAIN_SIZE * 4;
const PACKED_LANE_RANGE_EMPTY: u32 = u32::MAX;

#[derive(Clone, Copy, Debug)]
struct PackedLaneRange(u32);

impl PackedLaneRange {
    const EMPTY: Self = Self(PACKED_LANE_RANGE_EMPTY);

    #[inline(always)]
    const fn new(start: usize, len: usize) -> Self {
        if start > u16::MAX as usize || len > u16::MAX as usize {
            panic!("lane range descriptor overflow");
        }
        Self(((start as u32) << 16) | len as u32)
    }

    #[inline(always)]
    const fn is_empty(self) -> bool {
        self.0 == PACKED_LANE_RANGE_EMPTY
    }

    #[inline(always)]
    const fn start(self) -> usize {
        (self.0 >> 16) as usize
    }

    const fn len(self) -> usize {
        (self.0 & 0xffff) as usize
    }

    #[inline(always)]
    const fn end(self) -> usize {
        self.start().saturating_add(self.len())
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
    lanes: RoleLaneImage,
}

#[derive(Clone, Copy)]
struct RoleLaneImage {
    local_step_lanes: [u8; MAX_LOCAL_STEP_LANES],
    phase_boundaries: [u16; MAX_PHASE_BOUNDARY_ROWS],
    phase_lane_bit_boundaries: [u16; MAX_PHASE_BOUNDARY_ROWS],
    lane_bit_rows: [u8; MAX_RESIDENT_LANE_BIT_BYTES],
    route_arm_lane_rows: [PackedLaneRange; MAX_ROUTE_ARM_LANE_ROWS],
    route_offer_lane_rows: [PackedLaneRange; MAX_ROUTE_SCOPE_LANE_ROWS],
    active_lane_row: PackedLaneRange,
    phase_row_len: u16,
    lane_bit_row_len: u16,
    first_active_lane: u16,
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
    const fn new(facts: RoleFacts, source: RoleImageSource, lanes: RoleLaneImage) -> Self {
        Self {
            facts,
            source,
            lanes,
        }
    }
}

impl RoleLaneImage {
    const NO_ACTIVE_LANE: u16 = u16::MAX;

    #[inline(always)]
    const fn same_scope(left: ScopeId, right: ScopeId) -> bool {
        !left.is_none() && left.canonical_raw() == right.canonical_raw()
    }

    #[inline(always)]
    const fn first_enter_for_scope(markers: &[ScopeMarker], marker_idx: usize) -> bool {
        let marker = markers[marker_idx];
        if !matches!(marker.event, ScopeEvent::Enter) {
            return false;
        }
        let mut idx = 0usize;
        while idx < marker_idx {
            let candidate = markers[idx];
            if matches!(candidate.event, ScopeEvent::Enter)
                && Self::same_scope(candidate.scope_id, marker.scope_id)
            {
                return false;
            }
            idx += 1;
        }
        true
    }

    #[inline(always)]
    const fn route_arm_ranges(
        markers: &[ScopeMarker],
        route: ScopeId,
    ) -> Option<[(usize, usize); 2]> {
        if route.is_none() {
            return None;
        }
        let mut starts = [usize::MAX; 2];
        let mut ends = [usize::MAX; 2];
        let mut enter_len = 0usize;
        let mut exit_len = 0usize;
        let mut idx = 0usize;
        while idx < markers.len() {
            let marker = markers[idx];
            if Self::same_scope(marker.scope_id, route)
                && matches!(marker.scope_kind, ScopeKind::Route)
            {
                match marker.event {
                    ScopeEvent::Enter => {
                        if enter_len < 2 {
                            starts[enter_len] = marker.offset;
                        }
                        enter_len += 1;
                    }
                    ScopeEvent::Exit => {
                        if exit_len < 2 {
                            ends[exit_len] = marker.offset;
                        }
                        exit_len += 1;
                    }
                }
            }
            idx += 1;
        }
        if enter_len == 2 && exit_len == 2 {
            Some([(starts[0], ends[0]), (starts[1], ends[1])])
        } else {
            None
        }
    }

    #[inline(always)]
    const fn local_step_range_for_eff_range<const ROLE: u8>(
        program: &CompiledProgramImage,
        start_eff: usize,
        end_eff: usize,
    ) -> PackedLaneRange {
        if start_eff >= end_eff {
            return PackedLaneRange::new(0, 0);
        }
        let view = program.view();
        let mut local_step = 0usize;
        let mut local_start = usize::MAX;
        let mut local_len = 0usize;
        let mut eff_idx = 0usize;
        while eff_idx < view.len() {
            if let Some(atom) = view.atom_at(eff_idx) {
                if atom.from == ROLE || atom.to == ROLE {
                    if eff_idx >= start_eff && eff_idx < end_eff {
                        if local_start == usize::MAX {
                            local_start = local_step;
                        }
                        local_len += 1;
                    }
                    local_step += 1;
                }
            }
            eff_idx += 1;
        }
        if local_start == usize::MAX {
            PackedLaneRange::new(0, 0)
        } else {
            PackedLaneRange::new(local_start, local_len)
        }
    }

    #[inline(always)]
    const fn push_phase_row(&mut self, row: PackedLaneRange) {
        if row.len() == 0 {
            return;
        }
        let idx = self.phase_row_len as usize;
        if idx >= MAX_PHASE_LANE_ROWS {
            panic!("role phase lane row overflow");
        }
        if row.start() > u16::MAX as usize || row.end() > u16::MAX as usize {
            panic!("role phase lane row range overflow");
        }
        let start = row.start() as u16;
        let end = row.end() as u16;
        if idx == 0 {
            self.phase_boundaries[0] = start;
        } else if self.phase_boundaries[idx] != start {
            panic!("role phase lane rows must be contiguous");
        }
        self.phase_boundaries[idx + 1] = end;
        self.phase_row_len += 1;
    }

    #[inline(always)]
    const fn append_lane_bit_row_for_local_range(
        &mut self,
        row: PackedLaneRange,
    ) -> PackedLaneRange {
        if row.is_empty() || row.len() == 0 {
            return PackedLaneRange::new(0, 0);
        }
        if row.end() > MAX_LOCAL_STEP_LANES {
            panic!("resident lane bit row exceeds local lane table");
        }

        let mut bytes = [0u8; LANE_DOMAIN_BYTES];
        let mut max_lane_plus_one = 0usize;
        let mut pos = row.start();
        let end = row.end();
        while pos < end {
            let lane = self.local_step_lanes[pos] as usize;
            let (byte_idx, bit) = lane_byte_index(lane);
            bytes[byte_idx] |= bit;
            let lane_plus_one = lane.saturating_add(1);
            if lane_plus_one > max_lane_plus_one {
                max_lane_plus_one = lane_plus_one;
            }
            pos += 1;
        }

        let byte_len = lane_byte_count(max_lane_plus_one);
        if byte_len == 0 {
            return PackedLaneRange::new(0, 0);
        }
        let start = self.lane_bit_row_len as usize;
        let end = start.saturating_add(byte_len);
        if end > MAX_RESIDENT_LANE_BIT_BYTES || end > u16::MAX as usize {
            panic!("resident lane bit row overflow");
        }
        let mut idx = 0usize;
        while idx < byte_len {
            self.lane_bit_rows[start + idx] = bytes[idx];
            idx += 1;
        }
        self.lane_bit_row_len = end as u16;
        PackedLaneRange::new(start, byte_len)
    }

    #[inline(always)]
    const fn lane_bit_row_byte(&self, row: PackedLaneRange, idx: usize) -> u8 {
        if row.is_empty() || idx >= row.len() {
            0
        } else {
            let offset = row.start().saturating_add(idx);
            if offset >= MAX_RESIDENT_LANE_BIT_BYTES {
                0
            } else {
                self.lane_bit_rows[offset]
            }
        }
    }

    #[inline(always)]
    const fn append_lane_bit_union_row(
        &mut self,
        left: PackedLaneRange,
        right: PackedLaneRange,
    ) -> PackedLaneRange {
        let byte_len = if left.len() > right.len() {
            left.len()
        } else {
            right.len()
        };
        if byte_len == 0 {
            return PackedLaneRange::new(0, 0);
        }
        let start = self.lane_bit_row_len as usize;
        let end = start.saturating_add(byte_len);
        if end > MAX_RESIDENT_LANE_BIT_BYTES || end > u16::MAX as usize {
            panic!("resident lane bit union row overflow");
        }
        let mut idx = 0usize;
        while idx < byte_len {
            self.lane_bit_rows[start + idx] =
                self.lane_bit_row_byte(left, idx) | self.lane_bit_row_byte(right, idx);
            idx += 1;
        }
        self.lane_bit_row_len = end as u16;
        PackedLaneRange::new(start, byte_len)
    }

    #[inline(always)]
    const fn push_phase_lane_bit_rows(&mut self) {
        if self.phase_row_len == 0 {
            return;
        }
        let mut idx = 0usize;
        while idx < self.phase_row_len as usize {
            let bit_row = self.append_lane_bit_row_for_local_range(self.phase_range(idx));
            let start = bit_row.start();
            let end = bit_row.end();
            if start > u16::MAX as usize || end > u16::MAX as usize {
                panic!("resident phase lane bit row overflow");
            }
            if idx == 0 {
                self.phase_lane_bit_boundaries[0] = start as u16;
            } else if self.phase_lane_bit_boundaries[idx] != start as u16 {
                panic!("resident phase lane bit rows must be contiguous");
            }
            self.phase_lane_bit_boundaries[idx + 1] = end as u16;
            idx += 1;
        }
    }

    #[inline(always)]
    const fn push_phase_rows<const ROLE: u8>(&mut self, program: &CompiledProgramImage) {
        let view = program.view();
        let markers = view.scope_markers();
        let mut current_eff = 0usize;
        let mut marker_idx = 0usize;
        while marker_idx < markers.len() {
            let marker = markers[marker_idx];
            if matches!(marker.event, ScopeEvent::Enter)
                && matches!(marker.scope_kind, ScopeKind::Parallel)
            {
                let mut exit_eff = usize::MAX;
                let mut scan = marker_idx + 1;
                while scan < markers.len() {
                    let candidate = markers[scan];
                    if Self::same_scope(candidate.scope_id, marker.scope_id)
                        && matches!(candidate.event, ScopeEvent::Exit)
                    {
                        exit_eff = candidate.offset;
                        break;
                    }
                    scan += 1;
                }
                if exit_eff == usize::MAX {
                    panic!("parallel scope exit missing");
                }
                self.push_phase_row(Self::local_step_range_for_eff_range::<ROLE>(
                    program,
                    current_eff,
                    marker.offset,
                ));
                let parallel_start = if marker.offset > current_eff {
                    marker.offset
                } else {
                    current_eff
                };
                self.push_phase_row(Self::local_step_range_for_eff_range::<ROLE>(
                    program,
                    parallel_start,
                    exit_eff,
                ));
                current_eff = if exit_eff > current_eff {
                    exit_eff
                } else {
                    current_eff
                };
            }
            marker_idx += 1;
        }
        self.push_phase_row(Self::local_step_range_for_eff_range::<ROLE>(
            program,
            current_eff,
            view.len(),
        ));
        if self.phase_row_len == 0 {
            self.push_phase_row(Self::local_step_range_for_eff_range::<ROLE>(
                program,
                0,
                view.len(),
            ));
        }
    }

    #[inline(always)]
    const fn append_route_arm_lane_row<const ROLE: u8>(
        &mut self,
        program: &CompiledProgramImage,
        slot: usize,
        arm: usize,
        start_eff: usize,
        end_eff: usize,
    ) {
        let row_idx = slot.saturating_mul(2).saturating_add(arm);
        if row_idx >= MAX_ROUTE_ARM_LANE_ROWS {
            panic!("route arm lane row overflow");
        }
        let local_row = Self::local_step_range_for_eff_range::<ROLE>(program, start_eff, end_eff);
        self.route_arm_lane_rows[row_idx] = self.append_lane_bit_row_for_local_range(local_row);
    }

    #[inline(always)]
    const fn push_route_arm_lane_rows<const ROLE: u8>(&mut self, program: &CompiledProgramImage) {
        let view = program.view();
        let markers = view.scope_markers();
        let mut route_slot = 0usize;
        let mut marker_idx = 0usize;
        while marker_idx < markers.len() {
            let marker = markers[marker_idx];
            if Self::first_enter_for_scope(markers, marker_idx)
                && matches!(marker.scope_kind, ScopeKind::Route)
            {
                let Some(ranges) = Self::route_arm_ranges(markers, marker.scope_id) else {
                    panic!("route scope missing binary arm ranges");
                };
                let mut arm = 0usize;
                while arm < 2 {
                    let (start, end) = ranges[arm];
                    self.append_route_arm_lane_row::<ROLE>(program, route_slot, arm, start, end);
                    arm += 1;
                }
                if route_slot >= MAX_ROUTE_SCOPE_LANE_ROWS {
                    panic!("route offer lane row overflow");
                }
                let left = self.route_arm_lane_rows[route_slot.saturating_mul(2)];
                let right =
                    self.route_arm_lane_rows[route_slot.saturating_mul(2).saturating_add(1)];
                self.route_offer_lane_rows[route_slot] =
                    self.append_lane_bit_union_row(left, right);
                route_slot += 1;
            }
            marker_idx += 1;
        }
    }

    #[inline(always)]
    const fn from_program<const ROLE: u8>(
        program: &CompiledProgramImage,
        logical_lane_count: usize,
    ) -> Self {
        let mut lanes = Self {
            local_step_lanes: [0; MAX_LOCAL_STEP_LANES],
            phase_boundaries: [0; MAX_PHASE_BOUNDARY_ROWS],
            phase_lane_bit_boundaries: [0; MAX_PHASE_BOUNDARY_ROWS],
            lane_bit_rows: [0; MAX_RESIDENT_LANE_BIT_BYTES],
            route_arm_lane_rows: [PackedLaneRange::EMPTY; MAX_ROUTE_ARM_LANE_ROWS],
            route_offer_lane_rows: [PackedLaneRange::EMPTY; MAX_ROUTE_SCOPE_LANE_ROWS],
            active_lane_row: PackedLaneRange::EMPTY,
            phase_row_len: 0,
            lane_bit_row_len: 0,
            first_active_lane: Self::NO_ACTIVE_LANE,
        };
        let view = program.view();
        let mut step = 0usize;
        let mut idx = 0usize;
        while idx < view.len() {
            if let Some(atom) = view.atom_at(idx) {
                if atom.from == ROLE || atom.to == ROLE {
                    let lane = atom.lane as usize;
                    if lane < logical_lane_count {
                        if lane < lanes.first_active_lane as usize {
                            lanes.first_active_lane = lane as u16;
                        }
                        if step >= MAX_LOCAL_STEP_LANES {
                            panic!("role local lane table overflow");
                        }
                        lanes.local_step_lanes[step] = atom.lane;
                    }
                    step += 1;
                }
            }
            idx += 1;
        }
        lanes.active_lane_row =
            lanes.append_lane_bit_row_for_local_range(PackedLaneRange::new(0, step));
        lanes.push_phase_rows::<ROLE>(program);
        lanes.push_phase_lane_bit_rows();
        lanes.push_route_arm_lane_rows::<ROLE>(program);
        lanes
    }

    #[inline(always)]
    const fn lane_bit_view(&self, range: PackedLaneRange, word_len: usize) -> LaneSetView<'_> {
        if range.is_empty() || range.len() == 0 {
            LaneSetView::from_bytes(core::ptr::null(), 0, word_len)
        } else {
            if range.end() > MAX_RESIDENT_LANE_BIT_BYTES {
                panic!("resident lane bit range exceeds lane bit table");
            }
            LaneSetView::from_bytes(
                unsafe { self.lane_bit_rows.as_ptr().add(range.start()) },
                range.len(),
                word_len,
            )
        }
    }

    #[inline(always)]
    const fn active_lane_set(&self, word_len: usize) -> LaneSetView<'_> {
        self.lane_bit_view(self.active_lane_row, word_len)
    }

    #[inline(always)]
    const fn phase_lane_set(&self, idx: usize, word_len: usize) -> Option<LaneSetView<'_>> {
        if idx >= self.phase_row_len as usize {
            return None;
        }
        let start = self.phase_lane_bit_boundaries[idx] as usize;
        let end = self.phase_lane_bit_boundaries[idx + 1] as usize;
        Some(self.lane_bit_view(
            PackedLaneRange::new(start, end.saturating_sub(start)),
            word_len,
        ))
    }

    #[inline(always)]
    const fn phase_min_start(&self, idx: usize) -> Option<u16> {
        if idx >= self.phase_row_len as usize {
            return None;
        }
        let row = self.phase_range(idx);
        if row.is_empty() || row.len() == 0 {
            None
        } else if row.start() > u16::MAX as usize {
            panic!("phase start exceeds descriptor capacity");
        } else {
            Some(row.start() as u16)
        }
    }

    #[inline(always)]
    const fn phase_lane_steps(&self, idx: usize, lane_idx: usize) -> Option<LaneSteps> {
        if lane_idx > u8::MAX as usize {
            return None;
        }
        if idx >= self.phase_row_len as usize {
            return None;
        }
        let row = self.phase_range(idx);
        let mut pos = row.start();
        let end = row.end();
        let mut first = usize::MAX;
        let mut len = 0usize;
        let mut sparse = false;
        while pos < end && pos < MAX_LOCAL_STEP_LANES {
            if self.local_step_lanes[pos] as usize == lane_idx {
                if first == usize::MAX {
                    first = pos;
                } else if pos != first.saturating_add(len) {
                    sparse = true;
                }
                len += 1;
            }
            pos += 1;
        }
        if len == 0 {
            None
        } else if first > u16::MAX as usize || len > u16::MAX as usize {
            panic!("phase lane steps exceed descriptor capacity");
        } else {
            Some(LaneSteps {
                start: first as u16,
                len: len as u16,
                sparse,
            })
        }
    }

    #[inline(always)]
    const fn phase_lane_step_at(&self, idx: usize, lane_idx: usize, ordinal: usize) -> Option<u16> {
        if lane_idx > u8::MAX as usize {
            return None;
        }
        if idx >= self.phase_row_len as usize {
            return None;
        }
        let row = self.phase_range(idx);
        let mut pos = row.start();
        let end = row.end();
        let mut seen = 0usize;
        while pos < end && pos < MAX_LOCAL_STEP_LANES {
            if self.local_step_lanes[pos] as usize == lane_idx {
                if seen == ordinal {
                    if pos > u16::MAX as usize {
                        panic!("phase lane step index exceeds descriptor capacity");
                    }
                    return Some(pos as u16);
                }
                seen += 1;
            }
            pos += 1;
        }
        None
    }

    #[inline(always)]
    const fn phase_lane_step_ordinal(
        &self,
        idx: usize,
        lane_idx: usize,
        step_idx: usize,
    ) -> Option<u16> {
        if lane_idx > u8::MAX as usize {
            return None;
        }
        if idx >= self.phase_row_len as usize {
            return None;
        }
        let row = self.phase_range(idx);
        if step_idx < row.start() || step_idx >= row.end() || step_idx >= MAX_LOCAL_STEP_LANES {
            return None;
        }
        let mut pos = row.start();
        let end = row.end();
        let mut ordinal = 0usize;
        while pos < end && pos < MAX_LOCAL_STEP_LANES {
            if self.local_step_lanes[pos] as usize == lane_idx {
                if pos == step_idx {
                    if ordinal > u16::MAX as usize {
                        panic!("phase lane step ordinal exceeds descriptor capacity");
                    }
                    return Some(ordinal as u16);
                }
                ordinal += 1;
            }
            pos += 1;
        }
        None
    }

    #[inline(always)]
    const fn first_active_lane(&self) -> Option<usize> {
        if self.first_active_lane == Self::NO_ACTIVE_LANE {
            None
        } else {
            Some(self.first_active_lane as usize)
        }
    }

    #[inline(always)]
    const fn phase_range(&self, idx: usize) -> PackedLaneRange {
        if idx >= self.phase_row_len as usize {
            return PackedLaneRange::EMPTY;
        }
        let start = self.phase_boundaries[idx] as usize;
        let end = self.phase_boundaries[idx + 1] as usize;
        PackedLaneRange::new(start, end.saturating_sub(start))
    }

    #[inline(always)]
    const fn route_scope_arm_lane_set_by_slot(
        &self,
        slot: usize,
        arm: u8,
        logical_lane_word_count: usize,
    ) -> Option<LaneSetView<'_>> {
        if arm >= 2 {
            return None;
        }
        let row_idx = slot.saturating_mul(2).saturating_add(arm as usize);
        if row_idx >= MAX_ROUTE_ARM_LANE_ROWS {
            return None;
        }
        let row = self.route_arm_lane_rows[row_idx];
        if row.is_empty() {
            return None;
        }
        Some(self.lane_bit_view(row, logical_lane_word_count))
    }

    #[inline(always)]
    const fn route_scope_offer_lane_set_by_slot(
        &self,
        slot: usize,
        logical_lane_word_count: usize,
    ) -> Option<LaneSetView<'_>> {
        if slot >= MAX_ROUTE_SCOPE_LANE_ROWS {
            return None;
        }
        let row = self.route_offer_lane_rows[slot];
        if row.is_empty() {
            return None;
        }
        Some(self.lane_bit_view(row, logical_lane_word_count))
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

    #[inline(always)]
    pub(crate) const fn active_lane_set(self) -> LaneSetView<'static> {
        let footprint = self.footprint();
        self.image
            .lanes
            .active_lane_set(footprint.logical_lane_word_count)
    }

    #[inline(always)]
    pub(crate) const fn phase_lane_set(self, idx: usize) -> Option<LaneSetView<'static>> {
        self.image
            .lanes
            .phase_lane_set(idx, self.footprint().logical_lane_word_count)
    }

    #[inline(always)]
    pub(crate) const fn phase_min_start(self, idx: usize) -> Option<u16> {
        self.image.lanes.phase_min_start(idx)
    }

    #[inline(always)]
    pub(crate) const fn phase_lane_steps(self, idx: usize, lane_idx: usize) -> Option<LaneSteps> {
        self.image.lanes.phase_lane_steps(idx, lane_idx)
    }

    #[inline(always)]
    pub(crate) const fn phase_lane_step_at(
        self,
        idx: usize,
        lane_idx: usize,
        ordinal: usize,
    ) -> Option<u16> {
        self.image.lanes.phase_lane_step_at(idx, lane_idx, ordinal)
    }

    #[inline(always)]
    pub(crate) const fn phase_lane_step_ordinal(
        self,
        idx: usize,
        lane_idx: usize,
        step_idx: usize,
    ) -> Option<u16> {
        self.image
            .lanes
            .phase_lane_step_ordinal(idx, lane_idx, step_idx)
    }

    #[inline(always)]
    pub(crate) const fn first_active_lane(self) -> Option<usize> {
        self.image.lanes.first_active_lane()
    }

    #[inline(always)]
    pub(crate) const fn route_scope_arm_lane_set_by_slot(
        self,
        slot: usize,
        arm: u8,
    ) -> Option<LaneSetView<'static>> {
        self.image.lanes.route_scope_arm_lane_set_by_slot(
            slot,
            arm,
            self.footprint().logical_lane_word_count,
        )
    }

    #[inline(always)]
    pub(crate) const fn route_scope_offer_lane_set_by_slot(
        self,
        slot: usize,
    ) -> Option<LaneSetView<'static>> {
        self.image
            .lanes
            .route_scope_offer_lane_set_by_slot(slot, self.footprint().logical_lane_word_count)
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
    const LANES: RoleLaneImage = RoleLaneImage::from_program::<ROLE>(
        validated_program_image::<Steps>(),
        Self::FACTS.footprint().logical_lane_count,
    );
    const IMAGE: RoleImage = RoleImage::new(
        Self::FACTS,
        RoleImageSource::new(Self::program_image),
        Self::LANES,
    );
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
    use crate::eff::{EffAtom, EffStruct};
    use crate::g::{self, Msg, Role};
    use crate::global::compiled::images::RoleDescriptorRef;
    use crate::global::const_dsl::EffList;
    use crate::global::program::boundary_source_program_image;
    use crate::global::steps::{self, ParSteps, RouteSteps, SeqSteps, StepCons, StepNil};

    const LEGACY_TAP_EVENT_ROW_BUDGET: usize = 512;

    const fn test_atom(label: u8, lane: u8) -> EffStruct {
        EffStruct::atom(EffAtom {
            from: 0,
            to: 1,
            label,
            is_control: false,
            resource: None,
            lane,
        })
    }

    const fn over_tap_event_atom_program() -> EffList {
        let mut list = EffList::new();
        let mut idx = 0usize;
        while idx <= LEGACY_TAP_EVENT_ROW_BUDGET {
            list = list.push(test_atom(idx as u8, 0));
            idx += 1;
        }
        list
    }

    static OVER_TAP_EVENT_ATOMS: EffList = over_tap_event_atom_program();

    static OVER_TAP_EVENT_IMAGE: CompiledProgramImage =
        boundary_source_program_image(&OVER_TAP_EVENT_ATOMS);

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

    #[test]
    fn resident_lane_view_and_route_caps_stay_compact() {
        assert!(
            core::mem::size_of::<LaneSetView<'static>>() <= 2 * core::mem::size_of::<usize>(),
            "LaneSetView must stay a borrowed word/list descriptor, not a copied lane set"
        );
        assert_eq!(MAX_LOCAL_STEP_LANES, crate::eff::meta::MAX_EFF_NODES);
        assert!(MAX_ROUTE_SCOPE_LANE_ROWS >= crate::eff::meta::MAX_EFF_NODES / 2);
        assert_eq!(MAX_ROUTE_ARM_LANE_ROWS, MAX_ROUTE_SCOPE_LANE_ROWS * 2);
    }

    #[test]
    fn resident_local_step_capacity_is_not_tied_to_tap_events() {
        assert!(OVER_TAP_EVENT_ATOMS.len() > LEGACY_TAP_EVENT_ROW_BUDGET);
        let lanes = RoleLaneImage::from_program::<0>(
            &OVER_TAP_EVENT_IMAGE,
            logical_lane_count_for_role(1, RESERVED_BINDING_LANES),
        );

        let steps = lanes
            .phase_lane_steps(0, 0)
            .expect("lane 0 must cover every local atom");
        assert_eq!(steps.len as usize, OVER_TAP_EVENT_ATOMS.len());
        assert!(steps.is_contiguous());
        assert_eq!(
            lanes.phase_lane_step_at(0, 0, OVER_TAP_EVENT_ATOMS.len() - 1),
            Some((OVER_TAP_EVENT_ATOMS.len() - 1) as u16)
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

    type MultiPhaseProgramSteps = SeqSteps<
        StepCons<steps::SendStep<Role<0>, Role<1>, Msg<18, ()>, 0>, StepNil>,
        SeqSteps<
            ParSteps<ParallelLane0, ParallelLane1>,
            StepCons<steps::SendStep<Role<0>, Role<1>, Msg<19, ()>, 0>, StepNil>,
        >,
    >;
    fn multi_phase_program() -> Program<MultiPhaseProgramSteps> {
        g::seq(
            g::send::<Role<0>, Role<1>, Msg<18, ()>, 0>(),
            g::seq(
                parallel_program(),
                g::send::<Role<0>, Role<1>, Msg<19, ()>, 0>(),
            ),
        )
    }

    type SplitRouteLeft = SeqSteps<
        StepCons<steps::SendStep<Role<0>, Role<0>, Msg<20, ()>, 0>, StepNil>,
        StepCons<steps::SendStep<Role<0>, Role<1>, Msg<21, ()>, 0>, StepNil>,
    >;
    type SplitRouteRight = SeqSteps<
        StepCons<steps::SendStep<Role<0>, Role<0>, Msg<22, ()>, 1>, StepNil>,
        StepCons<steps::SendStep<Role<0>, Role<1>, Msg<23, ()>, 1>, StepNil>,
    >;
    type SplitRouteProgramSteps = RouteSteps<SplitRouteLeft, SplitRouteRight>;
    fn split_route_left_program() -> Program<SplitRouteLeft> {
        g::seq(
            g::send::<Role<0>, Role<0>, Msg<20, ()>, 0>(),
            g::send::<Role<0>, Role<1>, Msg<21, ()>, 0>(),
        )
    }
    fn split_route_right_program() -> Program<SplitRouteRight> {
        g::seq(
            g::send::<Role<0>, Role<0>, Msg<22, ()>, 1>(),
            g::send::<Role<0>, Role<1>, Msg<23, ()>, 1>(),
        )
    }
    fn split_route_program() -> Program<SplitRouteProgramSteps> {
        g::route(split_route_left_program(), split_route_right_program())
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
    fn resident_phase_rows_cover_multiple_exact_phases() {
        let program: RoleProgram<0> = project(&multi_phase_program());
        with_role_descriptor(&program, |descriptor| {
            let mut lanes = [u8::MAX; 2];

            let phase0 = descriptor.phase_lane_set(0).expect("pre-par phase");
            assert_eq!(
                phase0.write_lane_indices(descriptor.logical_lane_count(), &mut lanes),
                1
            );
            assert_eq!(lanes[0], 0);
            assert_eq!(descriptor.phase_min_start(0), Some(0));
            assert_eq!(
                descriptor.phase_lane_steps(0, 0).map(|steps| steps.len),
                Some(1)
            );

            lanes = [u8::MAX; 2];
            let phase1 = descriptor.phase_lane_set(1).expect("parallel phase");
            assert_eq!(
                phase1.write_lane_indices(descriptor.logical_lane_count(), &mut lanes),
                2
            );
            assert_eq!(lanes, [0, 1]);
            assert_eq!(descriptor.phase_min_start(1), Some(1));
            assert_eq!(descriptor.phase_lane_step_at(1, 0, 0), Some(1));
            assert_eq!(descriptor.phase_lane_step_at(1, 1, 0), Some(2));

            lanes = [u8::MAX; 2];
            let phase2 = descriptor.phase_lane_set(2).expect("post-par phase");
            assert_eq!(
                phase2.write_lane_indices(descriptor.logical_lane_count(), &mut lanes),
                1
            );
            assert_eq!(lanes[0], 0);
            assert_eq!(descriptor.phase_min_start(2), Some(3));
            assert!(descriptor.phase_lane_set(3).is_none());
        });
    }

    #[test]
    fn resident_lane_step_lookup_keeps_noncontiguous_lane_order() {
        let program = g::seq(
            g::send::<Role<0>, Role<1>, Msg<31, ()>, 0>(),
            g::seq(
                g::send::<Role<0>, Role<1>, Msg<32, ()>, 1>(),
                g::send::<Role<0>, Role<1>, Msg<33, ()>, 0>(),
            ),
        );
        let program: RoleProgram<0> = project(&program);
        with_role_descriptor(&program, |descriptor| {
            let lane0 = descriptor.phase_lane_steps(0, 0).expect("lane 0 steps");
            assert_eq!(lane0.start, 0);
            assert_eq!(lane0.len, 2);
            assert!(!lane0.is_contiguous());
            assert_eq!(descriptor.phase_lane_step_at(0, 0, 0), Some(0));
            assert_eq!(descriptor.phase_lane_step_at(0, 0, 1), Some(2));
            assert_eq!(descriptor.phase_lane_step_at(0, 1, 0), Some(1));
            assert_eq!(descriptor.phase_lane_step_ordinal(0, 0, 0), Some(0));
            assert_eq!(descriptor.phase_lane_step_ordinal(0, 0, 2), Some(1));
            assert_eq!(descriptor.phase_lane_step_ordinal(0, 0, 1), None);
        });
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

    #[test]
    fn route_arm_lane_rows_are_resident_and_exact() {
        let route_program = split_route_program();
        let program: RoleProgram<0> = project(&route_program);
        with_role_descriptor(&program, |descriptor| {
            let arm0 = descriptor
                .route_scope_arm_lane_set_by_slot(0, 0)
                .expect("arm 0 route lane row");
            let arm1 = descriptor
                .route_scope_arm_lane_set_by_slot(0, 1)
                .expect("arm 1 route lane row");
            let offer = descriptor
                .route_scope_offer_lane_set_by_slot(0)
                .expect("route offer lane row");
            let mut lanes = [u8::MAX; 2];

            assert_eq!(
                arm0.write_lane_indices(descriptor.logical_lane_count(), &mut lanes),
                1
            );
            assert_eq!(lanes[0], 0);
            lanes = [u8::MAX; 2];
            assert_eq!(
                arm1.write_lane_indices(descriptor.logical_lane_count(), &mut lanes),
                1
            );
            assert_eq!(lanes[0], 1);
            lanes = [u8::MAX; 2];
            assert_eq!(
                offer.write_lane_indices(descriptor.logical_lane_count(), &mut lanes),
                2
            );
            assert_eq!(lanes, [0, 1]);
        });
    }

    #[test]
    fn lane_resident_route_rows_do_not_restore_full_domain_copies() {
        let packed_route_lane_rows = (MAX_ROUTE_ARM_LANE_ROWS + MAX_ROUTE_SCOPE_LANE_ROWS)
            * core::mem::size_of::<PackedLaneRange>();
        let full_domain_route_lane_rows = (MAX_ROUTE_ARM_LANE_ROWS + MAX_ROUTE_SCOPE_LANE_ROWS)
            * LANE_SET_VIEW_WORDS
            * core::mem::size_of::<LaneWord>();

        assert!(
            packed_route_lane_rows < full_domain_route_lane_rows,
            "route lane rows must stay packed and must not restore full-domain lane-set copies: current={} full_domain={}",
            packed_route_lane_rows,
            full_domain_route_lane_rows
        );
    }
}
