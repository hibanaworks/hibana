use super::super::{
    Clock, EndpointLeaseId, EndpointLeaseSlot, LabelUniverse, Rendezvous, Transport,
};

impl<'rv, 'cfg, T: Transport, U: LabelUniverse, C: Clock, E: crate::control::cap::mint::EpochTable>
    Rendezvous<'rv, 'cfg, T, U, C, E>
where
    'cfg: 'rv,
{
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
        let old_bytes = Self::endpoint_lease_storage_bytes(current)?;
        let old_reclaim_delta = usize::from(self.endpoint_lease_reclaim_delta);
        let (storage, reclaim_delta) = self.allocate_external_persistent_sidecar_bytes(
            bytes,
            core::mem::align_of::<EndpointLeaseSlot>(),
        )?;
        let Ok(reclaim_delta_u16) = u16::try_from(reclaim_delta) else {
            self.free_external_persistent_sidecar_bytes(storage, bytes, reclaim_delta);
            return None;
        };
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
        self.endpoint_lease_reclaim_delta = reclaim_delta_u16;
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
                required = required
                    .checked_add(slot.resident_budget.cap_entries as usize)
                    .expect("invariant");
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
        if required_bytes > u32::MAX as usize {
            return None;
        }
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
        let required = self.resident_frontier_workspace_floor();
        let required = u32::try_from(required).expect("invariant");
        self.set_frontier_workspace_bytes(required);
    }
}
