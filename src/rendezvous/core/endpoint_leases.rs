use super::{
    Clock, EndpointLeaseId, EndpointLeaseSlot, EndpointResidentBudget, Rendezvous, Transport,
};
impl<'rv, 'cfg, T: Transport, C: Clock> Rendezvous<'rv, 'cfg, T, C>
where
    'cfg: 'rv,
{
    #[inline]
    pub(crate) fn mark_public_endpoint_lease(
        &mut self,
        lease_slot: EndpointLeaseId,
        generation: u32,
    ) -> bool {
        if let Some(slot) = self.endpoint_lease_mut(lease_slot, generation) {
            slot.public_endpoint = true;
            return true;
        }
        false
    }

    #[inline]
    pub(crate) unsafe fn allocate_endpoint_lease(
        &mut self,
        bytes: usize,
        align: usize,
        resident_budget: EndpointResidentBudget,
    ) -> Option<(EndpointLeaseId, u32, usize, usize)> {
        let mut has_empty_slot = false;
        let mut slot_idx = 0usize;
        while slot_idx < usize::from(self.endpoint_lease_capacity) {
            let slot = /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */ unsafe { &*self.endpoint_leases.add(slot_idx) };
            if !slot.occupied {
                has_empty_slot = true;
                break;
            }
            slot_idx += 1;
        }
        if !has_empty_slot {
            let required_slots = usize::from(self.endpoint_lease_capacity).checked_add(1)?;
            self.ensure_endpoint_lease_capacity(required_slots).ok()?;
        }
        let (slab_ptr, slab_len) = self.slab_ptr_and_len();
        let slab_base = slab_ptr as usize;
        let slab_end = slab_base.checked_add(slab_len)?;
        let lease_base = self.endpoint_leases as usize;
        let lease_bytes = usize::from(self.endpoint_lease_capacity)
            .checked_mul(core::mem::size_of::<EndpointLeaseSlot>())?;
        let lease_end = lease_base.checked_add(lease_bytes)?;
        assert!(
            lease_base >= slab_base && lease_end <= slab_end,
            "invariant"
        );
        let base = slab_ptr as usize;
        let floor = self.endpoint_lease_floor();
        let mut candidate_end = slab_len;

        loop {
            let mut best_idx = None;
            let mut best_offset = 0usize;
            let mut idx = 0usize;
            while idx < usize::from(self.endpoint_lease_capacity) {
                let slot = /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */ unsafe { &*self.endpoint_leases.add(idx) };
                let offset = slot.offset as usize;
                if slot.occupied && offset < candidate_end && offset >= best_offset {
                    best_offset = offset;
                    best_idx = Some(idx);
                }
                idx += 1;
            }

            let gap_start = match best_idx {
                Some(idx) => {
                    let slot = /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */ unsafe { &*self.endpoint_leases.add(idx) };
                    slot.offset as usize + slot.len as usize
                }
                None => floor,
            };
            let gap_end = candidate_end;
            if gap_end >= bytes {
                let offset_base = base.checked_add(gap_end.checked_sub(bytes)?)?;
                let offset = Self::align_down(offset_base, align).checked_sub(base)?;
                if offset >= gap_start && offset >= floor {
                    let lease_len = u32::try_from(bytes).ok()?;
                    let lease_offset = u32::try_from(offset).ok()?;
                    let mut insert_idx = 0usize;
                    while insert_idx < usize::from(self.endpoint_lease_capacity) {
                        let slot = /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */ unsafe { &mut *self.endpoint_leases.add(insert_idx) };
                        if !slot.occupied {
                            let generation = Self::next_endpoint_lease_generation(slot);
                            *slot = EndpointLeaseSlot {
                                generation,
                                offset: lease_offset,
                                len: lease_len,
                                resident_budget,
                                public_endpoint: false,
                                occupied: true,
                            };
                            return Some((
                                EndpointLeaseId::try_from(insert_idx).ok()?,
                                generation,
                                offset,
                                bytes,
                            ));
                        }
                        insert_idx += 1;
                    }
                    return None;
                }
            }

            let Some(idx) = best_idx else {
                break;
            };
            candidate_end = /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */ unsafe { (*self.endpoint_leases.add(idx)).offset as usize };
        }
        None
    }

    #[inline]
    pub(crate) fn release_endpoint_lease(&mut self, lease_slot: EndpointLeaseId, generation: u32) {
        let idx = usize::from(lease_slot);
        if idx >= usize::from(self.endpoint_lease_capacity) {
            return;
        }
        let slot = /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */ unsafe { &mut *self.endpoint_leases.add(idx) };
        if !slot.occupied || slot.generation != generation {
            return;
        }
        let generation = slot.generation;
        *slot = EndpointLeaseSlot {
            generation,
            ..EndpointLeaseSlot::EMPTY
        };
        self.recompute_frontier_workspace_bytes();
        self.trim_resident_headers_to_live_budget();
    }
}
