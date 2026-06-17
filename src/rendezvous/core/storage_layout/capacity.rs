use super::{
    AssocTable, FREE_REGION_CAPACITY, FreeRegion, Rendezvous, RouteTable, Sidecar, Transport,
};
use crate::session::cluster::error::ResourceScope;
mod endpoint_lease;
#[cfg(all(test, hibana_repo_tests))]
mod tests;

// # Unsafe Owner Contract
//
// This file owns rendezvous sidecar capacity growth, persistent region release,
// and resident table rebinding after storage has been allocated by the parent
// storage layout owner. Raw endpoint-lease and sidecar pointers are always
// range-checked against the pinned rendezvous slab metadata before use, and
// migration copies initialized entries into freshly allocated owner storage
// before publishing the new table ingress.

impl<'rv, 'cfg, T: Transport> Rendezvous<'rv, 'cfg, T>
where
    'cfg: 'rv,
{
    #[inline]
    fn free_region_empty_slots(&self) -> usize {
        let mut empty = 0usize;
        let mut idx = 0usize;
        while idx < FREE_REGION_CAPACITY {
            if !self.free_regions[idx].is_recorded() {
                empty += 1;
            }
            idx += 1;
        }
        empty
    }

    #[inline]
    fn first_empty_free_region_slot(&self) -> Option<usize> {
        let mut idx = 0usize;
        while idx < FREE_REGION_CAPACITY {
            if !self.free_regions[idx].is_recorded() {
                return Some(idx);
            }
            idx += 1;
        }
        None
    }

    #[inline]
    fn clear_free_region(&mut self, idx: usize) {
        if idx < FREE_REGION_CAPACITY {
            self.free_regions[idx] = FreeRegion::EMPTY;
        }
    }

    #[inline]
    fn record_free_region_fragment(&mut self, offset: u32, len: u32) {
        if len == 0 {
            return;
        }
        let Some(idx) = self.first_empty_free_region_slot() else {
            crate::invariant();
        };
        self.free_regions[idx] = FreeRegion::recorded(offset, len);
    }

    fn release_persistent_region(&mut self, offset: u32, len: u32) -> Result<(), ()> {
        if len == 0 {
            return Ok(());
        }
        let mut start = offset;
        let mut end = crate::invariant_some(offset.checked_add(len));
        let mut idx = 0usize;
        while idx < FREE_REGION_CAPACITY {
            let region = self.free_regions[idx];
            if !region.is_recorded() {
                idx += 1;
                continue;
            }
            let region_start = region.offset;
            let region_end = crate::invariant_some(region.offset.checked_add(region.len));
            if region_end < start || region_start > end {
                idx += 1;
                continue;
            }
            start = core::cmp::min(start, region_start);
            end = core::cmp::max(end, region_end);
            self.clear_free_region(idx);
            idx = 0;
        }

        if end == self.image_frontier {
            self.set_image_frontier(start);
            loop {
                let mut trimmed = false;
                let mut free_idx = 0usize;
                while free_idx < FREE_REGION_CAPACITY {
                    let region = self.free_regions[free_idx];
                    if region.is_recorded()
                        && crate::invariant_some(region.offset.checked_add(region.len))
                            == self.image_frontier
                    {
                        self.set_image_frontier(region.offset);
                        self.clear_free_region(free_idx);
                        trimmed = true;
                        break;
                    }
                    free_idx += 1;
                }
                if !trimmed {
                    break;
                }
            }
            return Ok(());
        }

        if let Some(idx) = self.first_empty_free_region_slot() {
            self.free_regions[idx] =
                FreeRegion::recorded(start, crate::invariant_some(end.checked_sub(start)));
            return Ok(());
        }
        Err(())
    }

    unsafe fn allocate_from_free_regions(
        &mut self,
        bytes: usize,
        align: usize,
    ) -> Option<(*mut u8, u32)> {
        let (slab_ptr, _) = self.slab_ptr_and_len();
        let base = slab_ptr as usize;
        let mut idx = 0usize;
        while idx < FREE_REGION_CAPACITY {
            let region = self.free_regions[idx];
            if !region.is_recorded() {
                idx += 1;
                continue;
            }
            let region_start = region.offset as usize;
            let region_end = region.offset as usize + region.len as usize;
            let alloc_start = Self::align_up(base + region_start, align).checked_sub(base)?;
            let alloc_end = alloc_start.checked_add(bytes)?;
            if alloc_end > region_end {
                idx += 1;
                continue;
            }
            let prefix_len = crate::invariant_some(alloc_start.checked_sub(region_start));
            let suffix_len = crate::invariant_some(region_end.checked_sub(alloc_end));
            let fragments = usize::from(prefix_len != 0) + usize::from(suffix_len != 0);
            if self.free_region_empty_slots() + 1 < fragments {
                idx += 1;
                continue;
            }
            self.clear_free_region(idx);
            if prefix_len != 0 {
                let prefix_len = crate::invariant_ok(u32::try_from(prefix_len));
                self.record_free_region_fragment(region.offset, prefix_len);
            }
            if suffix_len != 0 {
                let alloc_end = crate::invariant_ok(u32::try_from(alloc_end));
                let suffix_len = crate::invariant_ok(u32::try_from(suffix_len));
                self.record_free_region_fragment(alloc_end, suffix_len);
            }
            let alloc_start_u32 = crate::invariant_ok(u32::try_from(alloc_start));
            return Some((
                /* SAFETY: `alloc_start..alloc_end` was carved from a recorded
                free region inside this rendezvous slab and split before the
                pointer is returned. */
                unsafe { slab_ptr.add(alloc_start) },
                alloc_start_u32,
            ));
        }
        None
    }

    #[inline]
    unsafe fn allocate_persistent_sidecar_bytes(
        &mut self,
        bytes: usize,
        align: usize,
    ) -> Option<(*mut u8, u32)> {
        if let Some(region) =
            /* SAFETY: the rendezvous free-region table owns the resident slab
            fragments; allocation checks requested size/alignment before
            returning a sidecar range. */
            unsafe { self.allocate_from_free_regions(bytes, align) }
        {
            return Some(region);
        }
        let (slab_ptr, _) = self.slab_ptr_and_len();
        let base = slab_ptr as usize;
        let start = Self::align_up(base + self.image_frontier as usize, align).checked_sub(base)?;
        let end = start.checked_add(bytes)?;
        if end > self.endpoint_storage_floor() {
            return None;
        }
        if end > u32::MAX as usize {
            return None;
        }
        let end_u32 = crate::invariant_ok(u32::try_from(end));
        let start_u32 = crate::invariant_ok(u32::try_from(start));
        self.set_image_frontier(end_u32);
        Some((
            /* SAFETY: `start..end` is below `endpoint_storage_floor` and the
            image frontier was advanced before returning this persistent
            sidecar pointer. */
            unsafe { slab_ptr.add(start) },
            start_u32,
        ))
    }

    #[inline]
    pub(crate) fn allocate_external_persistent_sidecar_bytes(
        &mut self,
        bytes: usize,
        align: usize,
    ) -> Option<Sidecar<u8>> {
        let source_frontier = self.image_frontier;
        let (ptr, offset) = /* SAFETY: rendezvous core owns the resident slab slot and has checked lane/session generation before raw access. */ unsafe { self.allocate_persistent_sidecar_bytes(bytes, align) }?;
        let reclaim_delta = if offset > source_frontier {
            (offset - source_frontier) as usize
        } else {
            0
        };
        Some(Sidecar::from_raw_parts(ptr, bytes, reclaim_delta))
    }

    #[inline]
    fn reclaim_offset_for_sidecar<S>(&self, sidecar: Sidecar<S>) -> u32 {
        let (slab_ptr, _) = self.slab_ptr_and_len();
        let base = slab_ptr.addr();
        let payload_start = crate::invariant_some(sidecar.ptr().addr().checked_sub(base));
        let reclaim_start =
            crate::invariant_some(payload_start.checked_sub(sidecar.reclaim_delta()));
        crate::invariant_ok(u32::try_from(reclaim_start))
    }

    #[inline]
    fn free_bound_persistent_region<S>(
        &mut self,
        reclaim_offset: u32,
        sidecar: Sidecar<S>,
    ) -> Result<(), ()> {
        if sidecar.is_empty() {
            return Ok(());
        }
        let (slab_ptr, _) = self.slab_ptr_and_len();
        let base = slab_ptr.addr();
        let payload_start = crate::invariant_some(sidecar.ptr().addr().checked_sub(base));
        let reclaim_start = reclaim_offset as usize;
        let payload_end = crate::invariant_some(payload_start.checked_add(sidecar.bytes()));
        let release_len = crate::invariant_some(payload_end.checked_sub(reclaim_start));
        let release_len = crate::invariant_ok(u32::try_from(release_len));
        self.release_persistent_region(reclaim_offset, release_len)
    }

    #[inline]
    pub(crate) fn free_external_persistent_sidecar(
        &mut self,
        sidecar: Sidecar<u8>,
        failure_scope: ResourceScope,
    ) -> Result<(), ResourceScope> {
        if sidecar.is_empty() {
            return Ok(());
        }
        let reclaim_offset = self.reclaim_offset_for_sidecar(sidecar);
        self.free_bound_persistent_region(reclaim_offset, sidecar)
            .map_err(|()| failure_scope)
    }

    #[inline]
    fn lane_base(&self) -> u32 {
        self.lane_range.start
    }

    #[inline]
    fn lane_slot_count(&self) -> usize {
        if self.lane_range.end < self.lane_range.start {
            crate::invariant();
        }
        (self.lane_range.end - self.lane_range.start) as usize
    }

    #[inline]
    fn target_lane_slots(required_lane_slots: usize) -> Option<usize> {
        if required_lane_slots > usize::from(crate::runtime_core::consts::LANE_DOMAIN_SIZE) {
            return None;
        }
        Some(required_lane_slots.max(1))
    }

    #[inline]
    fn lease_sidecar<S>(&mut self, bytes: usize, align: usize) -> Option<Sidecar<S>> {
        Some(
            self.allocate_external_persistent_sidecar_bytes(bytes, align)?
                .cast::<S>(),
        )
    }

    #[inline]
    fn release_sidecar<S>(
        &mut self,
        sidecar: Sidecar<S>,
        failure_scope: ResourceScope,
    ) -> Result<(), ResourceScope> {
        self.free_external_persistent_sidecar(sidecar.cast::<u8>(), failure_scope)
    }

    #[inline]
    fn release_new_sidecar_or_invariant<S>(
        &mut self,
        sidecar: Sidecar<S>,
        failure_scope: ResourceScope,
    ) {
        if self
            .free_external_persistent_sidecar(sidecar.cast::<u8>(), failure_scope)
            .is_err()
        {
            crate::invariant();
        }
    }

    fn ensure_lane_storage_for_lane_slots(
        &mut self,
        required_lane_slots: usize,
    ) -> Result<(), ResourceScope> {
        let target_slots = self
            .lane_slot_count()
            .max(Self::target_lane_slots(required_lane_slots).ok_or(ResourceScope::LaneStorage)?);
        let lane_base = self.lane_base();
        let target_slots_u32 =
            u32::try_from(target_slots).map_err(|_| ResourceScope::LaneStorage)?;
        let lane_end = lane_base
            .checked_add(target_slots_u32)
            .ok_or(ResourceScope::LaneStorage)?;
        let core_growth = self.lane_slot_count() < target_slots;

        let assoc_was_bound = self.assoc.is_bound();
        let need_assoc = !assoc_was_bound || core_growth;

        if !need_assoc {
            return Ok(());
        }

        let source_assoc = self.assoc_storage;
        let lease = self
            .lease_sidecar::<u8>(
                AssocTable::storage_bytes(target_slots),
                AssocTable::storage_align(),
            )
            .ok_or(ResourceScope::LaneStorage)?;

        /* SAFETY: all required sidecar storage was leased before any table owner is rebound. */
        unsafe {
            if assoc_was_bound {
                self.assoc
                    .init_replacement_storage(lease.ptr(), lane_base, target_slots);
            } else {
                self.assoc
                    .bind_from_storage(lease.ptr(), lane_base, target_slots);
            }
        }
        if assoc_was_bound {
            if let Err(error) = self.release_sidecar(source_assoc, ResourceScope::LaneStorage) {
                /* SAFETY: the replacement assoc arena was initialized above but not published. */
                unsafe {
                    AssocTable::clear_waiters_in_storage(lease.ptr(), target_slots);
                }
                self.release_new_sidecar_or_invariant(lease, ResourceScope::LaneStorage);
                return Err(error);
            }
            self.assoc.clear_current_waiters();
            /* SAFETY: the replacement assoc arena was staged before source release succeeded. */
            unsafe {
                self.assoc
                    .commit_storage(lease.ptr(), lane_base, target_slots);
            }
        }
        self.assoc_storage = lease;
        self.lane_range = lane_base..lane_end;
        Ok(())
    }

    pub(crate) fn ensure_core_lane_tables_for_lane_slots(
        &mut self,
        required_lane_slots: usize,
    ) -> Result<(), ResourceScope> {
        self.ensure_lane_storage_for_lane_slots(required_lane_slots)
    }

    pub(crate) fn ensure_route_table_capacity(
        &mut self,
        required_frame_slots: usize,
        required_lane_slots: usize,
    ) -> Result<(), ResourceScope> {
        if required_frame_slots == 0
            || (self.routes.route_slots() >= required_frame_slots
                && self.routes.lane_slots() >= required_lane_slots)
        {
            return Ok(());
        }
        let lease = self
            .lease_sidecar::<u8>(
                RouteTable::storage_bytes(required_frame_slots, required_lane_slots),
                RouteTable::storage_align(),
            )
            .ok_or(ResourceScope::RouteTable)?;
        let source_route = self.route_storage;
        if self.routes.route_slots() == 0 {
            /* SAFETY: route storage is currently unbound. The fresh sidecar
            lease has route-table size/alignment, and binding initializes all
            frame/lane/waiter columns before publication. */
            unsafe {
                self.routes.bind_from_storage_with_layout(
                    lease.ptr(),
                    required_frame_slots,
                    self.lane_base(),
                    required_lane_slots,
                );
            }
            self.route_storage = lease;
            return Ok(());
        }

        /* SAFETY: the fresh route-table sidecar is unpublished replacement
        storage. Migration copies initialized frame/lane/waiter state before
        the old route sidecar is released. */
        unsafe {
            self.routes.migrate_from_storage(
                lease.ptr(),
                required_frame_slots,
                self.lane_base(),
                required_lane_slots,
            );
        }
        if let Err(error) = self.release_sidecar(source_route, ResourceScope::RouteTable) {
            /* SAFETY: the replacement route arena was initialized above but not published. */
            unsafe {
                RouteTable::clear_waiters_in_storage(
                    lease.ptr(),
                    required_frame_slots,
                    required_lane_slots,
                );
            }
            self.release_new_sidecar_or_invariant(lease, ResourceScope::RouteTable);
            return Err(error);
        }
        self.routes.clear_current_waiters();
        /* SAFETY: migration succeeded and the old sidecar has been released.
        Rebinding publishes the replacement route-table columns for the same
        rendezvous lane range. */
        unsafe {
            self.routes.rebind_from_storage(
                lease.ptr(),
                required_frame_slots,
                self.lane_base(),
                required_lane_slots,
            );
        }
        self.route_storage = lease;
        Ok(())
    }
}
