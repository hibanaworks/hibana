use super::{AssocTable, Rendezvous, RouteTable, Sidecar, Transport};
use crate::session::cluster::error::ResourceScope;
mod endpoint_lease;
#[cfg(all(test, hibana_repo_tests))]
mod tests;

// # Unsafe Owner Contract
//
// This file owns rendezvous sidecar capacity growth and resident table rebinding
// after storage has been allocated by the parent storage layout owner. Raw
// endpoint-lease and sidecar pointers are always range-checked against the
// pinned rendezvous slab metadata before use, and migration copies initialized
// entries into freshly allocated owner storage before publishing the new table
// ingress.

impl<'rv, 'cfg, T: Transport> Rendezvous<'rv, 'cfg, T>
where
    'cfg: 'rv,
{
    #[inline]
    unsafe fn allocate_persistent_sidecar_bytes(
        &mut self,
        bytes: usize,
        align: usize,
    ) -> Option<*mut u8> {
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
        self.set_image_frontier(end_u32);
        Some(
            /* SAFETY: `start..end` is below `endpoint_storage_floor` and the
            image frontier was advanced before returning this persistent
            sidecar pointer. */
            unsafe { slab_ptr.add(start) },
        )
    }

    #[inline]
    pub(crate) fn allocate_external_persistent_sidecar_bytes(
        &mut self,
        bytes: usize,
        align: usize,
    ) -> Option<Sidecar<u8>> {
        let ptr = /* SAFETY: rendezvous core owns the resident slab slot and has checked lane/session generation before raw access. */ unsafe { self.allocate_persistent_sidecar_bytes(bytes, align) }?;
        Some(Sidecar::from_raw_parts(ptr, bytes))
    }

    #[inline]
    pub(crate) fn release_external_persistent_sidecar(&mut self, sidecar: Sidecar<u8>) {
        if sidecar.is_empty() {
            return;
        }
        let (slab_ptr, _) = self.slab_ptr_and_len();
        let base = slab_ptr.addr();
        let payload_start = crate::invariant_some(sidecar.ptr().addr().checked_sub(base));
        let payload_end = crate::invariant_some(payload_start.checked_add(sidecar.bytes()));
        if payload_end > self.image_frontier as usize {
            crate::invariant();
        }
        if payload_end == self.image_frontier as usize {
            let payload_start = crate::invariant_ok(u32::try_from(payload_start));
            self.set_image_frontier(payload_start);
        }
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
    fn target_assoc_slots(required_assoc_slots: usize) -> Option<usize> {
        if required_assoc_slots > usize::from(u16::MAX) {
            return None;
        }
        Some(required_assoc_slots.max(1))
    }

    #[inline]
    fn lease_sidecar<S>(&mut self, bytes: usize, align: usize) -> Option<Sidecar<S>> {
        Some(
            self.allocate_external_persistent_sidecar_bytes(bytes, align)?
                .cast::<S>(),
        )
    }

    #[inline]
    fn release_sidecar<S>(&mut self, sidecar: Sidecar<S>) {
        self.release_external_persistent_sidecar(sidecar.cast::<u8>());
    }

    fn ensure_lane_storage(
        &mut self,
        required_lane_slots: usize,
        required_assoc_slots: usize,
    ) -> Result<(), ResourceScope> {
        let target_lane_slots = self
            .lane_slot_count()
            .max(Self::target_lane_slots(required_lane_slots).ok_or(ResourceScope::LaneStorage)?);
        let target_assoc_slots = self
            .assoc
            .assoc_slots()
            .max(Self::target_assoc_slots(required_assoc_slots).ok_or(ResourceScope::LaneStorage)?);
        let lane_base = self.lane_base();
        let target_slots_u32 =
            u32::try_from(target_lane_slots).map_err(|_| ResourceScope::LaneStorage)?;
        let lane_end = lane_base
            .checked_add(target_slots_u32)
            .ok_or(ResourceScope::LaneStorage)?;
        let core_growth = self.lane_slot_count() < target_lane_slots
            || self.assoc.assoc_slots() < target_assoc_slots;

        let assoc_was_bound = self.assoc.is_bound();
        let need_assoc = !assoc_was_bound || core_growth;

        if !need_assoc {
            return Ok(());
        }

        let source_assoc = self.assoc_storage;
        let lease = self
            .lease_sidecar::<u8>(
                AssocTable::storage_bytes(target_assoc_slots),
                AssocTable::storage_align(),
            )
            .ok_or(ResourceScope::LaneStorage)?;

        /* SAFETY: all required sidecar storage was leased before any table owner is rebound. */
        unsafe {
            if assoc_was_bound {
                self.assoc.init_replacement_storage(
                    lease.ptr(),
                    lane_base,
                    target_lane_slots,
                    target_assoc_slots,
                );
            } else {
                self.assoc.bind_from_storage(
                    lease.ptr(),
                    lane_base,
                    target_lane_slots,
                    target_assoc_slots,
                );
            }
        }
        if assoc_was_bound {
            self.release_sidecar(source_assoc);
            self.assoc.clear_current_overflow_waiters();
            /* SAFETY: the replacement assoc arena was staged before the owner pointer is published. */
            unsafe {
                self.assoc.commit_storage(
                    lease.ptr(),
                    lane_base,
                    target_lane_slots,
                    target_assoc_slots,
                );
            }
        }
        self.assoc_storage = lease;
        self.lane_range = lane_base..lane_end;
        Ok(())
    }

    pub(crate) fn ensure_core_lane_tables_for_assoc_entries(
        &mut self,
        required_lane_slots: usize,
        required_assoc_slots: usize,
    ) -> Result<(), ResourceScope> {
        self.ensure_lane_storage(required_lane_slots, required_assoc_slots)
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
        self.release_sidecar(source_route);
        self.routes.clear_current_waiters();
        /* SAFETY: migration populated the replacement route-table columns before
        rebinding publishes them for the same rendezvous lane range. */
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
