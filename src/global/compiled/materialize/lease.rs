use core::{marker::PhantomData, ptr::NonNull};

#[cfg(test)]
use crate::global::typestate::RoleCompileScratch;
use crate::global::{
    role_program::{LocalStep, PhaseRouteGuard, RoleFootprint, RoleLoweringInput},
    typestate::{RoleTypestateBuildScratch, StateIndex},
};

#[cfg(test)]
use super::program::CompiledProgram;
use super::program::CompiledProgramImage;
use super::role::CompiledRoleImage;
use super::{LoweringSummary, ProgramStamp};
#[cfg(test)]
use core::ptr;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LoweringLeaseMode {
    SummaryOnly,
    SummaryAndRoleScratch,
}

pub(crate) struct RoleLoweringScratch<'a> {
    typestate_build: *mut RoleTypestateBuildScratch,
    by_eff_index: *mut LocalStep,
    by_eff_index_len: usize,
    present: *mut bool,
    present_len: usize,
    steps: *mut LocalStep,
    steps_len: usize,
    eff_index_to_step: *mut u16,
    eff_index_to_step_len: usize,
    step_index_to_state: *mut StateIndex,
    step_index_to_state_len: usize,
    route_guards: *mut PhaseRouteGuard,
    route_guards_len: usize,
    parallel_ranges: *mut (usize, usize),
    parallel_ranges_len: usize,
    _marker: PhantomData<&'a mut ()>,
}

impl<'a> RoleLoweringScratch<'a> {
    #[cfg(test)]
    #[inline(always)]
    pub(crate) fn from_compile_scratch(scratch: &'a mut RoleCompileScratch) -> Self {
        Self {
            typestate_build: &mut scratch.typestate_build,
            by_eff_index: scratch.by_eff_index.as_mut_ptr(),
            by_eff_index_len: scratch.by_eff_index.len(),
            present: scratch.present.as_mut_ptr(),
            present_len: scratch.present.len(),
            steps: scratch.steps.as_mut_ptr(),
            steps_len: scratch.steps.len(),
            eff_index_to_step: scratch.eff_index_to_step.as_mut_ptr(),
            eff_index_to_step_len: scratch.eff_index_to_step.len(),
            step_index_to_state: scratch.step_index_to_state.as_mut_ptr(),
            step_index_to_state_len: scratch.step_index_to_state.len(),
            route_guards: scratch.route_guards.as_mut_ptr(),
            route_guards_len: scratch.route_guards.len(),
            parallel_ranges: scratch.parallel_ranges.as_mut_ptr(),
            parallel_ranges_len: scratch.parallel_ranges.len(),
            _marker: PhantomData,
        }
    }

    #[inline(always)]
    fn ptr_or_dangling<T>(ptr: usize, len: usize) -> *mut T {
        if len == 0 {
            NonNull::<T>::dangling().as_ptr()
        } else {
            ptr as *mut T
        }
    }

    #[inline(always)]
    unsafe fn init_empty(&mut self) {
        unsafe {
            RoleTypestateBuildScratch::init_empty(self.typestate_build);
        }
    }

    #[inline(always)]
    pub(crate) fn typestate_build_mut(&mut self) -> &mut RoleTypestateBuildScratch {
        unsafe { &mut *self.typestate_build }
    }

    pub(crate) fn local_step_build_slices_mut(
        &mut self,
    ) -> (&mut [LocalStep], &mut [bool], &mut [LocalStep], &mut [u16]) {
        unsafe {
            (
                core::slice::from_raw_parts_mut(self.by_eff_index, self.by_eff_index_len),
                core::slice::from_raw_parts_mut(self.present, self.present_len),
                core::slice::from_raw_parts_mut(self.steps, self.steps_len),
                core::slice::from_raw_parts_mut(self.eff_index_to_step, self.eff_index_to_step_len),
            )
        }
    }

    pub(crate) fn eff_index_to_step(&self) -> &[u16] {
        unsafe { core::slice::from_raw_parts(self.eff_index_to_step, self.eff_index_to_step_len) }
    }

    #[inline(always)]
    pub(crate) fn step_state_build_slices_mut(
        &mut self,
    ) -> (&[LocalStep], &[u16], &mut [StateIndex]) {
        unsafe {
            (
                core::slice::from_raw_parts(self.steps, self.steps_len),
                core::slice::from_raw_parts(self.eff_index_to_step, self.eff_index_to_step_len),
                core::slice::from_raw_parts_mut(
                    self.step_index_to_state,
                    self.step_index_to_state_len,
                ),
            )
        }
    }

    #[inline(always)]
    pub(crate) fn step_index_to_state(&self) -> &[StateIndex] {
        unsafe {
            core::slice::from_raw_parts(self.step_index_to_state, self.step_index_to_state_len)
        }
    }

    #[inline(always)]
    pub(crate) fn phase_build_slices_mut(
        &mut self,
    ) -> (
        &[LocalStep],
        &[StateIndex],
        &mut [PhaseRouteGuard],
        &mut [(usize, usize)],
    ) {
        unsafe {
            (
                core::slice::from_raw_parts(self.steps, self.steps_len),
                core::slice::from_raw_parts(self.step_index_to_state, self.step_index_to_state_len),
                core::slice::from_raw_parts_mut(self.route_guards, self.route_guards_len),
                core::slice::from_raw_parts_mut(self.parallel_ranges, self.parallel_ranges_len),
            )
        }
    }
}

#[derive(Clone, Copy)]
struct RoleLoweringScratchLayout {
    typestate_build: usize,
    by_eff_index: usize,
    present: usize,
    steps: usize,
    eff_index_to_step: usize,
    step_index_to_state: usize,
    route_guards: usize,
    parallel_ranges: usize,
    end: usize,
}

impl RoleLoweringScratchLayout {
    #[inline(always)]
    const fn align_up(value: usize, align: usize) -> usize {
        let mask = align.saturating_sub(1);
        (value + mask) & !mask
    }

    #[inline(always)]
    const fn eff_index_count(footprint: RoleFootprint) -> usize {
        footprint.eff_count
    }

    #[inline(always)]
    const fn step_count(footprint: RoleFootprint) -> usize {
        footprint.local_step_count
    }

    #[inline(always)]
    const fn parallel_range_count(footprint: RoleFootprint) -> usize {
        footprint.parallel_enter_count
    }

    #[inline(always)]
    const fn from_start(start: usize, footprint: RoleFootprint) -> Self {
        let eff_index_count = Self::eff_index_count(footprint);
        let step_count = Self::step_count(footprint);
        let parallel_range_count = Self::parallel_range_count(footprint);

        let typestate_build =
            Self::align_up(start, core::mem::align_of::<RoleTypestateBuildScratch>());
        let typestate_build_end =
            typestate_build + core::mem::size_of::<RoleTypestateBuildScratch>();
        let by_eff_index = Self::align_up(typestate_build_end, core::mem::align_of::<LocalStep>());
        let by_eff_index_end =
            by_eff_index + eff_index_count.saturating_mul(core::mem::size_of::<LocalStep>());
        let present = Self::align_up(by_eff_index_end, core::mem::align_of::<bool>());
        let present_end = present + eff_index_count.saturating_mul(core::mem::size_of::<bool>());
        let steps = Self::align_up(present_end, core::mem::align_of::<LocalStep>());
        let steps_end = steps + step_count.saturating_mul(core::mem::size_of::<LocalStep>());
        let eff_index_to_step = Self::align_up(steps_end, core::mem::align_of::<u16>());
        let eff_index_to_step_end =
            eff_index_to_step + eff_index_count.saturating_mul(core::mem::size_of::<u16>());
        let step_index_to_state =
            Self::align_up(eff_index_to_step_end, core::mem::align_of::<StateIndex>());
        let step_index_to_state_end =
            step_index_to_state + step_count.saturating_mul(core::mem::size_of::<StateIndex>());
        let route_guards = Self::align_up(
            step_index_to_state_end,
            core::mem::align_of::<PhaseRouteGuard>(),
        );
        let route_guards_end =
            route_guards + step_count.saturating_mul(core::mem::size_of::<PhaseRouteGuard>());
        let parallel_ranges =
            Self::align_up(route_guards_end, core::mem::align_of::<(usize, usize)>());
        let end = parallel_ranges
            + parallel_range_count.saturating_mul(core::mem::size_of::<(usize, usize)>());
        Self {
            typestate_build,
            by_eff_index,
            present,
            steps,
            eff_index_to_step,
            step_index_to_state,
            route_guards,
            parallel_ranges,
            end,
        }
    }

    #[inline(always)]
    unsafe fn scratch<'a>(self, footprint: RoleFootprint) -> RoleLoweringScratch<'a> {
        let eff_index_count = Self::eff_index_count(footprint);
        let step_count = Self::step_count(footprint);
        let parallel_range_count = Self::parallel_range_count(footprint);
        RoleLoweringScratch {
            typestate_build: self.typestate_build as *mut RoleTypestateBuildScratch,
            by_eff_index: RoleLoweringScratch::ptr_or_dangling(self.by_eff_index, eff_index_count),
            by_eff_index_len: eff_index_count,
            present: RoleLoweringScratch::ptr_or_dangling(self.present, eff_index_count),
            present_len: eff_index_count,
            steps: RoleLoweringScratch::ptr_or_dangling(self.steps, step_count),
            steps_len: step_count,
            eff_index_to_step: RoleLoweringScratch::ptr_or_dangling(
                self.eff_index_to_step,
                eff_index_count,
            ),
            eff_index_to_step_len: eff_index_count,
            step_index_to_state: RoleLoweringScratch::ptr_or_dangling(
                self.step_index_to_state,
                step_count,
            ),
            step_index_to_state_len: step_count,
            route_guards: RoleLoweringScratch::ptr_or_dangling(self.route_guards, step_count),
            route_guards_len: step_count,
            parallel_ranges: RoleLoweringScratch::ptr_or_dangling(
                self.parallel_ranges,
                parallel_range_count,
            ),
            parallel_ranges_len: parallel_range_count,
            _marker: PhantomData,
        }
    }
}

pub(crate) struct LoweringLease<'a> {
    summary: &'a LoweringSummary,
    role_lowering_scratch: Option<RoleLoweringScratch<'a>>,
}

impl<'a> LoweringLease<'a> {
    #[inline(always)]
    pub(crate) const fn summary(&self) -> &'a LoweringSummary {
        self.summary
    }

    #[inline(always)]
    pub(crate) fn into_parts(self) -> (&'a LoweringSummary, Option<RoleLoweringScratch<'a>>) {
        (self.summary, self.role_lowering_scratch)
    }
}

struct TransientLoweringLeaseStorage<'a> {
    lowering: *mut LoweringSummary,
    role_lowering_scratch: Option<RoleLoweringScratch<'a>>,
}

impl<'a> TransientLoweringLeaseStorage<'a> {
    #[inline(always)]
    const fn align_up(value: usize, align: usize) -> usize {
        let mask = align.saturating_sub(1);
        (value + mask) & !mask
    }

    #[inline(always)]
    const fn lowering_offset(base: usize) -> usize {
        Self::align_up(base, core::mem::align_of::<LoweringSummary>())
    }

    #[inline(always)]
    const fn required_bytes_from_base(
        base: usize,
        mode: LoweringLeaseMode,
        footprint: RoleFootprint,
    ) -> usize {
        let lowering_end = Self::lowering_offset(base) + core::mem::size_of::<LoweringSummary>();
        match mode {
            LoweringLeaseMode::SummaryOnly => lowering_end - base,
            LoweringLeaseMode::SummaryAndRoleScratch => {
                RoleLoweringScratchLayout::from_start(lowering_end, footprint).end - base
            }
        }
    }

    #[inline]
    unsafe fn from_storage(
        storage: *mut u8,
        len: usize,
        mode: LoweringLeaseMode,
        footprint: RoleFootprint,
    ) -> Option<Self> {
        let base = storage as usize;
        let required = Self::required_bytes_from_base(base, mode, footprint);
        if required > len {
            return None;
        }
        let lowering = Self::lowering_offset(base) as *mut LoweringSummary;
        let lowering_end = lowering as usize + core::mem::size_of::<LoweringSummary>();
        Some(Self {
            lowering,
            role_lowering_scratch: match mode {
                LoweringLeaseMode::SummaryOnly => None,
                LoweringLeaseMode::SummaryAndRoleScratch => Some(unsafe {
                    RoleLoweringScratchLayout::from_start(lowering_end, footprint)
                        .scratch(footprint)
                }),
            },
        })
    }

    #[inline]
    unsafe fn init(self, summary: &LoweringSummary, stamp: ProgramStamp) -> LoweringLease<'a> {
        unsafe {
            summary.write_clone_to(self.lowering);
            let summary = &*self.lowering;
            debug_assert_eq!(summary.stamp(), stamp);
            let mut role_lowering_scratch = self.role_lowering_scratch;
            if let Some(scratch) = role_lowering_scratch.as_mut() {
                scratch.init_empty();
            }
            LoweringLease {
                summary,
                role_lowering_scratch,
            }
        }
    }
}

#[inline]
pub(crate) unsafe fn with_lowering_lease<R>(
    input: RoleLoweringInput<'_>,
    storage: *mut u8,
    len: usize,
    mode: LoweringLeaseMode,
    f: impl FnOnce(LoweringLease<'_>) -> R,
) -> Option<R> {
    let storage = unsafe {
        TransientLoweringLeaseStorage::from_storage(storage, len, mode, input.footprint())
    }?;
    let lease = unsafe { storage.init(input.summary(), input.stamp()) };
    Some(f(lease))
}

pub(crate) unsafe fn init_compiled_program_image_from_summary(
    dst: *mut CompiledProgramImage,
    summary: &LoweringSummary,
) {
    unsafe {
        CompiledProgramImage::init_from_summary(dst, summary);
    }
}

pub(crate) unsafe fn init_compiled_role_image_from_summary<const ROLE: u8>(
    dst: *mut CompiledRoleImage,
    summary: &LoweringSummary,
    scratch: &mut RoleLoweringScratch<'_>,
    footprint: RoleFootprint,
) {
    unsafe {
        CompiledRoleImage::init_from_summary_for_program::<ROLE>(dst, summary, scratch, footprint);
    }
}

#[cfg(test)]
pub(crate) fn with_compiled_program<R>(
    input: RoleLoweringInput<'_>,
    f: impl FnOnce(&CompiledProgram) -> R,
) -> R {
    let summary = input.summary();
    let mut compiled = core::mem::MaybeUninit::<CompiledProgram>::uninit();
    unsafe {
        CompiledProgram::init_from_summary(compiled.as_mut_ptr(), summary);
        let result = f(compiled.assume_init_ref());
        compiled.assume_init_drop();
        result
    }
}

#[cfg(test)]
pub(crate) fn with_compiled_programs<R>(
    left: RoleLoweringInput<'_>,
    right: RoleLoweringInput<'_>,
    f: impl FnOnce(&CompiledProgram, &CompiledProgram) -> R,
) -> R {
    with_compiled_program(left, |left_compiled| {
        with_compiled_program(right, |right_compiled| f(left_compiled, right_compiled))
    })
}

#[cfg(test)]
pub(crate) unsafe fn init_compiled_role_image<const ROLE: u8>(
    dst: *mut CompiledRoleImage,
    input: RoleLoweringInput<'_>,
    scratch: *mut RoleCompileScratch,
) {
    unsafe {
        RoleCompileScratch::init_empty(scratch);
        CompiledRoleImage::init_from_summary::<ROLE>(dst, input.summary(), &mut *scratch);
    }
}

#[cfg(test)]
pub(crate) unsafe fn with_compiled_role_image<const ROLE: u8, R>(
    dst: *mut CompiledRoleImage,
    input: RoleLoweringInput<'_>,
    scratch: *mut RoleCompileScratch,
    f: impl FnOnce(&CompiledRoleImage) -> R,
) -> R {
    unsafe {
        init_compiled_role_image::<ROLE>(dst, input, scratch);
        let result = f(&*dst);
        ptr::drop_in_place(dst);
        ptr::drop_in_place(scratch);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lowering_lease_role_scratch_bytes_follow_footprint() {
        let empty = RoleFootprint {
            scope_count: 0,
            eff_count: 0,
            phase_count: 0,
            phase_lane_entry_count: 0,
            phase_lane_word_count: 0,
            parallel_enter_count: 0,
            route_scope_count: 0,
            local_step_count: 0,
            passive_linger_route_scope_count: 0,
            active_lane_count: 0,
            endpoint_lane_slot_count: 0,
            logical_lane_count: 0,
            logical_lane_word_count: 0,
            max_route_stack_depth: 0,
            scope_evidence_count: 0,
            frontier_entry_count: 0,
        };
        let compact = RoleFootprint {
            scope_count: 0,
            eff_count: 3,
            phase_count: 2,
            phase_lane_entry_count: 2,
            phase_lane_word_count: 2,
            parallel_enter_count: 1,
            route_scope_count: 0,
            local_step_count: 2,
            passive_linger_route_scope_count: 0,
            active_lane_count: 0,
            endpoint_lane_slot_count: 0,
            logical_lane_count: 0,
            logical_lane_word_count: 0,
            max_route_stack_depth: 0,
            scope_evidence_count: 0,
            frontier_entry_count: 0,
        };
        let base = 0usize;
        let summary_end = TransientLoweringLeaseStorage::lowering_offset(base)
            + core::mem::size_of::<LoweringSummary>();
        let legacy_required = TransientLoweringLeaseStorage::align_up(
            summary_end,
            core::mem::align_of::<RoleCompileScratch>(),
        ) + core::mem::size_of::<RoleCompileScratch>()
            - base;
        let empty_required = TransientLoweringLeaseStorage::required_bytes_from_base(
            base,
            LoweringLeaseMode::SummaryAndRoleScratch,
            empty,
        );
        let compact_required = TransientLoweringLeaseStorage::required_bytes_from_base(
            base,
            LoweringLeaseMode::SummaryAndRoleScratch,
            compact,
        );

        assert!(
            empty_required < legacy_required,
            "empty footprint must not reserve the legacy whole-role scratch owner"
        );
        assert!(
            compact_required < legacy_required,
            "exact lowering scratch should stay smaller than the legacy whole-role scratch owner"
        );
        assert!(
            compact_required > empty_required,
            "footprint-driven lowering scratch must grow with used role-local facts"
        );
    }
}
