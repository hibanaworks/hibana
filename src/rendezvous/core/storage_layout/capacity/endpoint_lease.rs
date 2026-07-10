use super::super::{EndpointLeaseSlot, Rendezvous, Transport};
use crate::{rendezvous::core::EndpointLeaseId, session::cluster::error::ResourceScope};

impl<'rv, 'cfg, T: Transport> Rendezvous<'rv, 'cfg, T>
where
    'cfg: 'rv,
{
    #[inline]
    pub(crate) fn endpoint_lease_slot_count(&self) -> usize {
        EndpointLeaseSlot::storage_slot_count(self.endpoint_lease_storage.get())
    }

    #[inline]
    pub(crate) fn endpoint_lease_slot_by_index(&self, idx: usize) -> Option<EndpointLeaseSlot> {
        if idx >= self.endpoint_lease_slot_count() {
            return None;
        }
        let endpoint_leases = self.endpoint_leases_ptr();
        /* SAFETY: `idx` is within the initialized endpoint-lease table owned by
        this `Rendezvous`; endpoint lease records are copied out by value. */
        Some(unsafe { *endpoint_leases.add(idx) })
    }

    #[inline]
    pub(crate) fn write_endpoint_lease_slot(&self, idx: usize, slot: EndpointLeaseSlot) {
        if idx >= self.endpoint_lease_slot_count() {
            crate::invariant();
        }
        let endpoint_leases = self.endpoint_leases_ptr();
        /* SAFETY: `idx` is inside the owner-bound endpoint lease table. The
        local-only rendezvous serializes lease-table updates and publishes each
        complete record with one write. */
        unsafe {
            endpoint_leases.add(idx).write(slot);
        }
    }

    #[inline]
    fn endpoint_lease(
        &self,
        lease_slot: EndpointLeaseId,
        generation: u32,
    ) -> Option<EndpointLeaseSlot> {
        let idx = usize::from(lease_slot);
        if idx >= self.endpoint_lease_slot_count() {
            return None;
        }
        let slot = crate::invariant_some(self.endpoint_lease_slot_by_index(idx));
        if slot.is_live() && slot.generation == generation {
            Some(slot)
        } else {
            None
        }
    }

    #[inline]
    pub(crate) fn next_endpoint_lease_generation(slot: EndpointLeaseSlot) -> u32 {
        let next = slot.generation.wrapping_add(1);
        if next == 0 { 1 } else { next }
    }

    #[inline]
    fn endpoint_lease_storage_bytes(capacity: usize) -> Option<usize> {
        capacity.checked_mul(core::mem::size_of::<EndpointLeaseSlot>())
    }

    pub(crate) fn ensure_endpoint_lease_capacity(
        &self,
        required_slots: usize,
    ) -> Result<(), ResourceScope> {
        let current = self.endpoint_lease_slot_count();
        if required_slots <= current {
            return Ok(());
        }
        if EndpointLeaseId::try_from(required_slots).is_err() {
            return Err(ResourceScope::EndpointLease);
        }
        let bytes = Self::endpoint_lease_storage_bytes(required_slots)
            .ok_or(ResourceScope::EndpointLease)?;
        let source_sidecar = self.endpoint_lease_storage.get();
        let lease = self
            .allocate_persistent_sidecar_bytes(bytes, core::mem::align_of::<EndpointLeaseSlot>())
            .ok_or(ResourceScope::EndpointLease)?;
        let new_ptr = lease.ptr().cast::<EndpointLeaseSlot>();
        let mut idx = 0usize;
        while idx < required_slots {
            let slot = if idx < current {
                crate::invariant_some(self.endpoint_lease_slot_by_index(idx))
            } else {
                EndpointLeaseSlot::EMPTY
            };
            /* SAFETY: `lease` is the freshly allocated endpoint-lease sidecar
            with `required_slots` capacity; this loop writes each destination
            slot once before publishing `endpoint_lease_storage`. */
            unsafe {
                new_ptr.add(idx).write(slot);
            }
            idx += 1;
        }
        self.endpoint_lease_storage
            .set(lease.cast::<EndpointLeaseSlot>());
        self.retire_persistent_sidecar(source_sidecar.cast());
        Ok(())
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
    pub(crate) fn resident_route_frame_slots_floor(&self) -> usize {
        let mut required = 0usize;
        let mut idx = 0usize;
        while idx < self.endpoint_lease_slot_count() {
            let slot = crate::invariant_some(self.endpoint_lease_slot_by_index(idx));
            if slot.is_live() {
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
        while idx < self.endpoint_lease_slot_count() {
            let slot = crate::invariant_some(self.endpoint_lease_slot_by_index(idx));
            if slot.is_live() {
                required = core::cmp::max(required, slot.resident_budget.route_lane_slots as usize);
            }
            idx += 1;
        }
        required
    }

    #[inline]
    pub(crate) fn resident_frontier_workspace_floor(&self) -> usize {
        let mut required = 0usize;
        let mut idx = 0usize;
        while idx < self.endpoint_lease_slot_count() {
            let slot = crate::invariant_some(self.endpoint_lease_slot_by_index(idx));
            if slot.is_live() {
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
        &self,
        required_bytes: usize,
    ) -> Result<(), ResourceScope> {
        if required_bytes > u32::MAX as usize {
            return Err(ResourceScope::EndpointLease);
        }
        if required_bytes <= self.frontier_workspace_bytes.get() as usize {
            return Ok(());
        }
        let floor = (self.image_frontier.get() as usize)
            .checked_add(required_bytes)
            .ok_or(ResourceScope::EndpointLease)?;
        if floor > self.endpoint_storage_floor() {
            return Err(ResourceScope::EndpointLease);
        }
        self.set_frontier_workspace_bytes(required_bytes as u32);
        Ok(())
    }

    #[inline]
    pub(crate) fn recompute_frontier_workspace_bytes(&self) {
        let required = self.resident_frontier_workspace_floor();
        let required = crate::invariant_ok(u32::try_from(required));
        self.set_frontier_workspace_bytes(required);
    }
}
