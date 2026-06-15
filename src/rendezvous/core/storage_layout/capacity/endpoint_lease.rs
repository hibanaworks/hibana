use super::super::{EndpointLeaseId, EndpointLeaseSlot, Rendezvous, Transport};
use crate::session::cluster::error::ResourceScope;

impl<'rv, 'cfg, T: Transport> Rendezvous<'rv, 'cfg, T>
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
        let endpoint_leases = self.endpoint_leases_ptr();
        let slot = /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */ unsafe { &*endpoint_leases.add(idx) };
        if slot.is_live() && slot.generation == generation {
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
        let endpoint_leases = self.endpoint_leases_ptr();
        let slot = /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */ unsafe { &mut *endpoint_leases.add(idx) };
        if slot.is_live() && slot.generation == generation {
            Some(slot)
        } else {
            None
        }
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

    pub(crate) fn ensure_endpoint_lease_capacity(
        &mut self,
        required_slots: usize,
    ) -> Result<(), ResourceScope> {
        let current = usize::from(self.endpoint_lease_capacity);
        if required_slots <= current {
            return Ok(());
        }
        let endpoint_lease_capacity =
            EndpointLeaseId::try_from(required_slots).map_err(|_| ResourceScope::EndpointLease)?;
        let bytes = Self::endpoint_lease_storage_bytes(required_slots)
            .ok_or(ResourceScope::EndpointLease)?;
        let source_sidecar = self.endpoint_lease_storage;
        let source_ptr = source_sidecar.ptr();
        let source_bytes =
            Self::endpoint_lease_storage_bytes(current).ok_or(ResourceScope::EndpointLease)?;
        let lease = self
            .allocate_external_persistent_sidecar_bytes(
                bytes,
                core::mem::align_of::<EndpointLeaseSlot>(),
            )
            .ok_or(ResourceScope::EndpointLease)?;
        let new_ptr = lease.ptr().cast::<EndpointLeaseSlot>();
        let mut idx = 0usize;
        while idx < required_slots {
            let slot = if idx < current {
                /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
                unsafe { *source_ptr.add(idx) }
            } else {
                EndpointLeaseSlot::EMPTY
            };
            /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
            unsafe {
                new_ptr.add(idx).write(slot);
            }
            idx += 1;
        }
        if !source_ptr.is_null()
            && source_bytes != 0
            && let Err(error) = self.free_external_persistent_sidecar(
                source_sidecar.cast(),
                ResourceScope::EndpointLease,
            )
        {
            if self
                .free_external_persistent_sidecar(lease, ResourceScope::EndpointLease)
                .is_err()
            {
                crate::invariant();
            }
            return Err(error);
        }
        self.endpoint_lease_storage = lease.cast::<EndpointLeaseSlot>();
        self.endpoint_lease_capacity = endpoint_lease_capacity;
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
        let endpoint_leases = self.endpoint_leases_ptr();
        let mut idx = 0usize;
        while idx < usize::from(self.endpoint_lease_capacity) {
            let slot = /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */ unsafe { &*endpoint_leases.add(idx) };
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
        let endpoint_leases = self.endpoint_leases_ptr();
        let mut idx = 0usize;
        while idx < usize::from(self.endpoint_lease_capacity) {
            let slot = /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */ unsafe { &*endpoint_leases.add(idx) };
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
        let endpoint_leases = self.endpoint_leases_ptr();
        let mut idx = 0usize;
        while idx < usize::from(self.endpoint_lease_capacity) {
            let slot = /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */ unsafe { &*endpoint_leases.add(idx) };
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
        &mut self,
        required_bytes: usize,
    ) -> Result<(), ResourceScope> {
        if required_bytes > u32::MAX as usize {
            return Err(ResourceScope::EndpointLease);
        }
        if required_bytes <= self.frontier_workspace_bytes as usize {
            return Ok(());
        }
        let floor = (self.image_frontier as usize)
            .checked_add(required_bytes)
            .ok_or(ResourceScope::EndpointLease)?;
        if floor > self.endpoint_storage_floor() {
            return Err(ResourceScope::EndpointLease);
        }
        self.set_frontier_workspace_bytes(required_bytes as u32);
        Ok(())
    }

    #[inline]
    pub(crate) fn recompute_frontier_workspace_bytes(&mut self) {
        let required = self.resident_frontier_workspace_floor();
        let required = crate::invariant_ok(u32::try_from(required));
        self.set_frontier_workspace_bytes(required);
    }
}
