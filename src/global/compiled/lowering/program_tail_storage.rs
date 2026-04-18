use core::ptr::NonNull;

use crate::control::cluster::effects::ResourceDescriptor;
use crate::global::compiled::images::program::{
    CompiledProgramCounts, CompiledProgramImage, DynamicPolicySite, RouteControlRecord,
};

pub(in crate::global::compiled) struct CompiledProgramTailStorage {
    pub(super) resources: *mut ResourceDescriptor,
    pub(super) resources_len: usize,
    pub(super) sites: *mut DynamicPolicySite,
    pub(super) sites_len: usize,
    pub(super) route_controls: *mut RouteControlRecord,
    pub(super) route_controls_len: usize,
}

impl CompiledProgramTailStorage {
    #[inline(always)]
    pub(in crate::global::compiled) const fn align_up(value: usize, align: usize) -> usize {
        let mask = align.saturating_sub(1);
        (value + mask) & !mask
    }

    #[inline(always)]
    const fn section_bytes<T>(count: usize) -> usize {
        count.saturating_mul(core::mem::size_of::<T>())
    }

    #[inline(always)]
    unsafe fn section_ptr<T>(base: *mut u8, offset: &mut usize, count: usize) -> *mut T {
        if count == 0 {
            return NonNull::<T>::dangling().as_ptr();
        }
        *offset = Self::align_up(*offset, core::mem::align_of::<T>());
        let ptr = unsafe { base.add(*offset) }.cast::<T>();
        *offset = offset.saturating_add(Self::section_bytes::<T>(count));
        ptr
    }

    #[inline(always)]
    pub(in crate::global::compiled) unsafe fn from_image_ptr(
        image: *mut CompiledProgramImage,
        counts: CompiledProgramCounts,
    ) -> Self {
        let base = image.cast::<u8>();
        let mut offset = core::mem::size_of::<CompiledProgramImage>();
        let resources = unsafe { Self::section_ptr(base, &mut offset, counts.resources) };
        let sites = unsafe { Self::section_ptr(base, &mut offset, counts.dynamic_policy_sites) };
        let route_controls = unsafe { Self::section_ptr(base, &mut offset, counts.route_controls) };
        Self {
            resources,
            resources_len: counts.resources,
            sites,
            sites_len: counts.dynamic_policy_sites,
            route_controls,
            route_controls_len: counts.route_controls,
        }
    }
}
