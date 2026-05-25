use super::*;

// # Unsafe Owner Contract
//
// This file owns rendezvous sidecar capacity growth, persistent region release,
// and resident table rebinding after storage has been allocated by the parent
// storage layout owner. Raw endpoint-lease and sidecar pointers are always
// range-checked against the pinned rendezvous slab metadata before use, and
// migration copies initialized entries into freshly allocated owner storage
// before publishing the new table binding.

impl<'rv, 'cfg, T: Transport, U: LabelUniverse, C: Clock, E: crate::control::cap::mint::EpochTable>
    Rendezvous<'rv, 'cfg, T, U, C, E>
where
    'cfg: 'rv,
{
    #[cfg(all(test, feature = "std"))]
    #[inline]
    pub(crate) fn live_endpoint_storage_bytes(&self) -> usize {
        let mut bytes = 0usize;
        let mut idx = 0usize;
        while idx < usize::from(self.endpoint_lease_capacity) {
            let slot = /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */ unsafe { &*self.endpoint_leases.add(idx) };
            if slot.occupied {
                bytes = bytes.saturating_add(slot.len as usize);
            }
            idx += 1;
        }
        bytes
    }

    #[inline]
    fn free_region_empty_slots(&self) -> usize {
        let mut empty = 0usize;
        let mut idx = 0usize;
        while idx < FREE_REGION_CAPACITY {
            if !self.free_regions[idx].occupied {
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
            if !self.free_regions[idx].occupied {
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

    fn release_persistent_region(&mut self, offset: u32, len: u32) {
        if len == 0 {
            return;
        }
        let mut start = offset;
        let mut end = offset.saturating_add(len);
        let mut idx = 0usize;
        while idx < FREE_REGION_CAPACITY {
            let region = self.free_regions[idx];
            if !region.occupied {
                idx += 1;
                continue;
            }
            let region_start = region.offset;
            let region_end = region.offset.saturating_add(region.len);
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
                    if region.occupied
                        && region.offset.saturating_add(region.len) == self.image_frontier
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
            return;
        }

        if let Some(idx) = self.first_empty_free_region_slot() {
            self.free_regions[idx] = FreeRegion {
                offset: start,
                len: end.saturating_sub(start),
                occupied: true,
            };
        }
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
            if !region.occupied {
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
            let prefix_len = alloc_start.saturating_sub(region_start);
            let suffix_len = region_end.saturating_sub(alloc_end);
            let fragments = usize::from(prefix_len != 0) + usize::from(suffix_len != 0);
            if self.free_region_empty_slots() + 1 < fragments {
                idx += 1;
                continue;
            }
            self.clear_free_region(idx);
            if prefix_len != 0 {
                self.release_persistent_region(region.offset, prefix_len as u32);
            }
            if suffix_len != 0 {
                self.release_persistent_region(alloc_end as u32, suffix_len as u32);
            }
            return Some((
                /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
                unsafe { slab_ptr.add(alloc_start) },
                alloc_start as u32,
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
            /* SAFETY: rendezvous core owns the resident slab slot and has checked lane/session generation before raw access. */
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
        self.set_image_frontier(end as u32);
        Some((
            /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
            unsafe { slab_ptr.add(start) },
            start as u32,
        ))
    }

    #[inline]
    pub(crate) fn allocate_external_persistent_sidecar_bytes(
        &mut self,
        bytes: usize,
        align: usize,
    ) -> Option<(*mut u8, usize)> {
        let prior_frontier = self.image_frontier;
        let (ptr, offset) = /* SAFETY: rendezvous core owns the resident slab slot and has checked lane/session generation before raw access. */ unsafe { self.allocate_persistent_sidecar_bytes(bytes, align) }?;
        let reclaim_delta = if offset > prior_frontier {
            offset.saturating_sub(prior_frontier) as usize
        } else {
            0
        };
        Some((ptr, reclaim_delta))
    }

    #[inline]
    pub(crate) fn reclaim_offset_for_payload(&self, ptr: *mut u8, reclaim_delta: usize) -> u32 {
        let (slab_ptr, _) = self.slab_ptr_and_len();
        let base = slab_ptr.addr();
        let payload_start = ptr.addr().saturating_sub(base);
        let reclaim_start = payload_start.checked_sub(reclaim_delta).unwrap();
        u32::try_from(reclaim_start).unwrap()
    }

    #[inline]
    pub(crate) fn free_bound_persistent_region(
        &mut self,
        reclaim_offset: u32,
        ptr: *mut u8,
        bytes: usize,
    ) {
        if ptr.is_null() || bytes == 0 {
            return;
        }
        let (slab_ptr, _) = self.slab_ptr_and_len();
        let base = slab_ptr.addr();
        let payload_start = ptr.addr().saturating_sub(base);
        let reclaim_start = reclaim_offset as usize;
        let payload_end = payload_start.checked_add(bytes).unwrap();
        let release_len = payload_end.checked_sub(reclaim_start).unwrap();
        let release_len = u32::try_from(release_len).unwrap();
        self.release_persistent_region(reclaim_offset, release_len);
    }

    #[inline]
    pub(crate) fn free_external_persistent_sidecar_bytes(
        &mut self,
        ptr: *mut u8,
        bytes: usize,
        reclaim_delta: usize,
    ) {
        if ptr.is_null() || bytes == 0 {
            return;
        }
        let reclaim_offset = self.reclaim_offset_for_payload(ptr, reclaim_delta);
        self.free_bound_persistent_region(reclaim_offset, ptr, bytes);
    }

    #[inline]
    fn endpoint_lease(
        &self,
        lease_slot: EndpointLeaseId,
        generation: u32,
    ) -> Option<&EndpointLeaseSlot> {
        let idx = usize::from(lease_slot);
        if idx >= usize::from(self.endpoint_lease_capacity) {
            return None;
        }
        let slot = /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */ unsafe { &*self.endpoint_leases.add(idx) };
        if slot.occupied && slot.generation == generation {
            Some(slot)
        } else {
            None
        }
    }

    #[inline]
    pub(crate) fn endpoint_lease_mut(
        &mut self,
        lease_slot: EndpointLeaseId,
        generation: u32,
    ) -> Option<&mut EndpointLeaseSlot> {
        let idx = usize::from(lease_slot);
        if idx >= usize::from(self.endpoint_lease_capacity) {
            return None;
        }
        let slot = /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */ unsafe { &mut *self.endpoint_leases.add(idx) };
        if slot.occupied && slot.generation == generation {
            Some(slot)
        } else {
            None
        }
    }

    #[inline]
    pub(crate) const fn endpoint_lease_capacity(&self) -> EndpointLeaseId {
        self.endpoint_lease_capacity
    }

    #[inline]
    pub(crate) fn next_endpoint_lease_generation(slot: &mut EndpointLeaseSlot) -> u32 {
        let next = slot.generation.wrapping_add(1);
        if next == 0 { 1 } else { next }
    }

    #[inline]
    fn endpoint_lease_storage_bytes(capacity: usize) -> Option<usize> {
        capacity.checked_mul(core::mem::size_of::<EndpointLeaseSlot>())
    }

    pub(crate) fn ensure_endpoint_lease_capacity(&mut self, required_slots: usize) -> Option<()> {
        let current = usize::from(self.endpoint_lease_capacity);
        if required_slots <= current {
            return Some(());
        }
        let endpoint_lease_capacity = EndpointLeaseId::try_from(required_slots).ok()?;
        let bytes = Self::endpoint_lease_storage_bytes(required_slots)?;
        let old_ptr = self.endpoint_leases;
        let old_bytes = Self::endpoint_lease_storage_bytes(current).unwrap_or(0);
        let old_reclaim_delta = usize::from(self.endpoint_lease_reclaim_delta);
        let (storage, reclaim_delta) = self.allocate_external_persistent_sidecar_bytes(
            bytes,
            core::mem::align_of::<EndpointLeaseSlot>(),
        )?;
        let new_ptr = storage.cast::<EndpointLeaseSlot>();
        let mut idx = 0usize;
        while idx < required_slots {
            let slot = if idx < current {
                /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
                unsafe { *old_ptr.add(idx) }
            } else {
                EndpointLeaseSlot::EMPTY
            };
            /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
            unsafe {
                new_ptr.add(idx).write(slot);
            }
            idx += 1;
        }
        self.endpoint_leases = new_ptr;
        self.endpoint_lease_capacity = endpoint_lease_capacity;
        self.endpoint_lease_reclaim_delta = u16::try_from(reclaim_delta).unwrap_or(u16::MAX);
        if !old_ptr.is_null() && old_bytes != 0 {
            self.free_external_persistent_sidecar_bytes(
                old_ptr.cast::<u8>(),
                old_bytes,
                old_reclaim_delta,
            );
        }
        Some(())
    }

    #[inline]
    pub(crate) fn endpoint_lease_storage(
        &self,
        lease_slot: EndpointLeaseId,
        generation: u32,
    ) -> Option<(usize, usize)> {
        let slot = self.endpoint_lease(lease_slot, generation)?;
        Some((slot.offset as usize, slot.len as usize))
    }

    #[inline]
    pub(crate) fn public_endpoint_lease_by_index(
        &self,
        idx: usize,
    ) -> Option<(EndpointLeaseId, u32)> {
        if idx >= usize::from(self.endpoint_lease_capacity) {
            return None;
        }
        let slot = /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */ unsafe { &*self.endpoint_leases.add(idx) };
        if !slot.occupied || !slot.public_endpoint {
            return None;
        }
        Some((EndpointLeaseId::try_from(idx).ok()?, slot.generation))
    }

    #[inline]
    pub(crate) fn resident_route_frame_slots_floor(&self) -> usize {
        let mut required = 0usize;
        let mut idx = 0usize;
        while idx < usize::from(self.endpoint_lease_capacity) {
            let slot = /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */ unsafe { &*self.endpoint_leases.add(idx) };
            if slot.occupied {
                required =
                    core::cmp::max(required, slot.resident_budget.route_frame_slots as usize);
            }
            idx += 1;
        }
        required
    }

    #[inline]
    pub(crate) fn resident_route_lane_slots_floor(&self) -> usize {
        let mut required = 0usize;
        let mut idx = 0usize;
        while idx < usize::from(self.endpoint_lease_capacity) {
            let slot = /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */ unsafe { &*self.endpoint_leases.add(idx) };
            if slot.occupied {
                required = core::cmp::max(required, slot.resident_budget.route_lane_slots as usize);
            }
            idx += 1;
        }
        required
    }

    pub(crate) fn resident_loop_slots_floor(&self) -> usize {
        let mut required = 0usize;
        let mut idx = 0usize;
        while idx < usize::from(self.endpoint_lease_capacity) {
            let slot = /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */ unsafe { &*self.endpoint_leases.add(idx) };
            if slot.occupied {
                required = core::cmp::max(required, slot.resident_budget.loop_slots as usize);
            }
            idx += 1;
        }
        required
    }

    #[inline]
    pub(crate) fn resident_cap_entries_floor(&self) -> usize {
        let mut required = self.caps.live_count();
        let mut idx = 0usize;
        while idx < usize::from(self.endpoint_lease_capacity) {
            let slot = /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */ unsafe { &*self.endpoint_leases.add(idx) };
            if slot.occupied {
                required = required.saturating_add(slot.resident_budget.cap_entries as usize);
            }
            idx += 1;
        }
        required
    }

    #[inline]
    pub(crate) fn resident_frontier_workspace_floor(&self) -> usize {
        let mut required = 0usize;
        let mut idx = 0usize;
        while idx < usize::from(self.endpoint_lease_capacity) {
            let slot = /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */ unsafe { &*self.endpoint_leases.add(idx) };
            if slot.occupied {
                required = core::cmp::max(
                    required,
                    slot.resident_budget.frontier_workspace_bytes as usize,
                );
            }
            idx += 1;
        }
        required
    }

    #[inline]
    pub(crate) fn ensure_frontier_workspace_capacity(
        &mut self,
        required_bytes: usize,
    ) -> Option<()> {
        let required_bytes = required_bytes.min(u32::MAX as usize);
        if required_bytes <= self.frontier_workspace_bytes as usize {
            return Some(());
        }
        let floor = (self.image_frontier as usize).checked_add(required_bytes)?;
        if floor > self.endpoint_storage_floor() {
            return None;
        }
        self.set_frontier_workspace_bytes(required_bytes as u32);
        Some(())
    }

    #[inline]
    pub(crate) fn recompute_frontier_workspace_bytes(&mut self) {
        self.set_frontier_workspace_bytes(self.resident_frontier_workspace_floor() as u32);
    }

    #[inline]
    fn lane_base(&self) -> u32 {
        self.lane_range.start
    }

    #[inline]
    fn lane_slot_count(&self) -> usize {
        self.lane_range.end.saturating_sub(self.lane_range.start) as usize
    }

    pub(crate) fn ensure_route_table_capacity(
        &mut self,
        required_frame_slots: usize,
        required_lane_slots: usize,
    ) -> Option<()> {
        if required_frame_slots == 0
            || (self.routes.route_slots() >= required_frame_slots
                && self.routes.lane_slots() >= required_lane_slots)
        {
            return Some(());
        }
        let prior_frontier = self.image_frontier;
        let (storage, storage_offset) = /* SAFETY: rendezvous core owns the resident slab slot and has checked lane/session generation before raw access. */ unsafe {
            self.allocate_persistent_sidecar_bytes(
                RouteTable::storage_bytes(required_frame_slots, required_lane_slots),
                RouteTable::storage_align(),
            )
        }?;
        let reclaim_delta = storage_offset.saturating_sub(prior_frontier) as usize;
        let old_ptr = self.routes.storage_ptr();
        let old_bytes = self.routes.storage_bytes_current();
        let old_reclaim_offset =
            self.reclaim_offset_for_payload(old_ptr, self.routes.storage_reclaim_delta());
        if self.routes.route_slots() == 0 {
            /* SAFETY: rendezvous core owns the resident slab slot and has checked lane/session generation before raw access. */
            unsafe {
                self.routes.bind_from_storage_with_layout(
                    storage,
                    required_frame_slots,
                    self.lane_base(),
                    required_lane_slots,
                    reclaim_delta,
                );
            }
        } else {
            /* SAFETY: rendezvous core owns the resident slab slot and has checked lane/session generation before raw access. */
            unsafe {
                self.routes.migrate_from_storage(
                    storage,
                    required_frame_slots,
                    self.lane_base(),
                    required_lane_slots,
                );
                self.routes.rebind_from_storage(
                    storage,
                    required_frame_slots,
                    self.lane_base(),
                    required_lane_slots,
                    reclaim_delta,
                );
            }
            self.free_bound_persistent_region(old_reclaim_offset, old_ptr, old_bytes);
        }
        Some(())
    }

    pub(crate) fn ensure_loop_table_capacity(&mut self, required_slots: usize) -> Option<()> {
        let required_lane_slots = self.lane_slot_count();
        if required_slots == 0 || self.loops.loop_slots() >= required_slots {
            return Some(());
        }
        let prior_frontier = self.image_frontier;
        let (storage, storage_offset) = /* SAFETY: rendezvous core owns the resident slab slot and has checked lane/session generation before raw access. */ unsafe {
            self.allocate_persistent_sidecar_bytes(
                LoopTable::storage_bytes(required_slots, required_lane_slots),
                LoopTable::storage_align(),
            )
        }?;
        let reclaim_delta = storage_offset.saturating_sub(prior_frontier) as usize;
        let old_ptr = self.loops.storage_ptr();
        let old_bytes = self.loops.storage_bytes_current();
        let old_reclaim_offset =
            self.reclaim_offset_for_payload(old_ptr, self.loops.storage_reclaim_delta());
        if self.loops.loop_slots() == 0 {
            /* SAFETY: rendezvous core owns the resident slab slot and has checked lane/session generation before raw access. */
            unsafe {
                self.loops.bind_from_storage(
                    storage,
                    required_slots,
                    self.lane_base(),
                    required_lane_slots,
                    reclaim_delta,
                );
            }
        } else {
            /* SAFETY: rendezvous core owns the resident slab slot and has checked lane/session generation before raw access. */
            unsafe {
                self.loops.migrate_from_storage(
                    storage,
                    required_slots,
                    self.lane_base(),
                    required_lane_slots,
                );
                self.loops.rebind_from_storage(
                    storage,
                    required_slots,
                    self.lane_base(),
                    required_lane_slots,
                    reclaim_delta,
                );
            }
            self.free_bound_persistent_region(old_reclaim_offset, old_ptr, old_bytes);
        }
        Some(())
    }

    fn ensure_generation_table_storage(&mut self) -> Option<()> {
        if self.r#gen.is_bound() || self.lane_slot_count() == 0 {
            return Some(());
        }
        let lane_slots = self.lane_slot_count();
        let (storage, _) = /* SAFETY: rendezvous core owns the resident slab slot and has checked lane/session generation before raw access. */ unsafe {
            self.allocate_persistent_sidecar_bytes(
                GenTable::storage_bytes(lane_slots),
                GenTable::storage_align(),
            )
        }?;
        /* SAFETY: rendezvous core owns the resident slab slot and has checked lane/session generation before raw access. */
        unsafe {
            self.r#gen
                .bind_from_storage(storage, self.lane_base(), lane_slots);
        }
        Some(())
    }

    fn ensure_assoc_table_storage(&mut self) -> Option<()> {
        if self.assoc.is_bound() || self.lane_slot_count() == 0 {
            return Some(());
        }
        let lane_slots = self.lane_slot_count();
        let (storage, _) = /* SAFETY: rendezvous core owns the resident slab slot and has checked lane/session generation before raw access. */ unsafe {
            self.allocate_persistent_sidecar_bytes(
                AssocTable::storage_bytes(lane_slots),
                AssocTable::storage_align(),
            )
        }?;
        /* SAFETY: rendezvous core owns the resident slab slot and has checked lane/session generation before raw access. */
        unsafe {
            self.assoc
                .bind_from_storage(storage, self.lane_base(), lane_slots);
        }
        Some(())
    }

    fn ensure_checkpoint_table_storage(&mut self) -> Option<()> {
        if self.state_snapshots.is_bound() || self.lane_slot_count() == 0 {
            return Some(());
        }
        let lane_slots = self.lane_slot_count();
        let (storage, _) = /* SAFETY: rendezvous core owns the resident slab slot and has checked lane/session generation before raw access. */ unsafe {
            self.allocate_persistent_sidecar_bytes(
                StateSnapshotTable::storage_bytes(lane_slots),
                StateSnapshotTable::storage_align(),
            )
        }?;
        /* SAFETY: rendezvous core owns the resident slab slot and has checked lane/session generation before raw access. */
        unsafe {
            self.state_snapshots
                .bind_from_storage(storage, self.lane_base(), lane_slots);
        }
        Some(())
    }

    pub(crate) fn ensure_cap_table_capacity(&mut self, required_entries: usize) -> Option<()> {
        if required_entries == 0 || self.caps.capacity() >= required_entries {
            return Some(());
        }
        let prior_frontier = self.image_frontier;
        let (storage, storage_offset) = /* SAFETY: rendezvous core owns the resident slab slot and has checked lane/session generation before raw access. */ unsafe {
            self.allocate_persistent_sidecar_bytes(
                CapTable::storage_bytes(required_entries),
                CapTable::storage_align(),
            )
        }?;
        let reclaim_delta = storage_offset.saturating_sub(prior_frontier) as usize;
        let old_ptr = self.caps.storage_ptr();
        let old_bytes = self.caps.storage_bytes_current();
        let old_reclaim_offset =
            self.reclaim_offset_for_payload(old_ptr, self.caps.storage_reclaim_delta());
        if self.caps.capacity() == 0 {
            /* SAFETY: rendezvous core owns the resident slab slot and has checked lane/session generation before raw access. */
            unsafe {
                self.caps
                    .bind_from_storage(storage, required_entries, reclaim_delta);
            }
        } else {
            let migrated = /* SAFETY: rendezvous core owns the resident slab slot and has checked lane/session generation before raw access. */ unsafe { self.caps.migrate_from_storage(storage, required_entries) };
            if !migrated {
                self.free_bound_persistent_region(
                    storage_offset.saturating_sub(reclaim_delta as u32),
                    storage,
                    CapTable::storage_bytes(required_entries),
                );
                return None;
            }
            /* SAFETY: rendezvous core owns the resident slab slot and has checked lane/session generation before raw access. */
            unsafe {
                self.caps
                    .rebind_from_storage(storage, required_entries, reclaim_delta);
            }
            self.free_bound_persistent_region(old_reclaim_offset, old_ptr, old_bytes);
        }
        Some(())
    }

    fn ensure_topology_table_storage(&mut self) -> Option<()> {
        let lane_slots = self.lane_slot_count();
        if self.topology.is_bound() && self.topology.lane_slots() >= lane_slots {
            return Some(());
        }
        let old_ptr = self.topology.storage_ptr();
        let old_bytes = self.topology.storage_bytes_current();
        let (storage, _) = /* SAFETY: rendezvous core owns the resident slab slot and has checked lane/session generation before raw access. */ unsafe {
            self.allocate_persistent_sidecar_bytes(
                TopologyStateTable::storage_bytes(lane_slots),
                TopologyStateTable::storage_align(),
            )
        }?;
        /* SAFETY: rendezvous core owns the resident slab slot and has checked lane/session generation before raw access. */
        unsafe {
            if self.topology.is_bound() {
                self.topology
                    .rebind_from_storage_preserving(storage, self.lane_base(), lane_slots);
            } else {
                self.topology
                    .bind_from_storage(storage, self.lane_base(), lane_slots);
            }
        }
        self.free_external_persistent_sidecar_bytes(old_ptr, old_bytes, 0);
        Some(())
    }

    pub(crate) fn ensure_topology_control_storage(&mut self) -> Option<()> {
        self.ensure_topology_table_storage()
    }

    pub(crate) fn prepare_topology_control_scope(&mut self, lane: Lane) -> Option<()> {
        self.ensure_core_lane_storage_for_lane_slots((lane.raw() as usize).saturating_add(1))?;
        self.ensure_topology_control_storage()?;
        self.initialise_control_scope(lane, ControlScopeKind::Topology);
        Some(())
    }

    pub(crate) fn ensure_policy_table_storage(&mut self) -> Option<()> {
        if self.policies.is_bound() {
            return Some(());
        }
        let lane_slots = self.lane_slot_count();
        let (storage, _) = /* SAFETY: rendezvous core owns the resident slab slot and has checked lane/session generation before raw access. */ unsafe {
            self.allocate_persistent_sidecar_bytes(
                PolicyTable::storage_bytes(lane_slots),
                PolicyTable::storage_align(),
            )
        }?;
        /* SAFETY: rendezvous core owns the resident slab slot and has checked lane/session generation before raw access. */
        unsafe {
            self.policies
                .bind_from_storage(storage, self.lane_base(), lane_slots);
        }
        Some(())
    }

    pub(crate) fn ensure_core_lane_storage(&mut self) -> Option<()> {
        self.ensure_generation_table_storage()?;
        self.ensure_assoc_table_storage()?;
        self.ensure_checkpoint_table_storage()?;
        self.ensure_policy_table_storage()?;
        Some(())
    }

    pub(crate) fn expand_bound_core_lane_storage(
        &mut self,
        required_lane_slots: usize,
    ) -> Option<()> {
        let lane_base = self.lane_base();
        let old_gen_ptr = self.r#gen.storage_ptr();
        let old_gen_bytes = self.r#gen.storage_bytes_current();
        let old_assoc_ptr = self.assoc.storage_ptr();
        let old_assoc_bytes = self.assoc.storage_bytes_current();
        let old_snapshot_ptr = self.state_snapshots.storage_ptr();
        let old_snapshot_bytes = self.state_snapshots.storage_bytes_current();

        let (gen_storage, gen_reclaim) = self.allocate_external_persistent_sidecar_bytes(
            GenTable::storage_bytes(required_lane_slots),
            GenTable::storage_align(),
        )?;
        let Some((assoc_storage, assoc_reclaim)) = self.allocate_external_persistent_sidecar_bytes(
            AssocTable::storage_bytes(required_lane_slots),
            AssocTable::storage_align(),
        ) else {
            self.free_external_persistent_sidecar_bytes(
                gen_storage,
                GenTable::storage_bytes(required_lane_slots),
                gen_reclaim,
            );
            return None;
        };
        let Some((snapshot_storage, _snapshot_reclaim)) = self
            .allocate_external_persistent_sidecar_bytes(
                StateSnapshotTable::storage_bytes(required_lane_slots),
                StateSnapshotTable::storage_align(),
            )
        else {
            self.free_external_persistent_sidecar_bytes(
                assoc_storage,
                AssocTable::storage_bytes(required_lane_slots),
                assoc_reclaim,
            );
            self.free_external_persistent_sidecar_bytes(
                gen_storage,
                GenTable::storage_bytes(required_lane_slots),
                gen_reclaim,
            );
            return None;
        };

        /* SAFETY: rendezvous core owns the resident slab slot and has checked lane/session generation before raw access. */
        unsafe {
            self.r#gen
                .rebind_from_storage_preserving(gen_storage, lane_base, required_lane_slots);
            self.assoc.rebind_from_storage_preserving(
                assoc_storage,
                lane_base,
                required_lane_slots,
            );
            self.state_snapshots.rebind_from_storage_preserving(
                snapshot_storage,
                lane_base,
                required_lane_slots,
            );
        }
        if self.policies.is_bound() {
            self.policies
                .rebind_lane_span(lane_base, required_lane_slots);
        }
        self.lane_range = lane_base..lane_base + required_lane_slots as u32;
        self.free_external_persistent_sidecar_bytes(old_gen_ptr, old_gen_bytes, 0);
        self.free_external_persistent_sidecar_bytes(old_assoc_ptr, old_assoc_bytes, 0);
        self.free_external_persistent_sidecar_bytes(old_snapshot_ptr, old_snapshot_bytes, 0);
        Some(())
    }
}
