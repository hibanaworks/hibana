use core::marker::PhantomData;

use crate::global::{role_program::RoleLoweringInput, typestate::RoleCompileScratch};

use super::program::CompiledProgramImage;
use super::role::CompiledRoleImage;
#[cfg(test)]
use super::program::CompiledProgram;
#[cfg(test)]
use super::role::CompiledRole;
use super::{LoweringSummary, ProgramStamp};
#[cfg(test)]
use core::{cell::UnsafeCell, mem::MaybeUninit, ptr};
#[cfg(test)]
use std::thread::LocalKey;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LoweringLeaseMode {
    SummaryOnly,
    SummaryAndRoleScratch,
}

pub(crate) struct LoweringLease<'a> {
    summary: &'a LoweringSummary,
    role_compile_scratch: Option<&'a mut RoleCompileScratch>,
}

impl<'a> LoweringLease<'a> {
    #[inline(always)]
    pub(crate) const fn summary(&self) -> &'a LoweringSummary {
        self.summary
    }

    #[inline(always)]
    pub(crate) fn into_parts(self) -> (&'a LoweringSummary, Option<&'a mut RoleCompileScratch>) {
        (self.summary, self.role_compile_scratch)
    }
}

struct TransientLoweringLeaseStorage<'a> {
    lowering: *mut LoweringSummary,
    role_compile_scratch: Option<*mut RoleCompileScratch>,
    _marker: PhantomData<&'a mut ()>,
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
    const fn role_compile_scratch_offset(base: usize) -> usize {
        let lowering_end = Self::lowering_offset(base) + core::mem::size_of::<LoweringSummary>();
        Self::align_up(lowering_end, core::mem::align_of::<RoleCompileScratch>())
    }

    #[inline(always)]
    const fn required_bytes_from_base(base: usize, mode: LoweringLeaseMode) -> usize {
        match mode {
            LoweringLeaseMode::SummaryOnly => {
                Self::lowering_offset(base) + core::mem::size_of::<LoweringSummary>() - base
            }
            LoweringLeaseMode::SummaryAndRoleScratch => {
                Self::role_compile_scratch_offset(base) + core::mem::size_of::<RoleCompileScratch>()
                    - base
            }
        }
    }

    #[inline]
    unsafe fn from_storage(storage: *mut u8, len: usize, mode: LoweringLeaseMode) -> Option<Self> {
        let base = storage as usize;
        let required = Self::required_bytes_from_base(base, mode);
        if required > len {
            return None;
        }
        Some(Self {
            lowering: Self::lowering_offset(base) as *mut LoweringSummary,
            role_compile_scratch: match mode {
                LoweringLeaseMode::SummaryOnly => None,
                LoweringLeaseMode::SummaryAndRoleScratch => {
                    Some(Self::role_compile_scratch_offset(base) as *mut RoleCompileScratch)
                }
            },
            _marker: PhantomData,
        })
    }

    #[inline]
    unsafe fn init(
        self,
        eff_list: &crate::global::const_dsl::EffList,
        stamp: ProgramStamp,
    ) -> LoweringLease<'a> {
        unsafe {
            LoweringSummary::init_scan(self.lowering, eff_list);
            let summary = &*self.lowering;
            debug_assert_eq!(summary.stamp(), stamp);
            let role_compile_scratch = match self.role_compile_scratch {
                Some(ptr) => {
                    RoleCompileScratch::init_empty(ptr);
                    Some(&mut *ptr)
                }
                None => None,
            };
            LoweringLease {
                summary,
                role_compile_scratch,
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
    let storage = unsafe { TransientLoweringLeaseStorage::from_storage(storage, len, mode) }?;
    let lease = unsafe { storage.init(input.eff_list(), input.stamp()) };
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

pub(crate) unsafe fn init_compiled_role_image_from_summary<const ROLE: u8, GlobalSteps>(
    dst: *mut CompiledRoleImage,
    summary: &LoweringSummary,
    scratch: *mut RoleCompileScratch,
    passive_linger_route_scope_count: usize,
    route_scope_count: usize,
    parallel_enter_count: usize,
)
where
    GlobalSteps: crate::g::advanced::steps::ProjectRole<crate::g::Role<ROLE>>,
    <GlobalSteps as crate::g::advanced::steps::ProjectRole<crate::g::Role<ROLE>>>::Output:
        crate::global::steps::StepCount,
{
    unsafe {
        CompiledRoleImage::init_from_summary_for_program::<ROLE, GlobalSteps>(
            dst,
            summary,
            &mut *scratch,
            passive_linger_route_scope_count,
            route_scope_count,
            parallel_enter_count,
        );
    }
}

#[cfg(test)]
pub(crate) fn with_compiled_program<R>(
    input: RoleLoweringInput<'_>,
    f: impl FnOnce(&CompiledProgram) -> R,
) -> R {
    let summary = LoweringSummary::scan_const(input.eff_list());
    let mut compiled = core::mem::MaybeUninit::<CompiledProgram>::uninit();
    unsafe {
        CompiledProgram::init_from_summary(compiled.as_mut_ptr(), &summary);
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
pub(crate) fn with_compiled_role_in_slot<const ROLE: u8, R>(
    compiled_slot: &'static LocalKey<UnsafeCell<MaybeUninit<CompiledRole>>>,
    scratch_slot: &'static LocalKey<UnsafeCell<MaybeUninit<RoleCompileScratch>>>,
    input: RoleLoweringInput<'_>,
    f: impl FnOnce(&CompiledRole) -> R,
) -> R {
    let summary = LoweringSummary::scan_const(input.eff_list());
    compiled_slot.with(|compiled| {
        scratch_slot.with(|scratch| unsafe {
            let compiled_ptr = (*compiled.get()).as_mut_ptr();
            let scratch_ptr = (*scratch.get()).as_mut_ptr();
            scratch_ptr.write(RoleCompileScratch::new());
            CompiledRole::init_from_summary::<ROLE>(compiled_ptr, &summary, &mut *scratch_ptr);
            let result = f(&*compiled_ptr);
            ptr::drop_in_place(compiled_ptr);
            ptr::drop_in_place(scratch_ptr);
            result
        })
    })
}

#[cfg(test)]
pub(crate) unsafe fn init_compiled_role_image<const ROLE: u8, GlobalSteps>(
    dst: *mut CompiledRoleImage,
    input: RoleLoweringInput<'_>,
    scratch: *mut RoleCompileScratch,
)
where
    GlobalSteps: crate::g::advanced::steps::ProjectRole<crate::g::Role<ROLE>>,
    <GlobalSteps as crate::g::advanced::steps::ProjectRole<crate::g::Role<ROLE>>>::Output:
        crate::global::steps::StepCount,
{
    let summary = LoweringSummary::scan_const(input.eff_list());
    unsafe {
        scratch.write(RoleCompileScratch::new());
        init_compiled_role_image_from_summary::<ROLE, GlobalSteps>(
            dst,
            &summary,
            scratch,
            input.passive_linger_route_scope_count(),
            input.route_scope_count(),
            input.parallel_enter_count(),
        );
    }
}

#[cfg(test)]
pub(crate) unsafe fn with_compiled_role_image<const ROLE: u8, GlobalSteps, R>(
    dst: *mut CompiledRoleImage,
    input: RoleLoweringInput<'_>,
    scratch: *mut RoleCompileScratch,
    f: impl FnOnce(&CompiledRoleImage) -> R,
) -> R
where
    GlobalSteps: crate::g::advanced::steps::ProjectRole<crate::g::Role<ROLE>>,
    <GlobalSteps as crate::g::advanced::steps::ProjectRole<crate::g::Role<ROLE>>>::Output:
        crate::global::steps::StepCount,
{
    unsafe {
        init_compiled_role_image::<ROLE, GlobalSteps>(dst, input, scratch);
        let result = f(&*dst);
        ptr::drop_in_place(dst);
        ptr::drop_in_place(scratch);
        result
    }
}
