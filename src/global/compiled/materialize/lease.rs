use core::{marker::PhantomData, ptr::NonNull};

use crate::global::{
    role_program::{LocalStep, PhaseRouteGuard, RoleFootprint, RoleLoweringInput},
    typestate::{RoleTypestateBuildScratch, StateIndex},
};

use super::super::images::{CompiledProgramFacts, CompiledRoleImage};
#[cfg(test)]
use super::super::lowering::program_owner::CompiledProgram;
use super::super::lowering::{CompiledRoleImageInitError, LoweringSummary, ProgramStamp};
use super::super::lowering::{program_image_builder, role_image_builder};
#[cfg(test)]
use core::ptr;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LoweringLeaseMode {
    SummaryOnly,
    SummaryAndRoleScratch,
}

pub(crate) struct RoleLoweringScratch<'a> {
    typestate_build: RoleTypestateBuildScratch,
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
    #[inline(always)]
    fn ptr_or_dangling<T>(ptr: usize, len: usize) -> *mut T {
        if len == 0 {
            NonNull::<T>::dangling().as_ptr()
        } else {
            ptr as *mut T
        }
    }

    #[inline(always)]
    pub(crate) unsafe fn init_empty(&mut self) {
        unsafe {
            self.typestate_build.init_empty();
        }
    }

    #[inline(always)]
    pub(crate) fn typestate_build_mut(&mut self) -> &mut RoleTypestateBuildScratch {
        &mut self.typestate_build
    }

    pub(crate) fn local_step_build_slices_mut(&mut self) -> (&mut [LocalStep], &mut [u16]) {
        unsafe {
            (
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
    const fn typestate_node_count(footprint: RoleFootprint) -> usize {
        let capped = footprint
            .local_step_count
            .saturating_add(footprint.scope_count)
            .saturating_add(footprint.passive_linger_route_scope_count)
            .saturating_add(1);
        if capped == 0 { 1 } else { capped }
    }

    #[inline(always)]
    const fn from_start(start: usize, footprint: RoleFootprint) -> Self {
        let eff_index_count = Self::eff_index_count(footprint);
        let step_count = Self::step_count(footprint);
        let parallel_range_count = Self::parallel_range_count(footprint);
        let typestate_node_count = Self::typestate_node_count(footprint);

        let typestate_build = Self::align_up(start, RoleTypestateBuildScratch::storage_align());
        let typestate_build_end = RoleTypestateBuildScratch::storage_end_from_start(
            typestate_build,
            typestate_node_count,
            footprint.scope_count,
            footprint.max_active_scope_depth,
        );
        let steps = Self::align_up(typestate_build_end, core::mem::align_of::<LocalStep>());
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
        let typestate_node_count = Self::typestate_node_count(footprint);
        RoleLoweringScratch {
            typestate_build: unsafe {
                RoleTypestateBuildScratch::from_storage(
                    self.typestate_build,
                    typestate_node_count,
                    footprint.scope_count,
                    footprint.max_active_scope_depth,
                )
            },
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
    role_lowering_scratch: Option<RoleLoweringScratch<'a>>,
}

impl<'a> TransientLoweringLeaseStorage<'a> {
    #[cfg(test)]
    #[inline(always)]
    const fn align_up(value: usize, align: usize) -> usize {
        let mask = align.saturating_sub(1);
        (value + mask) & !mask
    }

    #[inline(always)]
    const fn required_bytes_from_base(
        base: usize,
        mode: LoweringLeaseMode,
        footprint: RoleFootprint,
    ) -> usize {
        match mode {
            LoweringLeaseMode::SummaryOnly => 0,
            LoweringLeaseMode::SummaryAndRoleScratch => {
                RoleLoweringScratchLayout::from_start(base, footprint).end - base
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
        Some(Self {
            role_lowering_scratch: match mode {
                LoweringLeaseMode::SummaryOnly => None,
                LoweringLeaseMode::SummaryAndRoleScratch => Some(unsafe {
                    RoleLoweringScratchLayout::from_start(base, footprint).scratch(footprint)
                }),
            },
        })
    }

    #[inline]
    unsafe fn init(
        self,
        source: crate::global::role_program::RoleImageSource,
        stamp: ProgramStamp,
    ) -> LoweringLease<'a> {
        unsafe {
            let summary = source.summary();
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

#[inline(always)]
#[cfg(test)]
const fn lowering_lease_storage_align() -> usize {
    let mut align = core::mem::align_of::<LoweringSummary>();
    if RoleTypestateBuildScratch::storage_align() > align {
        align = RoleTypestateBuildScratch::storage_align();
    }
    if core::mem::align_of::<LocalStep>() > align {
        align = core::mem::align_of::<LocalStep>();
    }
    if core::mem::align_of::<bool>() > align {
        align = core::mem::align_of::<bool>();
    }
    if core::mem::align_of::<u16>() > align {
        align = core::mem::align_of::<u16>();
    }
    if core::mem::align_of::<StateIndex>() > align {
        align = core::mem::align_of::<StateIndex>();
    }
    if core::mem::align_of::<PhaseRouteGuard>() > align {
        align = core::mem::align_of::<PhaseRouteGuard>();
    }
    if core::mem::align_of::<(usize, usize)>() > align {
        align = core::mem::align_of::<(usize, usize)>();
    }
    align
}

#[cfg(test)]
pub(crate) const fn lowering_lease_storage_bytes(
    footprint: RoleFootprint,
    mode: LoweringLeaseMode,
) -> usize {
    let max_align = lowering_lease_storage_align();
    let mut max_required = 0usize;
    let mut base = 0usize;
    while base < max_align {
        let required =
            TransientLoweringLeaseStorage::required_bytes_from_base(base, mode, footprint);
        if required > max_required {
            max_required = required;
        }
        base += 1;
    }
    max_required
}

#[cfg(test)]
pub(crate) const fn role_lowering_scratch_storage_bytes(footprint: RoleFootprint) -> usize {
    let max_align = lowering_lease_storage_align();
    let mut max_required = 0usize;
    let mut base = 0usize;
    while base < max_align {
        let required = RoleLoweringScratchLayout::from_start(base, footprint).end - base;
        if required > max_required {
            max_required = required;
        }
        base += 1;
    }
    max_required
}

#[inline]
pub(crate) unsafe fn with_lowering_lease<R>(
    input: RoleLoweringInput,
    storage: *mut u8,
    len: usize,
    mode: LoweringLeaseMode,
    f: impl FnOnce(LoweringLease<'_>) -> R,
) -> Option<R> {
    debug_assert_eq!(input.start(), crate::eff::EffIndex::ZERO);
    let storage = unsafe {
        TransientLoweringLeaseStorage::from_storage(storage, len, mode, input.footprint())
    }?;
    let lease = unsafe { storage.init(input.source(), input.stamp()) };
    Some(f(lease))
}

#[cfg(test)]
pub(crate) unsafe fn with_role_lowering_scratch_storage<R>(
    footprint: RoleFootprint,
    storage: *mut u8,
    len: usize,
    f: impl FnOnce(&mut RoleLoweringScratch<'_>) -> R,
) -> Option<R> {
    let base = storage as usize;
    let layout = RoleLoweringScratchLayout::from_start(base, footprint);
    let required = layout.end - base;
    if required > len {
        return None;
    }
    let mut scratch = unsafe { layout.scratch(footprint) };
    unsafe {
        scratch.init_empty();
    }
    Some(f(&mut scratch))
}

pub(crate) unsafe fn init_compiled_program_image_from_summary(
    dst: *mut CompiledProgramFacts,
    summary: &LoweringSummary,
) {
    unsafe {
        program_image_builder::init_compiled_program_image_from_summary(dst, summary);
    }
}

#[cfg(test)]
pub(crate) unsafe fn init_compiled_role_image_from_summary(
    dst: *mut CompiledRoleImage,
    role: u8,
    summary: &LoweringSummary,
    scratch: &mut RoleLoweringScratch<'_>,
    footprint: RoleFootprint,
) {
    unsafe {
        role_image_builder::init_compiled_role_image_from_summary(
            dst, role, summary, scratch, footprint,
        );
    }
}

pub(crate) unsafe fn try_init_compiled_role_image_from_summary(
    dst: *mut CompiledRoleImage,
    role: u8,
    summary: &LoweringSummary,
    scratch: &mut RoleLoweringScratch<'_>,
    footprint: RoleFootprint,
) -> Result<(), CompiledRoleImageInitError> {
    unsafe {
        role_image_builder::try_init_compiled_role_image_from_summary(
            dst, role, summary, scratch, footprint,
        )
    }
}

pub(crate) unsafe fn validate_compiled_role_image_init_from_summary(
    dst: *mut CompiledRoleImage,
    role: u8,
    summary: &LoweringSummary,
    footprint: RoleFootprint,
) -> Result<(), CompiledRoleImageInitError> {
    unsafe {
        role_image_builder::validate_compiled_role_image_init_from_summary(
            dst, role, summary, footprint,
        )
    }
}

pub(crate) unsafe fn init_compiled_role_image_from_prevalidated_summary(
    dst: *mut CompiledRoleImage,
    role: u8,
    summary: &LoweringSummary,
    scratch: &mut RoleLoweringScratch<'_>,
    footprint: RoleFootprint,
) -> usize {
    unsafe {
        role_image_builder::init_compiled_role_image_from_prevalidated_summary(
            dst, role, summary, scratch, footprint,
        )
    }
}

#[cfg(test)]
pub(crate) fn with_compiled_program<R>(
    input: RoleLoweringInput,
    f: impl FnOnce(&CompiledProgram) -> R,
) -> R {
    let mut compiled = core::mem::MaybeUninit::<CompiledProgram>::uninit();
    unsafe {
        let summary = input.source().summary();
        CompiledProgram::init_from_summary(compiled.as_mut_ptr(), summary);
        let result = f(compiled.assume_init_ref());
        compiled.assume_init_drop();
        result
    }
}

#[cfg(test)]
pub(crate) fn with_role_lowering_scratch<R>(
    input: RoleLoweringInput,
    f: impl FnOnce(&LoweringSummary, &mut RoleLoweringScratch<'_>) -> R,
) -> R {
    let footprint = input.footprint();
    let mut storage = std::vec::Vec::with_capacity(lowering_lease_storage_bytes(
        footprint,
        LoweringLeaseMode::SummaryAndRoleScratch,
    ));
    storage.resize(
        lowering_lease_storage_bytes(footprint, LoweringLeaseMode::SummaryAndRoleScratch),
        0u8,
    );
    unsafe {
        with_lowering_lease(
            input,
            storage.as_mut_ptr(),
            storage.len(),
            LoweringLeaseMode::SummaryAndRoleScratch,
            |lease| {
                let (summary, role_lowering_scratch) = lease.into_parts();
                let scratch = role_lowering_scratch
                    .expect("summary-and-scratch lowering lease must provide role scratch");
                f(summary, &mut { scratch })
            },
        )
        .expect("exact lowering scratch storage must fit derived footprint")
    }
}

#[cfg(test)]
pub(crate) fn with_compiled_role_image<const ROLE: u8, R>(
    input: RoleLoweringInput,
    f: impl FnOnce(&CompiledRoleImage) -> R,
) -> R {
    let footprint = input.footprint();
    let bytes = CompiledRoleImage::persistent_bytes_for_program(footprint);
    let align = CompiledRoleImage::persistent_align();
    let mut storage = std::vec::Vec::with_capacity(bytes + align);
    storage.resize(bytes + align, 0u8);
    let base = storage.as_mut_ptr() as usize;
    let aligned = TransientLoweringLeaseStorage::align_up(base, align) as *mut CompiledRoleImage;
    debug_assert!((aligned as usize) + bytes <= base + storage.len());
    with_role_lowering_scratch(input, |summary, scratch| unsafe {
        init_compiled_role_image_from_summary(aligned, ROLE, summary, scratch, footprint);
        let result = f(&*aligned);
        ptr::drop_in_place(aligned);
        result
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lowering_lease_role_scratch_bytes_follow_footprint() {
        let empty = RoleFootprint {
            scope_count: 0,
            max_active_scope_depth: 0,
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
            max_active_scope_depth: 0,
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
        let summary_only_required = TransientLoweringLeaseStorage::required_bytes_from_base(
            base,
            LoweringLeaseMode::SummaryOnly,
            compact,
        );
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

        assert_eq!(
            summary_only_required, 0,
            "lowering leases borrow the static summary instead of cloning it into the attach slab"
        );
        assert!(
            compact_required > empty_required,
            "footprint-driven lowering scratch must grow with used role-local facts"
        );
    }
}
