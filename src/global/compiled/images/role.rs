use crate::endpoint::kernel::{EndpointArenaLayout, FrontierScratchLayout};
use crate::global::ControlDesc;
#[cfg(test)]
use crate::global::compiled::layout::compiled_role_image_bytes_for_counts;
use crate::global::compiled::layout::{
    compiled_role_image_align, compiled_role_image_bytes_for_layout,
};
use crate::global::role_program::{
    DENSE_LANE_NONE, DenseLaneOrdinal, LaneSetView, LaneSteps, LaneWord, PhaseRouteGuard,
    RoleFootprint, lane_word_count, logical_lane_count_for_role,
};
use crate::global::typestate::{RoleTypestateValue, RouteScopeRecord, StateIndex};

pub(in crate::global::compiled) const MACHINE_NO_STEP: u16 = u16::MAX;

#[inline(always)]
pub(in crate::global::compiled) const fn encode_compact_step_index(value: usize) -> u16 {
    if value > u16::MAX as usize {
        panic!("compiled role compact index overflow");
    }
    value as u16
}

#[inline(always)]
pub(in crate::global::compiled) const fn encode_compact_count_u16(value: usize) -> u16 {
    if value > u16::MAX as usize {
        panic!("compiled role compact count overflow");
    }
    value as u16
}

#[inline(always)]
pub(in crate::global::compiled) const fn encode_compact_offset_u16(value: usize) -> u16 {
    if value > u16::MAX as usize {
        panic!("compiled role compact offset overflow");
    }
    value as u16
}

/// Crate-private runtime image for role-local immutable facts.
#[derive(Clone, Debug)]
pub(crate) struct CompiledRoleImage {
    pub(in crate::global::compiled) segment_headers_offset: u16,
    pub(in crate::global::compiled) typestate_offset: u16,
    pub(in crate::global::compiled) phase_headers_offset: u16,
    pub(in crate::global::compiled) phase_lane_entries_offset: u16,
    pub(in crate::global::compiled) phase_lane_words_offset: u16,
    pub(in crate::global::compiled) eff_index_to_step_offset: u16,
    pub(in crate::global::compiled) step_index_to_state_offset: u16,
    pub(in crate::global::compiled) control_by_eff_offset: u16,
    pub(in crate::global::compiled) role: u8,
    pub(in crate::global::compiled) role_facts: RoleResidentFacts,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct PhaseImageHeader {
    pub(in crate::global::compiled) lane_entry_start: u16,
    pub(in crate::global::compiled) lane_entry_len: u16,
    pub(in crate::global::compiled) lane_word_start: u16,
    pub(in crate::global::compiled) lane_word_len: u16,
    pub(in crate::global::compiled) min_start: u16,
    pub(in crate::global::compiled) route_guard: PhaseRouteGuard,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CompiledRoleSegmentHeader {
    pub(in crate::global::compiled) eff_start: u16,
    pub(in crate::global::compiled) eff_len: u16,
    pub(in crate::global::compiled) scope_marker_len: u16,
    pub(in crate::global::compiled) control_marker_len: u16,
    pub(in crate::global::compiled) policy_marker_len: u16,
    pub(in crate::global::compiled) control_desc_len: u16,
}

impl CompiledRoleSegmentHeader {
    pub(in crate::global::compiled) const EMPTY: Self = Self {
        eff_start: 0,
        eff_len: 0,
        scope_marker_len: 0,
        control_marker_len: 0,
        policy_marker_len: 0,
        control_desc_len: 0,
    };
}

impl PhaseImageHeader {
    pub(in crate::global::compiled) const EMPTY: Self = Self {
        lane_entry_start: 0,
        lane_entry_len: 0,
        lane_word_start: 0,
        lane_word_len: 0,
        min_start: 0,
        route_guard: PhaseRouteGuard::EMPTY,
    };
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct PhaseLaneEntry {
    pub(crate) lane: u8,
    pub(crate) steps: LaneSteps,
}

impl PhaseLaneEntry {
    pub(in crate::global::compiled) const EMPTY: Self = Self {
        lane: 0,
        steps: LaneSteps::EMPTY,
    };
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct RoleRuntimeTableView<'a> {
    pub(crate) segment_headers: &'a [CompiledRoleSegmentHeader],
    pub(crate) route_record_by_dense_route: &'a [RouteScopeRecord],
    pub(crate) route_dense_by_scope_slot: &'a [u16],
    pub(crate) route_offer_lane_words_by_dense_route: &'a [LaneWord],
    pub(crate) route_arm0_lane_words_by_dense_route: &'a [LaneWord],
    pub(crate) route_arm1_lane_words_by_dense_route: &'a [LaneWord],
    pub(crate) phase_headers: &'a [PhaseImageHeader],
    pub(crate) control_by_eff: &'a [ControlDesc],
}

#[derive(Clone, Copy, Debug)]
pub(in crate::global::compiled) struct RoleResidentFacts {
    pub(in crate::global::compiled) active_lane_count: u16,
    pub(in crate::global::compiled) endpoint_lane_slot_count: u16,
    pub(in crate::global::compiled) phase_len: u16,
    pub(in crate::global::compiled) phase_lane_entry_len: u16,
    pub(in crate::global::compiled) phase_lane_word_len: u16,
    pub(in crate::global::compiled) eff_index_to_step_len: u16,
    pub(in crate::global::compiled) step_index_to_state_len: u16,
    pub(in crate::global::compiled) persistent_bytes: u16,
}

impl RoleResidentFacts {
    pub(in crate::global::compiled) const EMPTY: Self = Self {
        active_lane_count: 0,
        endpoint_lane_slot_count: 0,
        phase_len: 0,
        phase_lane_entry_len: 0,
        phase_lane_word_len: 0,
        eff_index_to_step_len: 0,
        step_index_to_state_len: 0,
        persistent_bytes: 0,
    };

    #[inline(always)]
    pub(in crate::global::compiled) const fn active_lane_count(self) -> usize {
        self.active_lane_count as usize
    }

    #[inline(always)]
    pub(in crate::global::compiled) const fn endpoint_lane_slot_count(self) -> usize {
        let count = self.endpoint_lane_slot_count as usize;
        if count == 0 { 1 } else { count }
    }

    #[inline(always)]
    pub(in crate::global::compiled) const fn phase_len(self) -> usize {
        self.phase_len as usize
    }

    #[inline(always)]
    pub(in crate::global::compiled) const fn phase_lane_entry_len(self) -> usize {
        self.phase_lane_entry_len as usize
    }

    #[inline(always)]
    pub(in crate::global::compiled) const fn phase_lane_word_len(self) -> usize {
        self.phase_lane_word_len as usize
    }

    #[inline(always)]
    pub(in crate::global::compiled) const fn eff_index_to_step_len(self) -> usize {
        self.eff_index_to_step_len as usize
    }

    #[inline(always)]
    pub(in crate::global::compiled) const fn step_index_to_state_len(self) -> usize {
        self.step_index_to_state_len as usize
    }

    #[inline(always)]
    pub(in crate::global::compiled) const fn persistent_bytes(self) -> usize {
        self.persistent_bytes as usize
    }
}

impl CompiledRoleImage {
    #[inline(always)]
    fn base_ptr(&self) -> *const u8 {
        (self as *const Self).cast::<u8>()
    }

    #[inline(always)]
    fn ptr_at<T>(&self, offset: u16) -> *const T {
        if offset == 0 {
            core::ptr::null()
        } else {
            unsafe { self.base_ptr().add(offset as usize).cast::<T>() }
        }
    }

    #[inline(always)]
    pub(in crate::global::compiled) fn typestate_ptr(&self) -> *const RoleTypestateValue {
        self.ptr_at(self.typestate_offset)
    }

    #[inline(always)]
    pub(in crate::global::compiled) fn segment_headers_ptr(
        &self,
    ) -> *const CompiledRoleSegmentHeader {
        self.ptr_at(self.segment_headers_offset)
    }

    #[inline(always)]
    pub(in crate::global::compiled) fn phase_headers_ptr(&self) -> *const PhaseImageHeader {
        self.ptr_at(self.phase_headers_offset)
    }

    #[inline(always)]
    pub(in crate::global::compiled) fn phase_lane_entries_ptr(&self) -> *const PhaseLaneEntry {
        self.ptr_at(self.phase_lane_entries_offset)
    }

    #[inline(always)]
    pub(in crate::global::compiled) fn phase_lane_words_ptr(&self) -> *const LaneWord {
        self.ptr_at(self.phase_lane_words_offset)
    }

    #[inline(always)]
    pub(in crate::global::compiled) fn eff_index_to_step_ptr(&self) -> *const u16 {
        self.ptr_at(self.eff_index_to_step_offset)
    }

    #[inline(always)]
    pub(in crate::global::compiled) fn step_index_to_state_ptr(&self) -> *const StateIndex {
        self.ptr_at(self.step_index_to_state_offset)
    }

    #[inline(always)]
    pub(in crate::global::compiled) fn control_by_eff_ptr(&self) -> *const ControlDesc {
        self.ptr_at(self.control_by_eff_offset)
    }

    #[cfg(test)]
    #[inline(always)]
    pub(crate) const fn persistent_bytes_for_counts(
        scope_count: usize,
        route_scope_count: usize,
        eff_count: usize,
    ) -> usize {
        compiled_role_image_bytes_for_counts(scope_count, route_scope_count, eff_count)
    }

    #[inline(always)]
    pub(crate) const fn persistent_bytes_for_program(footprint: RoleFootprint) -> usize {
        compiled_role_image_bytes_for_layout(footprint)
    }

    #[inline(always)]
    pub(crate) const fn persistent_align() -> usize {
        compiled_role_image_align()
    }

    #[inline(always)]
    pub(crate) fn actual_persistent_bytes(&self) -> usize {
        self.role_facts.persistent_bytes()
    }

    #[inline(always)]
    pub(crate) const fn role(&self) -> u8 {
        self.role
    }

    #[inline(always)]
    pub(crate) fn local_len(&self) -> usize {
        self.role_facts.step_index_to_state_len()
    }

    #[inline(always)]
    pub(crate) fn runtime_tables(&self) -> RoleRuntimeTableView<'_> {
        let typestate = self.typestate_ref();
        RoleRuntimeTableView {
            segment_headers: self.segment_headers(),
            route_record_by_dense_route: typestate.route_records_table(),
            route_dense_by_scope_slot: typestate.route_dense_by_slot_table(),
            route_offer_lane_words_by_dense_route: typestate.route_offer_lane_words_table(),
            route_arm0_lane_words_by_dense_route: typestate.route_arm0_lane_words_table(),
            route_arm1_lane_words_by_dense_route: typestate.route_arm1_lane_words_table(),
            phase_headers: if self.phase_len() == 0 {
                &[]
            } else {
                unsafe { core::slice::from_raw_parts(self.phase_headers_ptr(), self.phase_len()) }
            },
            control_by_eff: self.control_by_eff(),
        }
    }

    #[inline]
    pub(crate) fn route_scope_dense_ordinal_by_slot(&self, slot: usize) -> Option<usize> {
        let tables = self.runtime_tables();
        let dense = *tables.route_dense_by_scope_slot.get(slot)?;
        if dense == u16::MAX {
            None
        } else {
            Some(dense as usize)
        }
    }

    #[inline]
    pub(crate) fn route_scope_offer_entry_by_slot(&self, slot: usize) -> Option<StateIndex> {
        let dense = self.route_scope_dense_ordinal_by_slot(slot)?;
        Some(
            self.runtime_tables()
                .route_record_by_dense_route
                .get(dense)?
                .offer_entry(),
        )
    }

    #[inline]
    pub(crate) fn route_scope_offer_lane_set_by_slot(&self, slot: usize) -> Option<LaneSetView> {
        let dense = self.route_scope_dense_ordinal_by_slot(slot)?;
        let route = self
            .runtime_tables()
            .route_record_by_dense_route
            .get(dense)?;
        let start = route.offer_lane_word_start();
        let len = self.typestate_ref().route_lane_word_len();
        let end = start.checked_add(len)?;
        let lanes = self
            .runtime_tables()
            .route_offer_lane_words_by_dense_route
            .get(start..end)?;
        Some(LaneSetView::from_parts(lanes.as_ptr(), lanes.len()))
    }

    #[inline]
    pub(crate) fn route_scope_arm_lane_set_by_slot(
        &self,
        slot: usize,
        arm: u8,
    ) -> Option<LaneSetView> {
        let dense = self.route_scope_dense_ordinal_by_slot(slot)?;
        let route = self
            .runtime_tables()
            .route_record_by_dense_route
            .get(dense)?;
        let start = route.route_arm_lane_word_start();
        let len = self.typestate_ref().route_lane_word_len();
        let end = start.checked_add(len)?;
        let tables = self.runtime_tables();
        let lanes = match arm {
            0 => tables
                .route_arm0_lane_words_by_dense_route
                .get(start..end)?,
            1 => tables
                .route_arm1_lane_words_by_dense_route
                .get(start..end)?,
            _ => return None,
        };
        Some(LaneSetView::from_parts(lanes.as_ptr(), lanes.len()))
    }

    #[inline(always)]
    pub(crate) fn segment_headers(&self) -> &[CompiledRoleSegmentHeader] {
        if self.segment_count() == 0 {
            &[]
        } else {
            unsafe { core::slice::from_raw_parts(self.segment_headers_ptr(), self.segment_count()) }
        }
    }

    #[inline(always)]
    pub(crate) fn segment_count(&self) -> usize {
        let eff_count = self.role_facts.eff_index_to_step_len();
        if eff_count == 0 {
            0
        } else {
            eff_count.div_ceil(crate::eff::meta::MAX_SEGMENT_EFFS)
        }
    }

    #[cfg(test)]
    #[inline(always)]
    pub(crate) fn segment_header(&self, segment: usize) -> Option<CompiledRoleSegmentHeader> {
        self.segment_headers().get(segment).copied()
    }

    #[cfg(test)]
    #[inline(always)]
    pub(crate) fn phase_count(&self) -> usize {
        self.phase_len()
    }

    #[inline(always)]
    fn phase_header(&self, idx: usize) -> Option<PhaseImageHeader> {
        if idx >= self.phase_len() {
            return None;
        }
        Some(unsafe { *self.phase_headers_ptr().add(idx) })
    }

    #[inline(always)]
    fn phase_lane_entries_for_header(&self, header: PhaseImageHeader) -> &[PhaseLaneEntry] {
        let start = header.lane_entry_start as usize;
        let len = header.lane_entry_len as usize;
        let total = self.role_facts.phase_lane_entry_len();
        if start > total || len > total.saturating_sub(start) {
            debug_assert!(false, "compiled role phase lane-entry bounds out of range");
            return &[];
        }
        unsafe { core::slice::from_raw_parts(self.phase_lane_entries_ptr().add(start), len) }
    }

    #[inline(always)]
    pub(crate) fn phase_lane_entries(&self, idx: usize) -> &[PhaseLaneEntry] {
        let Some(header) = self.phase_header(idx) else {
            return &[];
        };
        self.phase_lane_entries_for_header(header)
    }

    #[inline(always)]
    fn phase_lane_words_for_header(&self, header: PhaseImageHeader) -> LaneSetView {
        let start = header.lane_word_start as usize;
        let len = header.lane_word_len as usize;
        let total = self.role_facts.phase_lane_word_len();
        if start > total || len > total.saturating_sub(start) {
            debug_assert!(false, "compiled role phase lane-word bounds out of range");
            return LaneSetView::from_parts(core::ptr::null(), 0);
        }
        LaneSetView::from_parts(unsafe { self.phase_lane_words_ptr().add(start) }, len)
    }

    #[inline(always)]
    pub(crate) fn phase_lane_set(&self, idx: usize) -> Option<LaneSetView> {
        self.phase_header(idx)
            .map(|header| self.phase_lane_words_for_header(header))
    }

    #[inline(always)]
    pub(crate) fn phase_min_start(&self, idx: usize) -> Option<u16> {
        self.phase_header(idx).map(|header| header.min_start)
    }

    #[inline(always)]
    pub(crate) fn phase_route_guard(&self, idx: usize) -> Option<PhaseRouteGuard> {
        self.phase_header(idx).map(|header| header.route_guard)
    }

    #[inline(always)]
    pub(crate) fn phase_lane_steps(&self, idx: usize, lane_idx: usize) -> Option<LaneSteps> {
        if lane_idx >= self.logical_lane_count() {
            return None;
        }
        let header = self.phase_header(idx)?;
        let lane_entries = self.phase_lane_entries_for_header(header);
        let mut entry_idx = 0usize;
        while entry_idx < lane_entries.len() {
            let entry = lane_entries[entry_idx];
            if entry.lane as usize == lane_idx {
                return Some(entry.steps);
            }
            entry_idx += 1;
        }
        None
    }

    #[inline(always)]
    pub(crate) fn typestate_ref(&self) -> &RoleTypestateValue {
        debug_assert!(!self.typestate_ptr().is_null());
        unsafe { &*self.typestate_ptr() }
    }

    #[inline(always)]
    pub(crate) fn eff_index_to_step(&self) -> &[u16] {
        unsafe {
            core::slice::from_raw_parts(
                self.eff_index_to_step_ptr(),
                self.role_facts.eff_index_to_step_len(),
            )
        }
    }

    #[inline(always)]
    pub(crate) fn step_index_to_state(&self) -> &[StateIndex] {
        unsafe {
            core::slice::from_raw_parts(
                self.step_index_to_state_ptr(),
                self.role_facts.step_index_to_state_len(),
            )
        }
    }

    #[inline(always)]
    pub(crate) fn control_by_eff(&self) -> &[ControlDesc] {
        unsafe {
            core::slice::from_raw_parts(
                self.control_by_eff_ptr(),
                self.role_facts.eff_index_to_step_len(),
            )
        }
    }

    #[cfg(test)]
    #[inline(always)]
    pub(crate) fn is_active_lane(&self, lane_idx: usize) -> bool {
        self.has_active_lane(lane_idx)
    }

    #[inline(always)]
    pub(crate) fn has_active_lane(&self, lane_idx: usize) -> bool {
        let mut phase_idx = 0usize;
        while phase_idx < self.phase_len() {
            let lane_entries = self.phase_lane_entries(phase_idx);
            let mut entry_idx = 0usize;
            while entry_idx < lane_entries.len() {
                if lane_entries[entry_idx].lane as usize == lane_idx {
                    return true;
                }
                entry_idx += 1;
            }
            phase_idx += 1;
        }
        false
    }

    #[inline(always)]
    pub(crate) fn first_active_lane(&self) -> Option<usize> {
        let mut best = usize::MAX;
        let mut phase_idx = 0usize;
        while phase_idx < self.phase_len() {
            let lane_entries = self.phase_lane_entries(phase_idx);
            let mut entry_idx = 0usize;
            while entry_idx < lane_entries.len() {
                let lane = lane_entries[entry_idx].lane as usize;
                if lane < best {
                    best = lane;
                }
                entry_idx += 1;
            }
            phase_idx += 1;
        }
        if best == usize::MAX { None } else { Some(best) }
    }

    #[cfg(test)]
    #[inline(always)]
    pub(crate) fn controller_arm_entry_by_arm(
        &self,
        scope: crate::global::const_dsl::ScopeId,
        arm: u8,
    ) -> Option<(StateIndex, u8)> {
        self.typestate_ref().controller_arm_entry_by_arm(scope, arm)
    }

    #[cfg(test)]
    #[inline(always)]
    pub(crate) fn first_recv_dispatch_entry(
        &self,
        scope: crate::global::const_dsl::ScopeId,
        idx: usize,
    ) -> Option<(u8, u8, StateIndex)> {
        self.typestate_ref().first_recv_dispatch_entry(scope, idx)
    }

    #[inline(always)]
    pub(crate) fn fill_active_lane_dense_by_lane(&self, dst: &mut [DenseLaneOrdinal]) -> usize {
        Self::build_active_lane_dense_map_into(self, dst)
    }

    #[inline(always)]
    pub(crate) fn fill_logical_lane_dense_by_lane(&self, dst: &mut [DenseLaneOrdinal]) -> usize {
        Self::build_logical_lane_dense_map_into(self.logical_lane_count(), dst)
    }

    #[inline(always)]
    pub(crate) fn logical_lane_count(&self) -> usize {
        logical_lane_count_for_role(self.active_lane_count(), self.endpoint_lane_slot_count())
    }

    #[inline(always)]
    pub(crate) fn logical_lane_word_count(&self) -> usize {
        lane_word_count(self.logical_lane_count())
    }

    #[inline(always)]
    pub(crate) fn endpoint_lane_slot_count(&self) -> usize {
        self.role_facts.endpoint_lane_slot_count()
    }

    #[inline(always)]
    pub(crate) fn max_route_stack_depth(&self) -> usize {
        self.typestate_ref().max_route_stack_depth()
    }

    #[inline(always)]
    pub(crate) fn max_loop_stack_depth(&self) -> usize {
        self.typestate_ref().max_loop_stack_depth()
    }

    #[inline(always)]
    pub(crate) fn route_table_frame_slots(&self) -> usize {
        let lane_slots = self.route_table_lane_slots();
        if lane_slots == 0 {
            return 0;
        }
        lane_slots.saturating_mul(self.max_route_stack_depth().max(1))
    }

    #[inline(always)]
    pub(crate) fn route_table_lane_slots(&self) -> usize {
        if self.runtime_tables().route_record_by_dense_route.is_empty() {
            0
        } else {
            self.endpoint_lane_slot_count()
        }
    }

    #[inline(always)]
    pub(crate) fn loop_table_lane_slots(&self) -> usize {
        if self.max_loop_stack_depth() == 0 {
            0
        } else {
            self.endpoint_lane_slot_count()
        }
    }

    #[inline(always)]
    pub(crate) fn loop_table_slots(&self) -> usize {
        self.loop_table_lane_slots()
            .saturating_mul(self.max_loop_stack_depth())
    }

    #[inline(always)]
    pub(crate) fn resident_cap_entries(&self) -> usize {
        self.active_lane_count().saturating_mul(4).max(4)
    }

    #[inline(always)]
    pub(crate) fn max_frontier_entries(&self) -> usize {
        self.compiled_frontier_entry_capacity()
    }

    #[inline(always)]
    pub(crate) fn route_scope_count(&self) -> usize {
        self.runtime_tables().route_record_by_dense_route.len()
    }

    #[inline(always)]
    pub(crate) fn scope_evidence_count(&self) -> usize {
        self.runtime_tables().route_record_by_dense_route.len()
    }

    #[inline(always)]
    pub(crate) fn endpoint_arena_layout_for_binding(
        &self,
        binding_enabled: bool,
    ) -> EndpointArenaLayout {
        EndpointArenaLayout::from_footprint_with_binding(
            self.endpoint_layout_footprint(),
            binding_enabled,
        )
    }

    #[inline(always)]
    pub(crate) fn frontier_scratch_layout(&self) -> FrontierScratchLayout {
        FrontierScratchLayout::new(
            self.max_frontier_entries(),
            self.logical_lane_count(),
            self.logical_lane_word_count(),
        )
    }

    #[cfg(test)]
    #[inline(always)]
    pub(crate) fn compiled_max_frontier_entries(&self) -> usize {
        self.compiled_frontier_entry_capacity()
    }

    #[cfg(test)]
    #[inline(always)]
    pub(crate) fn compiled_frontier_scratch_layout(&self) -> FrontierScratchLayout {
        FrontierScratchLayout::new(
            self.compiled_max_frontier_entries(),
            self.logical_lane_count(),
            self.logical_lane_word_count(),
        )
    }

    #[inline(always)]
    pub(crate) fn active_lane_count(&self) -> usize {
        self.role_facts.active_lane_count()
    }

    #[inline(always)]
    pub(crate) fn endpoint_layout_footprint(&self) -> RoleFootprint {
        let mut footprint = RoleFootprint::for_endpoint_layout(
            self.active_lane_count(),
            self.endpoint_lane_slot_count(),
            self.logical_lane_count(),
            self.max_route_stack_depth(),
            self.scope_evidence_count(),
            self.max_frontier_entries(),
        );
        footprint.route_scope_count = self.route_scope_count();
        footprint
    }

    #[inline(always)]
    fn phase_len(&self) -> usize {
        self.role_facts.phase_len()
    }

    #[cfg(test)]
    #[inline(always)]
    pub(crate) fn phase_lane_entry_len(&self) -> usize {
        self.role_facts.phase_lane_entry_len()
    }

    #[inline(always)]
    fn compiled_frontier_entry_capacity(&self) -> usize {
        self.typestate_ref().frontier_entry_capacity()
    }

    fn build_active_lane_dense_map_into(image: &Self, dst: &mut [DenseLaneOrdinal]) -> usize {
        dst.fill(DENSE_LANE_NONE);
        let mut phase_idx = 0usize;
        while phase_idx < image.phase_len() {
            if let Some(header) = image.phase_header(phase_idx) {
                let lane_entries = image.phase_lane_entries_for_header(header);
                let mut entry_idx = 0usize;
                while entry_idx < lane_entries.len() {
                    let lane = lane_entries[entry_idx].lane as usize;
                    if lane < dst.len() {
                        dst[lane] = DenseLaneOrdinal::ZERO;
                    }
                    entry_idx += 1;
                }
            }
            phase_idx += 1;
        }
        let mut lane_idx = 0usize;
        let mut dense = 0usize;
        while lane_idx < dst.len() {
            if dst[lane_idx] != DENSE_LANE_NONE {
                dst[lane_idx] =
                    DenseLaneOrdinal::new(dense).expect("dense active lane ordinal fits u16");
                dense += 1;
            }
            lane_idx += 1;
        }
        dense
    }

    fn build_logical_lane_dense_map_into(
        logical_lane_count: usize,
        dst: &mut [DenseLaneOrdinal],
    ) -> usize {
        let mut lane_idx = 0usize;
        while lane_idx < dst.len() {
            dst[lane_idx] = if lane_idx < logical_lane_count {
                DenseLaneOrdinal::new(lane_idx).expect("logical lane ordinal fits u16")
            } else {
                DENSE_LANE_NONE
            };
            lane_idx += 1;
        }
        core::cmp::min(logical_lane_count, dst.len())
    }
}

#[cfg(test)]
mod tests {
    extern crate self as hibana;

    mod fanout_program {
        extern crate self as hibana;
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/internal/pico_smoke/src/fanout_program.rs"
        ));
    }
    mod huge_program {
        extern crate self as hibana;
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/internal/pico_smoke/src/huge_program.rs"
        ));
    }
    mod linear_program {
        extern crate self as hibana;
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/internal/pico_smoke/src/linear_program.rs"
        ));
    }
    mod localside {
        extern crate self as hibana;
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/internal/pico_smoke/src/localside.rs"
        ));
    }
    mod route_localside {
        extern crate self as hibana;
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/internal/pico_smoke/src/route_localside.rs"
        ));
    }
    mod route_control_kinds {
        extern crate self as hibana;
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/internal/pico_smoke/src/route_control_kinds.rs"
        ));
    }
    mod snapshot_control_kind {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/support/snapshot_control.rs"
        ));
    }

    fn drive<F: core::future::Future>(future: F) -> F::Output {
        futures::executor::block_on(future)
    }

    use super::CompiledRoleImage;
    use crate::global::compiled::layout::{
        compiled_role_phase_cap, compiled_role_route_scope_cap, compiled_role_scope_cap,
        compiled_role_step_cap, compiled_role_typestate_node_cap,
    };
    use crate::{
        control::{
            cap::mint::{ControlResourceKind, GenericCapToken, ResourceKind},
            cap::resource_kinds::{LoopBreakKind, LoopContinueKind, RouteDecisionKind},
        },
        eff::meta::MAX_SEGMENT_EFFS,
        g::{self, Msg, Role},
        global::compiled::lowering::LoweringSummary,
        global::{
            role_program,
            steps::{RouteSteps, SendStep, SeqSteps, StepCons, StepNil},
            typestate::{JumpReason, LocalAction},
        },
    };
    use snapshot_control_kind::SnapshotControl;

    const ROUTE_RIGHT_LABEL: u8 = 123;
    type RouteRightKind = route_control_kinds::RouteControl<ROUTE_RIGHT_LABEL, 1>;

    fn retain_pico_smoke_fixture_symbols() {
        let _ = fanout_program::ROUTE_SCOPE_COUNT;
        let _ = fanout_program::EXPECTED_WORKER_BRANCH_LABELS;
        let _ = fanout_program::ACK_LABELS;
        let _ = huge_program::ROUTE_SCOPE_COUNT;
        let _ = huge_program::EXPECTED_WORKER_BRANCH_LABELS;
        let _ = huge_program::ACK_LABELS;
        let _ = linear_program::ROUTE_SCOPE_COUNT;
        let _ = linear_program::EXPECTED_WORKER_BRANCH_LABELS;
        let _ = linear_program::ACK_LABELS;
        let _ = huge_program::run
            as fn(&mut localside::ControllerEndpoint<'_>, &mut localside::WorkerEndpoint<'_>);
        let _ = huge_program::controller_program as fn() -> role_program::RoleProgram<0>;
        let _ = linear_program::run
            as fn(&mut localside::ControllerEndpoint<'_>, &mut localside::WorkerEndpoint<'_>);
        let _ = linear_program::controller_program as fn() -> role_program::RoleProgram<0>;
        let _ = fanout_program::run
            as fn(&mut localside::ControllerEndpoint<'_>, &mut localside::WorkerEndpoint<'_>);
        let _ = fanout_program::controller_program as fn() -> role_program::RoleProgram<0>;
        let _ =
            localside::worker_offer_decode_u8::<0> as fn(&mut localside::WorkerEndpoint<'_>) -> u8;
    }

    #[test]
    fn pico_smoke_fixture_symbols_are_reachable() {
        retain_pico_smoke_fixture_symbols();
    }

    #[test]
    fn logical_lane_dense_map_preserves_lane_255() {
        let mut lanes = [role_program::DENSE_LANE_NONE; role_program::LANE_DOMAIN_SIZE + 2];
        let count = CompiledRoleImage::build_logical_lane_dense_map_into(
            role_program::LANE_DOMAIN_SIZE,
            &mut lanes,
        );

        assert_eq!(count, role_program::LANE_DOMAIN_SIZE);
        assert_eq!(
            lanes[255],
            role_program::DenseLaneOrdinal::new(255).expect("lane 255 dense ordinal")
        );
        assert_ne!(lanes[255], role_program::DENSE_LANE_NONE);
        assert_eq!(lanes[256], role_program::DENSE_LANE_NONE);
        assert_eq!(lanes[257], role_program::DENSE_LANE_NONE);
    }

    type SendOnly<const LANE: u8, S, D, M> = StepCons<SendStep<S, D, M, LANE>, StepNil>;
    type BranchSteps<L, R> = RouteSteps<L, R>;

    fn with_compiled_role_image<const ROLE: u8, R>(
        program: &role_program::RoleProgram<ROLE>,
        f: impl FnOnce(&CompiledRoleImage) -> R,
    ) -> R {
        crate::global::compiled::materialize::with_compiled_role_image::<ROLE, _>(
            crate::global::lowering_input(program),
            f,
        )
    }

    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    struct TypestateNodeStats {
        node_count: usize,
        send_count: usize,
        recv_count: usize,
        local_count: usize,
        jump_count: usize,
        terminate_count: usize,
        route_arm_end_jumps: usize,
        loop_continue_jumps: usize,
        loop_break_jumps: usize,
        passive_observer_branch_jumps: usize,
    }

    fn typestate_node_stats(image: &CompiledRoleImage) -> TypestateNodeStats {
        let typestate = image.typestate_ref();
        let mut stats = TypestateNodeStats::default();
        let mut idx = 0usize;
        while idx < typestate.len() {
            let node = typestate.node(idx);
            stats.node_count += 1;
            match node.action() {
                LocalAction::Send { .. } => stats.send_count += 1,
                LocalAction::Recv { .. } => stats.recv_count += 1,
                LocalAction::Local { .. } => stats.local_count += 1,
                LocalAction::Terminate => stats.terminate_count += 1,
                LocalAction::Jump { reason } => {
                    stats.jump_count += 1;
                    match reason {
                        JumpReason::RouteArmEnd => stats.route_arm_end_jumps += 1,
                        JumpReason::LoopContinue => stats.loop_continue_jumps += 1,
                        JumpReason::LoopBreak => stats.loop_break_jumps += 1,
                        JumpReason::PassiveObserverBranch => {
                            stats.passive_observer_branch_jumps += 1;
                        }
                    }
                }
            }
            idx += 1;
        }
        stats
    }

    #[test]
    fn compiled_role_image_header_stays_compact() {
        assert!(
            core::mem::size_of::<super::CompiledRoleImage>() <= 34,
            "CompiledRoleImage header regressed back to pointer-rich layout: {} bytes",
            core::mem::size_of::<super::CompiledRoleImage>()
        );
    }

    #[test]
    fn compiled_role_image_persistent_bytes_match_exact_footprint() {
        let program: crate::g::Program<SendOnly<0, Role<0>, Role<1>, Msg<7, ()>>> =
            g::send::<Role<0>, Role<1>, Msg<7, ()>, 0>();
        let worker: role_program::RoleProgram<1> = role_program::project(&program);
        let lowering = crate::global::lowering_input(&worker);
        let expected = CompiledRoleImage::persistent_bytes_for_program(lowering.footprint());
        with_compiled_role_image(&worker, |image| {
            assert_eq!(image.actual_persistent_bytes(), expected);
        });
    }

    #[test]
    fn compiled_role_exposes_controller_arm_and_dispatch_tables() {
        type LeftSteps = SeqSteps<
            SendOnly<
                0,
                Role<0>,
                Role<0>,
                Msg<
                    { crate::runtime::consts::LABEL_ROUTE_DECISION },
                    GenericCapToken<RouteDecisionKind>,
                    RouteDecisionKind,
                >,
            >,
            SendOnly<0, Role<0>, Role<1>, Msg<41, ()>>,
        >;
        type RightSteps = SeqSteps<
            SendOnly<
                0,
                Role<0>,
                Role<0>,
                Msg<ROUTE_RIGHT_LABEL, GenericCapToken<RouteRightKind>, RouteRightKind>,
            >,
            SendOnly<0, Role<0>, Role<1>, Msg<47, ()>>,
        >;
        type ProgramSteps = BranchSteps<LeftSteps, RightSteps>;

        let left: g::Program<LeftSteps> = g::seq(
            g::send::<
                Role<0>,
                Role<0>,
                Msg<
                    { crate::runtime::consts::LABEL_ROUTE_DECISION },
                    GenericCapToken<RouteDecisionKind>,
                    RouteDecisionKind,
                >,
                0,
            >(),
            g::send::<Role<0>, Role<1>, Msg<41, ()>, 0>(),
        );
        let right: g::Program<RightSteps> = g::seq(
            g::send::<
                Role<0>,
                Role<0>,
                Msg<ROUTE_RIGHT_LABEL, GenericCapToken<RouteRightKind>, RouteRightKind>,
                0,
            >(),
            g::send::<Role<0>, Role<1>, Msg<47, ()>, 0>(),
        );
        let program: g::Program<ProgramSteps> = g::route(left, right);

        let controller: role_program::RoleProgram<0> = role_program::project(&program);
        with_compiled_role_image(&controller, |controller_compiled| {
            let controller_scope = controller_compiled.typestate_ref().node(0).scope();
            assert_eq!(controller_compiled.role(), 0);
            assert!(controller_compiled.is_active_lane(0));
            assert_eq!(
                controller_compiled
                    .controller_arm_entry_by_arm(controller_scope, 0)
                    .map(|(_, label)| label),
                Some(crate::runtime::consts::LABEL_ROUTE_DECISION)
            );
            assert_eq!(
                controller_compiled
                    .controller_arm_entry_by_arm(controller_scope, 1)
                    .map(|(_, label)| label),
                Some(ROUTE_RIGHT_LABEL)
            );
            assert!(
                controller_compiled
                    .typestate_ref()
                    .controller_arm_entry_by_arm(controller_scope, 0)
                    .is_some(),
                "compiled role typestate must remain the single source of controller-arm facts"
            );
        });

        let worker: role_program::RoleProgram<1> = role_program::project(&program);
        with_compiled_role_image(&worker, |worker_compiled| {
            let worker_scope = worker_compiled.typestate_ref().node(0).scope();
            assert_eq!(worker_compiled.role(), 1);
            assert!(worker_compiled.is_active_lane(0));
            assert!(worker_compiled.phase_count() > 0);
            assert!(worker_compiled.local_len() > 0);
            assert_eq!(
                worker_compiled
                    .first_recv_dispatch_entry(worker_scope, 0)
                    .map(|(label, arm, _)| (label, arm)),
                Some((41, 0))
            );
            assert_eq!(
                worker_compiled
                    .first_recv_dispatch_entry(worker_scope, 1)
                    .map(|(label, arm, _)| (label, arm)),
                Some((47, 1))
            );
            assert!(
                worker_compiled
                    .typestate_ref()
                    .first_recv_dispatch_entry(worker_scope, 0)
                    .is_some(),
                "compiled role typestate must remain the single source of first-recv dispatch facts"
            );
            assert!(
                worker_compiled
                    .eff_index_to_step()
                    .iter()
                    .any(|&step_idx| step_idx != u16::MAX),
                "compiled role image must retain at least one eff-index mapping",
            );
            assert!(
                worker_compiled
                    .step_index_to_state()
                    .iter()
                    .any(|state| !state.is_max()),
                "compiled role image must retain at least one step-state mapping",
            );
        });
    }

    #[test]
    fn large_route_prefix_keeps_offer_and_frontier_bounds_local() {
        type Prefix01 = StepCons<SendStep<Role<0>, Role<1>, Msg<1, u8>, 0>, StepNil>;
        type Prefix02 = StepCons<SendStep<Role<1>, Role<0>, Msg<2, u8>, 0>, StepNil>;
        type Prefix03 = StepCons<SendStep<Role<0>, Role<1>, Msg<3, u8>, 0>, StepNil>;
        type Prefix04 = StepCons<SendStep<Role<1>, Role<0>, Msg<4, u8>, 0>, StepNil>;
        type Prefix05 = StepCons<SendStep<Role<0>, Role<1>, Msg<5, u8>, 0>, StepNil>;
        type Prefix06 = StepCons<SendStep<Role<1>, Role<0>, Msg<6, u8>, 0>, StepNil>;
        type Prefix07 = StepCons<SendStep<Role<0>, Role<1>, Msg<7, u8>, 0>, StepNil>;
        type Prefix08 = StepCons<SendStep<Role<1>, Role<0>, Msg<8, u8>, 0>, StepNil>;
        type Prefix09 = StepCons<SendStep<Role<0>, Role<1>, Msg<9, u8>, 0>, StepNil>;
        type Prefix10 = StepCons<SendStep<Role<1>, Role<0>, Msg<10, u8>, 0>, StepNil>;
        type Prefix11 = StepCons<SendStep<Role<0>, Role<1>, Msg<11, u8>, 0>, StepNil>;
        type Prefix12 = StepCons<SendStep<Role<1>, Role<0>, Msg<12, u8>, 0>, StepNil>;
        type Prefix13 = StepCons<SendStep<Role<0>, Role<1>, Msg<13, u8>, 0>, StepNil>;
        type Prefix14 = StepCons<SendStep<Role<1>, Role<0>, Msg<14, u8>, 0>, StepNil>;
        type Prefix15 = StepCons<SendStep<Role<0>, Role<1>, Msg<15, u8>, 0>, StepNil>;
        type Prefix16 = StepCons<SendStep<Role<1>, Role<0>, Msg<16, u8>, 0>, StepNil>;
        type PrefixSteps = SeqSteps<
            Prefix01,
            SeqSteps<
                Prefix02,
                SeqSteps<
                    Prefix03,
                    SeqSteps<
                        Prefix04,
                        SeqSteps<
                            Prefix05,
                            SeqSteps<
                                Prefix06,
                                SeqSteps<
                                    Prefix07,
                                    SeqSteps<
                                        Prefix08,
                                        SeqSteps<
                                            Prefix09,
                                            SeqSteps<
                                                Prefix10,
                                                SeqSteps<
                                                    Prefix11,
                                                    SeqSteps<
                                                        Prefix12,
                                                        SeqSteps<
                                                            Prefix13,
                                                            SeqSteps<
                                                                Prefix14,
                                                                SeqSteps<Prefix15, Prefix16>,
                                                            >,
                                                        >,
                                                    >,
                                                >,
                                            >,
                                        >,
                                    >,
                                >,
                            >,
                        >,
                    >,
                >,
            >,
        >;
        type LeftSteps = SeqSteps<
            StepCons<
                SendStep<
                    Role<0>,
                    Role<0>,
                    Msg<
                        { crate::runtime::consts::LABEL_ROUTE_DECISION },
                        GenericCapToken<RouteDecisionKind>,
                        RouteDecisionKind,
                    >,
                    0,
                >,
                StepNil,
            >,
            StepCons<SendStep<Role<0>, Role<1>, Msg<41, ()>, 0>, StepNil>,
        >;
        type RightSteps = SeqSteps<
            StepCons<
                SendStep<
                    Role<0>,
                    Role<0>,
                    Msg<ROUTE_RIGHT_LABEL, GenericCapToken<RouteRightKind>, RouteRightKind>,
                    0,
                >,
                StepNil,
            >,
            StepCons<SendStep<Role<0>, Role<1>, Msg<47, ()>, 0>, StepNil>,
        >;
        type ProgramSteps = SeqSteps<PrefixSteps, RouteSteps<LeftSteps, RightSteps>>;

        let prefix: crate::g::Program<PrefixSteps> = g::seq(
            g::send::<Role<0>, Role<1>, Msg<1, u8>, 0>(),
            g::seq(
                g::send::<Role<1>, Role<0>, Msg<2, u8>, 0>(),
                g::seq(
                    g::send::<Role<0>, Role<1>, Msg<3, u8>, 0>(),
                    g::seq(
                        g::send::<Role<1>, Role<0>, Msg<4, u8>, 0>(),
                        g::seq(
                            g::send::<Role<0>, Role<1>, Msg<5, u8>, 0>(),
                            g::seq(
                                g::send::<Role<1>, Role<0>, Msg<6, u8>, 0>(),
                                g::seq(
                                    g::send::<Role<0>, Role<1>, Msg<7, u8>, 0>(),
                                    g::seq(
                                        g::send::<Role<1>, Role<0>, Msg<8, u8>, 0>(),
                                        g::seq(
                                            g::send::<Role<0>, Role<1>, Msg<9, u8>, 0>(),
                                            g::seq(
                                                g::send::<Role<1>, Role<0>, Msg<10, u8>, 0>(),
                                                g::seq(
                                                    g::send::<Role<0>, Role<1>, Msg<11, u8>, 0>(),
                                                    g::seq(
                                                        g::send::<Role<1>, Role<0>, Msg<12, u8>, 0>(
                                                        ),
                                                        g::seq(
                                                            g::send::<
                                                                Role<0>,
                                                                Role<1>,
                                                                Msg<13, u8>,
                                                                0,
                                                            >(
                                                            ),
                                                            g::seq(
                                                                g::send::<
                                                                    Role<1>,
                                                                    Role<0>,
                                                                    Msg<14, u8>,
                                                                    0,
                                                                >(
                                                                ),
                                                                g::seq(
                                                                    g::send::<
                                                                        Role<0>,
                                                                        Role<1>,
                                                                        Msg<15, u8>,
                                                                        0,
                                                                    >(
                                                                    ),
                                                                    g::send::<
                                                                        Role<1>,
                                                                        Role<0>,
                                                                        Msg<16, u8>,
                                                                        0,
                                                                    >(
                                                                    ),
                                                                ),
                                                            ),
                                                        ),
                                                    ),
                                                ),
                                            ),
                                        ),
                                    ),
                                ),
                            ),
                        ),
                    ),
                ),
            ),
        );
        let left: crate::g::Program<LeftSteps> = g::seq(
            g::send::<
                Role<0>,
                Role<0>,
                Msg<
                    { crate::runtime::consts::LABEL_ROUTE_DECISION },
                    GenericCapToken<RouteDecisionKind>,
                    RouteDecisionKind,
                >,
                0,
            >(),
            g::send::<Role<0>, Role<1>, Msg<41, ()>, 0>(),
        );
        let right: crate::g::Program<RightSteps> = g::seq(
            g::send::<
                Role<0>,
                Role<0>,
                Msg<ROUTE_RIGHT_LABEL, GenericCapToken<RouteRightKind>, RouteRightKind>,
                0,
            >(),
            g::send::<Role<0>, Role<1>, Msg<47, ()>, 0>(),
        );
        let program: crate::g::Program<ProgramSteps> = g::seq(prefix, g::route(left, right));

        let worker: role_program::RoleProgram<1> = role_program::project(&program);
        let lowering = crate::global::lowering_input(&worker);
        assert!(
            CompiledRoleImage::persistent_bytes_for_program(lowering.footprint())
                < CompiledRoleImage::persistent_bytes_for_counts(
                    lowering.footprint().scope_count,
                    lowering.route_scope_count(),
                    lowering.eff_count(),
                ),
            "role image sizing should use the projected local step count instead of full eff_count"
        );
        with_compiled_role_image(&worker, |image| {
            assert!(
                image.local_len() >= 9,
                "large prefix should still project a substantial local program"
            );
            assert_eq!(
                image.route_scope_count(),
                1,
                "single trailing route should compile to one route scope"
            );
            assert_eq!(
                image.compiled_max_frontier_entries(),
                1,
                "frontier bound must stay tied to the active route frontier"
            );
            assert!(
                image.compiled_frontier_scratch_layout().total_bytes()
                    < image.local_len()
                        * core::mem::size_of::<crate::global::typestate::StateIndex>()
                        * 8,
                "frontier scratch must stay local to route metadata instead of scaling with the full local program"
            );
        });
    }

    fn assert_huge_shape_bounds(
        worker: &role_program::RoleProgram<1>,
        expected_route_scope_count: usize,
        expected_frontier_entries: usize,
    ) {
        with_compiled_role_image(&worker, |image| {
            let active_lane_count = image.active_lane_count();
            let layout = image.endpoint_arena_layout_for_binding(true);
            let no_binding_layout = image.endpoint_arena_layout_for_binding(false);

            assert!(
                image.local_len() >= expected_route_scope_count,
                "huge choreography local length must dominate the route scope count"
            );
            assert_eq!(
                image.route_scope_count(),
                expected_route_scope_count,
                "route scope count must stay tied to the huge choreography shape"
            );
            assert_eq!(
                image.compiled_max_frontier_entries(),
                expected_frontier_entries,
                "frontier bound must stay tied to branch-local fan-out"
            );
            assert_eq!(
                image.max_frontier_entries(),
                image.compiled_max_frontier_entries(),
                "test-visible frontier capacity must match the compiled exact count"
            );
            assert!(
                image.compiled_max_frontier_entries() < image.local_len().max(1),
                "frontier bound must not grow with the full local prefix"
            );
            assert_eq!(
                layout.scope_evidence_slots().count(),
                image.scope_evidence_count(),
                "scope evidence storage must stay exact-bound to the compiled evidence count"
            );
            assert_eq!(
                layout.binding_slots().count(),
                image.logical_lane_count() * 8,
                "binding storage must stay exact-bound to the logical lane count"
            );
            assert_eq!(
                no_binding_layout.binding_slots().count(),
                0,
                "NoBinding layout must not reserve buffered binding slots"
            );
            assert_eq!(
                no_binding_layout.phase_cursor_lane_cursors().count(),
                image.logical_lane_count(),
                "NoBinding layout must still reserve phase cursor lane storage"
            );
            assert_eq!(
                no_binding_layout.route_state_lane_dense_by_lane().count(),
                image.logical_lane_count(),
                "NoBinding layout must still reserve route lane maps"
            );
            assert_eq!(
                no_binding_layout.binding_len().count(),
                0,
                "NoBinding layout must not reserve binding len storage"
            );
            assert_eq!(
                no_binding_layout.binding_label_masks().count(),
                0,
                "NoBinding layout must not reserve binding label masks"
            );
            assert!(
                no_binding_layout.total_bytes() < layout.total_bytes(),
                "NoBinding layout must stay smaller than the binding-capable layout"
            );
            assert_eq!(
                layout.route_arm_stack().count(),
                active_lane_count * image.max_route_stack_depth(),
                "route-arm stack must stay exact-bound to active lanes and route depth"
            );
            assert_eq!(
                layout.frontier_offer_entry_slots().count(),
                image.compiled_max_frontier_entries(),
                "offer entry storage must stay tied to the compiled simultaneous frontier bound"
            );
        });
    }

    #[test]
    fn loop_table_budget_scales_with_endpoint_lane_span() {
        let handshake = g::send::<Role<0>, Role<1>, Msg<10, ()>, 0>();
        let inner_continue = g::seq(
            g::send::<
                Role<0>,
                Role<0>,
                Msg<
                    { LoopContinueKind::LABEL },
                    GenericCapToken<LoopContinueKind>,
                    LoopContinueKind,
                >,
                1,
            >()
            .policy::<10>(),
            g::send::<
                Role<0>,
                Role<0>,
                Msg<
                    { LoopContinueKind::LABEL },
                    GenericCapToken<LoopContinueKind>,
                    LoopContinueKind,
                >,
                1,
            >(),
        );
        let inner_break = g::seq(
            g::send::<
                Role<0>,
                Role<0>,
                Msg<{ LoopBreakKind::LABEL }, GenericCapToken<LoopBreakKind>, LoopBreakKind>,
                1,
            >()
            .policy::<10>(),
            g::send::<
                Role<0>,
                Role<0>,
                Msg<{ LoopBreakKind::LABEL }, GenericCapToken<LoopBreakKind>, LoopBreakKind>,
                1,
            >(),
        );
        let inner_route = g::route(inner_continue, inner_break);
        let outer_continue = g::seq(
            g::send::<
                Role<0>,
                Role<0>,
                Msg<
                    { LoopContinueKind::LABEL },
                    GenericCapToken<LoopContinueKind>,
                    LoopContinueKind,
                >,
                1,
            >()
            .policy::<11>(),
            g::seq(g::send::<Role<0>, Role<1>, Msg<11, ()>, 1>(), inner_route),
        );
        let outer_break = g::send::<
            Role<0>,
            Role<0>,
            Msg<{ LoopBreakKind::LABEL }, GenericCapToken<LoopBreakKind>, LoopBreakKind>,
            1,
        >()
        .policy::<11>();
        let decision = g::route(outer_continue, outer_break);
        let program = g::par(handshake, decision);
        let controller: role_program::RoleProgram<0> = role_program::project(&program);

        with_compiled_role_image(&controller, |image| {
            assert_eq!(image.endpoint_lane_slot_count(), 2);
            assert_eq!(image.max_loop_stack_depth(), 1);
            assert_eq!(image.loop_table_lane_slots(), 2);
            assert_eq!(image.loop_table_slots(), 2);
        });
    }

    fn count_parallel_enter_markers(summary: &LoweringSummary) -> usize {
        let markers = summary.view().scope_markers();
        let mut count = 0usize;
        let mut idx = 0usize;
        while idx < markers.len() {
            let marker = markers[idx];
            if matches!(marker.event, crate::global::const_dsl::ScopeEvent::Enter)
                && matches!(
                    marker.scope_kind,
                    crate::global::const_dsl::ScopeKind::Parallel
                )
            {
                count += 1;
            }
            idx += 1;
        }
        count
    }

    #[test]
    fn huge_shape_phase_counts_stay_bounded_by_parallel_markers() {
        let route_worker = huge_program::worker_program();
        let route_lowering = crate::global::lowering_input(&route_worker);
        let route_parallel_markers = route_lowering.with_summary(count_parallel_enter_markers);
        with_compiled_role_image(&route_worker, |image| {
            let phase_count = image.phase_len();
            let phase_lane_entry_len = image.phase_lane_entry_len();
            let local_len = image.local_len();
            let bound = if local_len == 0 {
                0
            } else {
                route_parallel_markers.saturating_mul(2).saturating_add(1)
            };
            assert!(
                phase_count <= bound,
                "route-heavy phase count must stay bounded by parallel enter markers"
            );
            std::println!(
                "phase-shape name=route_heavy local_len={} phase_count={} phase_lane_entry_len={} parallel_enter_markers={} phase_header_size={} phase_lane_entry_size={} lane_steps_size={} route_guard_size={}",
                local_len,
                phase_count,
                phase_lane_entry_len,
                route_parallel_markers,
                core::mem::size_of::<super::PhaseImageHeader>(),
                core::mem::size_of::<super::PhaseLaneEntry>(),
                core::mem::size_of::<crate::global::role_program::LaneSteps>(),
                core::mem::size_of::<crate::global::role_program::PhaseRouteGuard>(),
            );
        });

        let linear_worker = linear_program::worker_program();
        let linear_lowering = crate::global::lowering_input(&linear_worker);
        let linear_parallel_markers = linear_lowering.with_summary(count_parallel_enter_markers);
        with_compiled_role_image(&linear_worker, |image| {
            let phase_count = image.phase_len();
            let local_len = image.local_len();
            let bound = if local_len == 0 {
                0
            } else {
                linear_parallel_markers.saturating_mul(2).saturating_add(1)
            };
            assert!(
                phase_count <= bound,
                "linear-heavy phase count must stay bounded by parallel enter markers"
            );
            std::println!(
                "phase-shape name=linear_heavy local_len={} phase_count={} parallel_enter_markers={}",
                local_len,
                phase_count,
                linear_parallel_markers,
            );
        });

        let fanout_worker = fanout_program::worker_program();
        let fanout_lowering = crate::global::lowering_input(&fanout_worker);
        let fanout_parallel_markers = fanout_lowering.with_summary(count_parallel_enter_markers);
        with_compiled_role_image(&fanout_worker, |image| {
            let phase_count = image.phase_len();
            let local_len = image.local_len();
            let bound = if local_len == 0 {
                0
            } else {
                fanout_parallel_markers.saturating_mul(2).saturating_add(1)
            };
            assert!(
                phase_count <= bound,
                "fanout-heavy phase count must stay bounded by parallel enter markers"
            );
            std::println!(
                "phase-shape name=fanout_heavy local_len={} phase_count={} parallel_enter_markers={}",
                local_len,
                phase_count,
                fanout_parallel_markers,
            );
        });
    }

    fn long_linear_worker_program() -> role_program::RoleProgram<1> {
        let program = g::send::<Role<0>, Role<1>, Msg<1, u8>, 0>();
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<2, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<3, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<4, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<5, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<6, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<7, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<8, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<9, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<10, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<11, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<12, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<13, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<14, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<15, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<16, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<17, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<18, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<19, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<20, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<21, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<22, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<23, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<24, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<25, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<26, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<27, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<28, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<29, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<30, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<31, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<32, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<33, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<34, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<35, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<36, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<37, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<38, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<39, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<40, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<41, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<42, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<43, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<44, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<45, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<46, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<47, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<50, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<51, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<52, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<53, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<54, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<55, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<56, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<58, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<59, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<60, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<61, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<62, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<63, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<64, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<65, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<66, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<67, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<68, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<69, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<70, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<71, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<72, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<73, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<74, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<75, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<76, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<77, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<78, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<79, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<80, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<81, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<82, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<83, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<84, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<85, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<86, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<87, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<88, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<89, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<90, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<91, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<92, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<93, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<94, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<95, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<96, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<97, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<98, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<99, u8>, 0>());
        let program = g::seq(program, g::send::<Role<0>, Role<1>, Msg<100, u8>, 0>());
        role_program::project(&program)
    }

    #[test]
    fn long_linear_role_image_keeps_segmented_eff_indices_past_first_segment() {
        let worker = long_linear_worker_program();
        role_program::lowering_input(&worker).with_summary(|summary| {
            assert_eq!(
                summary.segment_summary(0).eff_len(),
                MAX_SEGMENT_EFFS,
                "first lowering segment must be summarized at segment-local capacity",
            );
            assert!(
                summary.segment_summary(1).eff_len() > 0,
                "long linear program must populate the next lowering segment",
            );
        });
        with_compiled_role_image(&worker, |image| {
            assert!(
                image.segment_count() > 1,
                "compiled role image must persist segment descriptor rows"
            );
            let first = image.segment_header(0).expect("first segment header");
            let second = image.segment_header(1).expect("second segment header");
            assert_eq!(first.eff_start, 0);
            assert_eq!(first.eff_len as usize, MAX_SEGMENT_EFFS);
            assert_eq!(second.eff_start as usize, MAX_SEGMENT_EFFS);
            assert!(second.eff_len > 0);
            let typestate = image.typestate_ref();
            let mut idx = 0usize;
            while idx < typestate.len() {
                let eff_index = match typestate.node(idx).action() {
                    LocalAction::Send { eff_index, .. }
                    | LocalAction::Recv { eff_index, .. }
                    | LocalAction::Local { eff_index, .. } => Some(eff_index),
                    LocalAction::Terminate | LocalAction::Jump { .. } => None,
                };

                if let Some(eff_index) = eff_index {
                    if eff_index.segment() > 0 {
                        assert!(
                            eff_index.as_usize() >= MAX_SEGMENT_EFFS,
                            "segmented index must still map back to its flat descriptor slot",
                        );
                        return;
                    }
                }
                idx += 1;
            }

            panic!("long linear role image did not retain a segment-1 effect index");
        });
    }

    #[test]
    fn role_image_segment_streaming_failure_rolls_back_header() {
        let worker = long_linear_worker_program();
        let input = crate::global::lowering_input(&worker);
        let real_footprint = input.footprint();
        let constrained_footprint = role_program::RoleFootprint {
            eff_count: MAX_SEGMENT_EFFS,
            ..real_footprint
        };
        let constrained_bytes =
            CompiledRoleImage::persistent_bytes_for_program(constrained_footprint);
        let bytes = CompiledRoleImage::persistent_bytes_for_program(real_footprint);
        let align = CompiledRoleImage::persistent_align();
        let mut storage = std::vec::Vec::with_capacity(bytes + align);
        storage.resize(bytes + align, 0xA5);
        let base = storage.as_mut_ptr() as usize;
        let aligned = crate::global::compiled::materialize::with_role_lowering_scratch(
            input,
            |summary, scratch| {
                let aligned = ((base + align - 1) & !(align - 1)) as *mut CompiledRoleImage;
                let result = unsafe {
                    crate::global::compiled::materialize::try_init_compiled_role_image_from_summary(
                        aligned,
                        1,
                        summary,
                        scratch,
                        constrained_footprint,
                    )
                };
                assert_eq!(
                    result,
                    Err(crate::global::compiled::lowering::CompiledRoleImageInitError::SegmentHeaderCapacity),
                    "constrained segment header storage must preserve the init failure reason"
                );
                aligned
            },
        );
        let image = unsafe { &*aligned };
        assert_eq!(image.role(), 1);
        assert_eq!(image.typestate_offset, 0);
        assert_eq!(image.segment_headers_offset, 0);
        assert_eq!(image.eff_index_to_step_offset, 0);
        assert_eq!(image.step_index_to_state_offset, 0);
        assert_eq!(image.control_by_eff_offset, 0);
        assert_eq!(image.actual_persistent_bytes(), 0);
        let rolled_back_rows =
            unsafe { std::slice::from_raw_parts(aligned.cast::<u8>(), constrained_bytes) };
        assert!(
            rolled_back_rows[core::mem::size_of::<CompiledRoleImage>()..]
                .iter()
                .all(|byte| *byte == 0),
            "descriptor row storage must be zeroed on streaming init failure",
        );
    }

    #[test]
    fn role_image_phase_row_streaming_failure_rolls_back_rows() {
        let worker = long_linear_worker_program();
        let input = crate::global::lowering_input(&worker);
        let real_footprint = input.footprint();
        let constrained_footprint = role_program::RoleFootprint {
            phase_count: real_footprint.phase_count.saturating_sub(1),
            ..real_footprint
        };
        let constrained_bytes =
            CompiledRoleImage::persistent_bytes_for_program(constrained_footprint);
        let bytes = CompiledRoleImage::persistent_bytes_for_program(real_footprint);
        let align = CompiledRoleImage::persistent_align();
        let mut storage = std::vec::Vec::with_capacity(bytes + align);
        storage.resize(bytes + align, 0xA5);
        let base = storage.as_mut_ptr() as usize;
        let aligned = crate::global::compiled::materialize::with_role_lowering_scratch(
            input,
            |summary, scratch| {
                let aligned = ((base + align - 1) & !(align - 1)) as *mut CompiledRoleImage;
                let result = unsafe {
                    crate::global::compiled::materialize::try_init_compiled_role_image_from_summary(
                        aligned,
                        1,
                        summary,
                        scratch,
                        constrained_footprint,
                    )
                };
                assert_eq!(
                    result,
                    Err(crate::global::compiled::lowering::CompiledRoleImageInitError::PhaseHeaderCapacity),
                    "constrained phase row storage must preserve the init failure reason"
                );
                aligned
            },
        );
        let image = unsafe { &*aligned };
        assert_eq!(image.role(), 1);
        assert_eq!(image.typestate_offset, 0);
        assert_eq!(image.segment_headers_offset, 0);
        assert_eq!(image.phase_headers_offset, 0);
        assert_eq!(image.actual_persistent_bytes(), 0);
        let rolled_back_rows =
            unsafe { std::slice::from_raw_parts(aligned.cast::<u8>(), constrained_bytes) };
        assert!(
            rolled_back_rows[core::mem::size_of::<CompiledRoleImage>()..]
                .iter()
                .all(|byte| *byte == 0),
            "descriptor row storage must be zeroed on phase row streaming failure",
        );
    }

    fn assert_role_image_row_capacity_failure_rolls_back<const ROLE: u8>(
        name: &str,
        program: &role_program::RoleProgram<ROLE>,
        constrained_footprint: role_program::RoleFootprint,
        expected: crate::global::compiled::lowering::CompiledRoleImageInitError,
    ) {
        let input = crate::global::lowering_input(program);
        let real_footprint = input.footprint();
        let constrained_bytes =
            CompiledRoleImage::persistent_bytes_for_program(constrained_footprint);
        let bytes = CompiledRoleImage::persistent_bytes_for_program(real_footprint);
        let align = CompiledRoleImage::persistent_align();
        let mut storage = std::vec::Vec::with_capacity(bytes + align);
        storage.resize(bytes + align, 0xA5);
        let base = storage.as_mut_ptr() as usize;
        let aligned = crate::global::compiled::materialize::with_role_lowering_scratch(
            input,
            |summary, scratch| {
                let aligned = ((base + align - 1) & !(align - 1)) as *mut CompiledRoleImage;
                let result = unsafe {
                    crate::global::compiled::materialize::try_init_compiled_role_image_from_summary(
                        aligned,
                        ROLE,
                        summary,
                        scratch,
                        constrained_footprint,
                    )
                };
                assert_eq!(
                    result,
                    Err(expected),
                    "{name} must preserve the row init failure reason"
                );
                aligned
            },
        );
        let image = unsafe { &*aligned };
        assert_eq!(image.role(), ROLE);
        assert_eq!(image.typestate_offset, 0);
        assert_eq!(image.segment_headers_offset, 0);
        assert_eq!(image.eff_index_to_step_offset, 0);
        assert_eq!(image.step_index_to_state_offset, 0);
        assert_eq!(image.control_by_eff_offset, 0);
        assert_eq!(image.actual_persistent_bytes(), 0);
        let rolled_back_rows =
            unsafe { std::slice::from_raw_parts(aligned.cast::<u8>(), constrained_bytes) };
        assert!(
            rolled_back_rows[core::mem::size_of::<CompiledRoleImage>()..]
                .iter()
                .all(|byte| *byte == 0),
            "{name} descriptor row storage must be zeroed on init failure",
        );
    }

    #[test]
    fn role_image_descriptor_row_capacity_failures_roll_back_rows() {
        let worker = long_linear_worker_program();
        let input = crate::global::lowering_input(&worker);
        let real = input.footprint();
        assert_role_image_row_capacity_failure_rolls_back(
            "typestate rows",
            &worker,
            role_program::RoleFootprint {
                local_step_count: real.local_step_count.saturating_sub(1),
                ..real
            },
            crate::global::compiled::lowering::CompiledRoleImageInitError::TypestateNodeCapacity,
        );
        let routed_worker = huge_program::worker_program();
        let routed_input = crate::global::lowering_input(&routed_worker);
        let routed = routed_input.footprint();
        assert_role_image_row_capacity_failure_rolls_back(
            "route records",
            &routed_worker,
            role_program::RoleFootprint {
                route_scope_count: routed.route_scope_count.saturating_sub(1),
                ..routed
            },
            crate::global::compiled::lowering::CompiledRoleImageInitError::RouteRowCapacity,
        );
        assert_role_image_row_capacity_failure_rolls_back(
            "phase lane entries",
            &worker,
            role_program::RoleFootprint {
                phase_lane_entry_count: real.phase_lane_entry_count.saturating_sub(1),
                ..real
            },
            crate::global::compiled::lowering::CompiledRoleImageInitError::PhaseLaneEntryCapacity,
        );
        assert_role_image_row_capacity_failure_rolls_back(
            "phase lane words",
            &worker,
            role_program::RoleFootprint {
                phase_lane_word_count: real.phase_lane_word_count.saturating_sub(1),
                ..real
            },
            crate::global::compiled::lowering::CompiledRoleImageInitError::PhaseLaneWordCapacity,
        );
    }

    fn assert_role_image_stream_fault_rolls_back<const ROLE: u8>(
        name: &str,
        program: &role_program::RoleProgram<ROLE>,
        fault: crate::global::compiled::lowering::RoleImageStreamFault,
        expected: crate::global::compiled::lowering::CompiledRoleImageInitError,
    ) {
        let input = crate::global::lowering_input(program);
        let footprint = input.footprint();
        let bytes = CompiledRoleImage::persistent_bytes_for_program(footprint);
        let align = CompiledRoleImage::persistent_align();
        let mut storage = std::vec::Vec::with_capacity(bytes + align);
        storage.resize(bytes + align, 0xA5);
        let base = storage.as_mut_ptr() as usize;
        let aligned = crate::global::compiled::materialize::with_role_lowering_scratch(
            input,
            |summary, scratch| {
                let aligned = ((base + align - 1) & !(align - 1)) as *mut CompiledRoleImage;
                let result = unsafe {
                    crate::global::compiled::lowering::try_init_compiled_role_image_from_summary_with_fault(
                        aligned,
                        ROLE,
                        summary,
                        scratch,
                        footprint,
                        fault,
                    )
                };
                assert_eq!(
                    result,
                    Err(expected),
                    "{name} must preserve writer fault reason"
                );
                aligned
            },
        );
        let image = unsafe { &*aligned };
        assert_eq!(image.role(), ROLE);
        assert_eq!(image.typestate_offset, 0);
        assert_eq!(image.segment_headers_offset, 0);
        assert_eq!(image.eff_index_to_step_offset, 0);
        assert_eq!(image.step_index_to_state_offset, 0);
        assert_eq!(image.control_by_eff_offset, 0);
        assert_eq!(image.actual_persistent_bytes(), 0);
        let rolled_back_rows = unsafe { std::slice::from_raw_parts(aligned.cast::<u8>(), bytes) };
        assert!(
            rolled_back_rows[core::mem::size_of::<CompiledRoleImage>()..]
                .iter()
                .all(|byte| *byte == 0),
            "{name} descriptor row storage must be zeroed on writer fault",
        );
    }

    #[test]
    fn descriptor_row_writer_faults_roll_back_rows() {
        let linear_worker = long_linear_worker_program();
        let route_worker = huge_program::worker_program();

        assert_role_image_stream_fault_rolls_back(
            "typestate node writer",
            &linear_worker,
            crate::global::compiled::lowering::RoleImageStreamFault::AfterTypestateNode(0),
            crate::global::compiled::lowering::CompiledRoleImageInitError::TypestateNodeCapacity,
        );
        assert_role_image_stream_fault_rolls_back(
            "scope row writer",
            &route_worker,
            crate::global::compiled::lowering::RoleImageStreamFault::AfterScopeRow(0),
            crate::global::compiled::lowering::CompiledRoleImageInitError::ScopeRowCapacity,
        );
        assert_role_image_stream_fault_rolls_back(
            "route record writer",
            &route_worker,
            crate::global::compiled::lowering::RoleImageStreamFault::AfterRouteRecord(0),
            crate::global::compiled::lowering::CompiledRoleImageInitError::RouteRowCapacity,
        );
        assert_role_image_stream_fault_rolls_back(
            "route slot writer",
            &route_worker,
            crate::global::compiled::lowering::RoleImageStreamFault::AfterRouteSlot(0),
            crate::global::compiled::lowering::CompiledRoleImageInitError::RouteRowCapacity,
        );
        assert_role_image_stream_fault_rolls_back(
            "lane mask writer",
            &route_worker,
            crate::global::compiled::lowering::RoleImageStreamFault::AfterLaneMask(0),
            crate::global::compiled::lowering::CompiledRoleImageInitError::LaneMatrixCapacity,
        );
        assert_role_image_stream_fault_rolls_back(
            "phase header writer",
            &linear_worker,
            crate::global::compiled::lowering::RoleImageStreamFault::AfterPhaseHeader(0),
            crate::global::compiled::lowering::CompiledRoleImageInitError::PhaseHeaderCapacity,
        );
        assert_role_image_stream_fault_rolls_back(
            "phase lane entry writer",
            &linear_worker,
            crate::global::compiled::lowering::RoleImageStreamFault::AfterPhaseLaneEntry(0),
            crate::global::compiled::lowering::CompiledRoleImageInitError::PhaseLaneEntryCapacity,
        );
        assert_role_image_stream_fault_rolls_back(
            "phase lane word writer",
            &linear_worker,
            crate::global::compiled::lowering::RoleImageStreamFault::AfterPhaseLaneWord(0),
            crate::global::compiled::lowering::CompiledRoleImageInitError::PhaseLaneWordCapacity,
        );
        assert_role_image_stream_fault_rolls_back(
            "eff-index writer",
            &linear_worker,
            crate::global::compiled::lowering::RoleImageStreamFault::AfterEffIndexRow(0),
            crate::global::compiled::lowering::CompiledRoleImageInitError::EffIndexCapacity,
        );
        assert_role_image_stream_fault_rolls_back(
            "step-index writer",
            &linear_worker,
            crate::global::compiled::lowering::RoleImageStreamFault::AfterStepIndexRow(0),
            crate::global::compiled::lowering::CompiledRoleImageInitError::StepIndexCapacity,
        );
        assert_role_image_stream_fault_rolls_back(
            "control-by-eff writer",
            &linear_worker,
            crate::global::compiled::lowering::RoleImageStreamFault::AfterControlByEffRow(0),
            crate::global::compiled::lowering::CompiledRoleImageInitError::EffIndexCapacity,
        );
    }

    #[test]
    fn role_runtime_table_view_names_match_actual_slice_semantics() {
        let worker = huge_program::worker_program();
        with_compiled_role_image(&worker, |image| {
            let tables = image.runtime_tables();
            assert_eq!(tables.segment_headers.len(), image.segment_count());
            assert_eq!(
                tables.route_record_by_dense_route.len(),
                image.route_scope_count()
            );
            assert_eq!(
                tables.route_dense_by_scope_slot.len(),
                image.typestate_ref().scope_count()
            );
            assert!(!tables.route_offer_lane_words_by_dense_route.is_empty());
            assert_eq!(tables.phase_headers.len(), image.phase_len());
            assert_eq!(tables.control_by_eff.len(), image.eff_index_to_step().len());
        });
    }

    #[test]
    fn role_runtime_table_view_route_dense_by_scope_slot_maps_to_expected_row() {
        let worker = huge_program::worker_program();
        with_compiled_role_image(&worker, |image| {
            let typestate = image.typestate_ref();
            let tables = image.runtime_tables();
            let mut checked = 0usize;

            let mut idx = 0usize;
            while idx < typestate.len() {
                let scope = typestate.node(idx).scope();
                if let Some(slot) = typestate.route_scope_slot_for_test(scope) {
                    let dense = typestate
                        .route_scope_dense_ordinal_for_test(slot)
                        .expect("route scope slot must map to a dense ordinal");
                    assert_eq!(
                        tables.route_dense_by_scope_slot[slot] as usize, dense,
                        "route_dense_by_scope_slot must map sparse scope slots to dense route rows",
                    );
                    assert_eq!(
                        tables.route_record_by_dense_route[dense],
                        typestate.route_records_table()[dense],
                        "dense route row must address the expected route scope record",
                    );
                    checked += 1;
                }
                idx += 1;
            }

            assert!(checked > 0, "fixture must contain route scopes");
        });
    }

    #[test]
    fn role_runtime_table_view_control_by_eff_contains_control_descriptors() {
        let program = g::send::<
            Role<0>,
            Role<0>,
            Msg<{ LoopContinueKind::LABEL }, GenericCapToken<LoopContinueKind>, LoopContinueKind>,
            0,
        >();
        let controller: role_program::RoleProgram<0> = role_program::project(&program);
        with_compiled_role_image(&controller, |image| {
            let tables = image.runtime_tables();
            let control = tables
                .control_by_eff
                .iter()
                .copied()
                .find(|desc| desc.label() == LoopContinueKind::LABEL)
                .expect("control_by_eff must contain the projected control descriptor row");

            assert_eq!(control.op(), LoopContinueKind::OP);
            assert_eq!(control.resource_tag(), LoopContinueKind::TAG);
        });
    }

    #[test]
    fn phase_headers_are_not_indexed_as_par_join_scope_table() {
        let worker = huge_program::worker_program();
        with_compiled_role_image(&worker, |image| {
            let tables = image.runtime_tables();
            assert_eq!(tables.phase_headers.len(), image.phase_len());
            let mut idx = 0usize;
            while idx < image.phase_len() {
                let expected = image.phase_header(idx).expect("phase header row");
                let actual = tables.phase_headers[idx];
                assert_eq!(actual.lane_entry_start, expected.lane_entry_start);
                assert_eq!(actual.lane_entry_len, expected.lane_entry_len);
                assert_eq!(actual.lane_word_start, expected.lane_word_start);
                assert_eq!(actual.lane_word_len, expected.lane_word_len);
                assert_eq!(actual.min_start, expected.min_start);
                assert!(actual.route_guard.matches(expected.route_guard));
                idx += 1;
            }
        });
    }

    #[test]
    fn segment_headers_match_eff_index_to_step_boundaries() {
        let worker = long_linear_worker_program();
        with_compiled_role_image(&worker, |image| {
            let tables = image.runtime_tables();
            let mut expected_start = 0usize;
            for (segment_idx, header) in tables.segment_headers.iter().copied().enumerate() {
                assert_eq!(
                    header.eff_start as usize, expected_start,
                    "segment header {segment_idx} must begin at the previous segment boundary",
                );
                if segment_idx + 1 < tables.segment_headers.len() {
                    assert_eq!(
                        header.eff_len as usize, MAX_SEGMENT_EFFS,
                        "non-final segment must be full",
                    );
                } else {
                    assert!(header.eff_len > 0, "final segment must not be empty");
                }
                expected_start += header.eff_len as usize;
            }
            assert_eq!(
                expected_start,
                image.eff_index_to_step().len(),
                "segment headers must exactly cover the eff-index table"
            );
        });
    }

    #[test]
    fn nested_parallel_exact_facts_match_built_phase_image() {
        type Lane0 = SendOnly<0, Role<0>, Role<1>, Msg<1, ()>>;
        type Lane1 = SendOnly<1, Role<1>, Role<0>, Msg<2, ()>>;
        type Lane2 = SendOnly<2, Role<0>, Role<1>, Msg<3, ()>>;
        type InnerSteps = crate::global::steps::ParSteps<Lane0, Lane1>;
        type ProgramSteps = crate::global::steps::ParSteps<InnerSteps, Lane2>;

        let lane0_program: crate::global::Program<Lane0> =
            g::send::<Role<0>, Role<1>, Msg<1, ()>, 0>();
        let lane1_program: crate::global::Program<Lane1> =
            g::send::<Role<1>, Role<0>, Msg<2, ()>, 1>();
        let lane2_program: crate::global::Program<Lane2> =
            g::send::<Role<0>, Role<1>, Msg<3, ()>, 2>();
        let inner_program: crate::global::Program<InnerSteps> =
            g::par(lane0_program, lane1_program);
        let program: crate::global::Program<ProgramSteps> = g::par(inner_program, lane2_program);

        let worker: role_program::RoleProgram<0> = role_program::project(&program);
        let lowering = crate::global::lowering_input(&worker);
        let counts = lowering.with_summary(|summary| summary.role_lowering_counts::<0>());

        with_compiled_role_image(&worker, |image| {
            assert_eq!(counts.phase_count, image.phase_len());
            assert_eq!(counts.phase_lane_entry_count, image.phase_lane_entry_len());
            assert_eq!(
                counts.phase_lane_word_count,
                image.role_facts.phase_lane_word_len()
            );
        });
    }

    fn print_role_tail_breakdown<const ROLE: u8>(
        name: &str,
        worker: &role_program::RoleProgram<ROLE>,
    ) {
        let lowering = crate::global::lowering_input(&worker);
        let scope_count = lowering.footprint().scope_count;
        let eff_count = lowering.eff_count();
        let route_enter_count = lowering.with_summary(|summary| {
            summary
                .view()
                .scope_markers()
                .iter()
                .filter(|marker| {
                    matches!(marker.event, crate::global::const_dsl::ScopeEvent::Enter)
                        && matches!(
                            marker.scope_kind,
                            crate::global::const_dsl::ScopeKind::Route
                        )
                })
                .count()
        });
        let local_len = lowering.local_step_count();
        let phase_cap = compiled_role_phase_cap(lowering.footprint());
        let typestate_node_cap = compiled_role_typestate_node_cap(
            scope_count,
            lowering.passive_linger_route_scope_count(),
            local_len,
        );
        let scope_cap = compiled_role_scope_cap(scope_count);
        let route_scope_cap = compiled_role_route_scope_cap(lowering.route_scope_count());
        let eff_cap = compiled_role_step_cap(eff_count);
        let step_cap = compiled_role_step_cap(local_len);
        let route_stats = with_compiled_role_image(&worker, |image| {
            image.typestate_ref().route_scope_payload_stats()
        });
        let scope_stats =
            with_compiled_role_image(&worker, |image| image.typestate_ref().scope_payload_stats());
        let node_stats = with_compiled_role_image(&worker, typestate_node_stats);
        let actual_total_bytes =
            with_compiled_role_image(&worker, |image| image.actual_persistent_bytes());
        std::println!(
            "role-tail-breakdown name={name} scope_count={} eff_count={} local_len={} phase_cap={} typestate_node_cap={} built_node_len={} typestate_node_slack={} local_node_size={} local_action_size={} policy_mode_size={} scope_record_size={} route_scope_record_size={} state_index_size={} typestate_nodes_bytes={} phases_bytes={} records_bytes={} slots_bytes={} route_dense_bytes={} route_records_bytes={} route_recv_bytes={} eff_index_bytes={} step_index_bytes={} total_bytes={} send_nodes={} recv_nodes={} local_nodes={} jump_nodes={} terminate_nodes={} route_arm_end_jumps={} loop_continue_jumps={} loop_break_jumps={} passive_observer_branch_jumps={} total_lane_first_entries={} max_lane_first_entries={} total_lane_last_entries={} max_lane_last_entries={} total_arm_entries={} max_arm_entries={} total_passive_arm_scopes={} max_passive_arm_scopes={} route_scope_count={} route_enter_count={} total_first_recv_entries={} max_first_recv_entries={} total_arm_lane_last_entries={} max_arm_lane_last_entries={} total_arm_lane_last_override_entries={} max_arm_lane_last_override_entries={} total_offer_lane_entries={} max_offer_lane_entries={}",
            scope_count,
            eff_count,
            local_len,
            phase_cap,
            typestate_node_cap,
            node_stats.node_count,
            typestate_node_cap.saturating_sub(node_stats.node_count),
            core::mem::size_of::<crate::global::typestate::LocalNode>(),
            crate::global::typestate::LocalNode::packed_action_size(),
            core::mem::size_of::<crate::global::const_dsl::PolicyMode>(),
            core::mem::size_of::<crate::global::typestate::ScopeRecord>(),
            core::mem::size_of::<crate::global::typestate::RouteScopeRecord>(),
            core::mem::size_of::<crate::global::typestate::StateIndex>(),
            typestate_node_cap * core::mem::size_of::<crate::global::typestate::LocalNode>(),
            phase_cap * core::mem::size_of::<super::PhaseImageHeader>()
                + step_cap * core::mem::size_of::<super::PhaseLaneEntry>(),
            scope_cap * core::mem::size_of::<crate::global::typestate::ScopeRecord>(),
            scope_cap * core::mem::size_of::<u16>(),
            scope_cap * core::mem::size_of::<u16>(),
            route_scope_cap * core::mem::size_of::<crate::global::typestate::RouteScopeRecord>(),
            0usize,
            eff_cap * core::mem::size_of::<u16>(),
            step_cap * core::mem::size_of::<crate::global::typestate::StateIndex>(),
            actual_total_bytes,
            node_stats.send_count,
            node_stats.recv_count,
            node_stats.local_count,
            node_stats.jump_count,
            node_stats.terminate_count,
            node_stats.route_arm_end_jumps,
            node_stats.loop_continue_jumps,
            node_stats.loop_break_jumps,
            node_stats.passive_observer_branch_jumps,
            scope_stats.total_lane_first_entries,
            scope_stats.max_lane_first_entries,
            scope_stats.total_lane_last_entries,
            scope_stats.max_lane_last_entries,
            scope_stats.total_arm_entries,
            scope_stats.max_arm_entries,
            scope_stats.total_passive_arm_scopes,
            scope_stats.max_passive_arm_scopes,
            route_stats.route_scope_count,
            route_enter_count,
            route_stats.total_first_recv_entries,
            route_stats.max_first_recv_entries,
            route_stats.total_arm_lane_last_entries,
            route_stats.max_arm_lane_last_entries,
            route_stats.total_arm_lane_last_override_entries,
            route_stats.max_arm_lane_last_override_entries,
            route_stats.total_offer_lane_entries,
            route_stats.max_offer_lane_entries,
        );
    }

    #[test]
    fn huge_shape_role_image_tail_breakdown_is_reported() {
        let route_worker = huge_program::worker_program();
        print_role_tail_breakdown::<1>("route_heavy", &route_worker);

        let linear_worker = linear_program::worker_program();
        print_role_tail_breakdown::<1>("linear_heavy", &linear_worker);

        let fanout_worker = fanout_program::worker_program();
        print_role_tail_breakdown::<1>("fanout_heavy", &fanout_worker);
    }

    #[test]
    fn offer_regression_role_tail_breakdown_is_reported() {
        type LoopContinueMsg = Msg<
            { crate::runtime::consts::LABEL_LOOP_CONTINUE },
            GenericCapToken<crate::control::cap::resource_kinds::LoopContinueKind>,
            crate::control::cap::resource_kinds::LoopContinueKind,
        >;
        type LoopBreakMsg = Msg<
            { crate::runtime::consts::LABEL_LOOP_BREAK },
            GenericCapToken<crate::control::cap::resource_kinds::LoopBreakKind>,
            crate::control::cap::resource_kinds::LoopBreakKind,
        >;
        type SessionRequestWireMsg = Msg<0x10, u8>;
        type AdminReplyMsg = Msg<0x50, u8>;
        type SnapshotCandidatesReplyMsg = Msg<0x51, u8>;
        type CheckpointMsg =
            Msg<{ SnapshotControl::LABEL }, GenericCapToken<SnapshotControl>, SnapshotControl>;
        type StaticRouteLeftMsg = Msg<
            { crate::runtime::consts::LABEL_ROUTE_DECISION },
            GenericCapToken<RouteDecisionKind>,
            RouteDecisionKind,
        >;
        type StaticRouteRightMsg =
            Msg<ROUTE_RIGHT_LABEL, GenericCapToken<RouteRightKind>, RouteRightKind>;
        type ReplyDecisionLeftSteps = SeqSteps<
            SendOnly<3, Role<1>, Role<1>, StaticRouteLeftMsg>,
            SendOnly<3, Role<1>, Role<0>, AdminReplyMsg>,
        >;
        type SnapshotReplyPathSteps = SeqSteps<
            SendOnly<3, Role<1>, Role<1>, StaticRouteLeftMsg>,
            SeqSteps<
                SendOnly<3, Role<1>, Role<1>, StaticRouteLeftMsg>,
                SeqSteps<
                    SendOnly<3, Role<1>, Role<0>, SnapshotCandidatesReplyMsg>,
                    SendOnly<3, Role<0>, Role<0>, CheckpointMsg>,
                >,
            >,
        >;
        type ReplyDecisionRightSteps =
            SeqSteps<SendOnly<3, Role<1>, Role<1>, StaticRouteRightMsg>, SnapshotReplyPathSteps>;
        type ReplyDecisionSteps = BranchSteps<ReplyDecisionLeftSteps, ReplyDecisionRightSteps>;
        type RequestExchangeSteps =
            SeqSteps<SendOnly<3, Role<0>, Role<1>, SessionRequestWireMsg>, ReplyDecisionSteps>;
        type ContinueArmSteps =
            SeqSteps<SendOnly<3, Role<0>, Role<0>, LoopContinueMsg>, RequestExchangeSteps>;
        type BreakArmSteps = SendOnly<3, Role<0>, Role<0>, LoopBreakMsg>;
        type LoopProgramSteps = BranchSteps<ContinueArmSteps, BreakArmSteps>;

        let reply_decision: g::Program<ReplyDecisionSteps> = g::route(
            g::seq(
                g::send::<Role<1>, Role<1>, StaticRouteLeftMsg, 3>(),
                g::send::<Role<1>, Role<0>, AdminReplyMsg, 3>(),
            ),
            g::seq(
                g::send::<Role<1>, Role<1>, StaticRouteRightMsg, 3>(),
                g::seq(
                    g::send::<Role<1>, Role<1>, StaticRouteLeftMsg, 3>(),
                    g::seq(
                        g::send::<Role<1>, Role<1>, StaticRouteLeftMsg, 3>(),
                        g::seq(
                            g::send::<Role<1>, Role<0>, SnapshotCandidatesReplyMsg, 3>(),
                            g::send::<Role<0>, Role<0>, CheckpointMsg, 3>(),
                        ),
                    ),
                ),
            ),
        );
        let request_exchange: g::Program<RequestExchangeSteps> = g::seq(
            g::send::<Role<0>, Role<1>, SessionRequestWireMsg, 3>(),
            reply_decision,
        );
        let loop_program: g::Program<LoopProgramSteps> = g::route(
            g::seq(
                g::send::<Role<0>, Role<0>, LoopContinueMsg, 3>(),
                request_exchange,
            ),
            g::send::<Role<0>, Role<0>, LoopBreakMsg, 3>(),
        );
        let program = loop_program;

        let client: role_program::RoleProgram<0> = role_program::project(&program);
        print_role_tail_breakdown::<0>("offer_admin_snapshot_client", &client);
        let server: role_program::RoleProgram<1> = role_program::project(&program);
        print_role_tail_breakdown::<1>("offer_admin_snapshot_server", &server);
    }

    #[test]
    fn huge_route_heavy_shape_keeps_resident_bounds_local() {
        let worker = huge_program::worker_program();
        assert_huge_shape_bounds(&worker, huge_program::ROUTE_SCOPE_COUNT, 1);
    }

    #[test]
    fn huge_linear_heavy_shape_keeps_resident_bounds_local() {
        let worker = linear_program::worker_program();
        assert_huge_shape_bounds(&worker, linear_program::ROUTE_SCOPE_COUNT, 0);
    }

    #[test]
    fn huge_fanout_heavy_shape_keeps_resident_bounds_local() {
        let worker = fanout_program::worker_program();
        assert_huge_shape_bounds(&worker, fanout_program::ROUTE_SCOPE_COUNT, 1);
    }
}
