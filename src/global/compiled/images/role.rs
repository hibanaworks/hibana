use crate::endpoint::kernel::{EndpointArenaLayout, FrontierScratchLayout};
#[cfg(test)]
use crate::global::compiled::layout::compiled_role_image_bytes_for_counts;
use crate::global::compiled::layout::{
    compiled_role_image_align, compiled_role_image_bytes_for_layout,
};
use crate::global::role_program::{
    DENSE_LANE_NONE, DenseLaneOrdinal, LaneSetView, LaneSteps, LaneWord, PhaseRouteGuard,
    RoleFootprint, lane_word_count, logical_lane_count_for_role,
};
use crate::global::typestate::{RoleTypestateValue, StateIndex};

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
    pub(in crate::global::compiled) typestate_offset: u16,
    pub(in crate::global::compiled) phase_headers_offset: u16,
    pub(in crate::global::compiled) phase_lane_entries_offset: u16,
    pub(in crate::global::compiled) phase_lane_words_offset: u16,
    pub(in crate::global::compiled) eff_index_to_step_offset: u16,
    pub(in crate::global::compiled) step_index_to_state_offset: u16,
    pub(in crate::global::compiled) role: u8,
    pub(in crate::global::compiled) role_facts: RoleResidentFacts,
}

#[derive(Clone, Copy, Debug)]
pub(in crate::global::compiled) struct PhaseImageHeader {
    pub(in crate::global::compiled) lane_entry_start: u16,
    pub(in crate::global::compiled) lane_entry_len: u16,
    pub(in crate::global::compiled) lane_word_start: u16,
    pub(in crate::global::compiled) lane_word_len: u16,
    pub(in crate::global::compiled) min_start: u16,
    pub(in crate::global::compiled) route_guard: PhaseRouteGuard,
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
        if self.typestate_ref().route_scope_count() == 0 {
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
    #[cfg(test)]
    pub(crate) fn route_scope_count(&self) -> usize {
        self.typestate_ref().route_scope_count()
    }

    #[inline(always)]
    pub(crate) fn scope_evidence_count(&self) -> usize {
        self.typestate_ref().route_scope_count()
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
        RoleFootprint::for_endpoint_layout(
            self.active_lane_count(),
            self.endpoint_lane_slot_count(),
            self.logical_lane_count(),
            self.max_route_stack_depth(),
            self.scope_evidence_count(),
            self.max_frontier_entries(),
        )
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
            cap::mint::{ControlResourceKind, GenericCapToken},
            cap::resource_kinds::{LoopBreakKind, LoopContinueKind, RouteDecisionKind},
        },
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
            core::mem::size_of::<super::CompiledRoleImage>() <= 32,
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
