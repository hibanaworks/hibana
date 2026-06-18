use super::{
    EndpointLeaseId, EndpointLeaseSlot, EndpointLeaseState, EndpointResidentBudget, Rendezvous,
    RouteTable, Sidecar, Transport,
};
use crate::{session::cluster::error::ResourceScope, session::types::SessionId};
impl<'rv, 'cfg, T: Transport> Rendezvous<'rv, 'cfg, T>
where
    'cfg: 'rv,
{
    #[inline]
    pub(crate) fn ensure_endpoint_lease_live(
        &mut self,
        lease_slot: EndpointLeaseId,
        generation: u32,
    ) -> Result<(), ResourceScope> {
        if self.endpoint_lease_mut(lease_slot, generation).is_some() {
            Ok(())
        } else {
            Err(ResourceScope::EndpointMark)
        }
    }
    #[inline]
    pub(crate) unsafe fn allocate_endpoint_lease(
        &mut self,
        sid: SessionId,
        role: u8,
        bytes: usize,
        align: usize,
        resident_budget: EndpointResidentBudget,
    ) -> Result<(EndpointLeaseId, u32, usize, usize), ResourceScope> {
        if bytes > u32::MAX as usize {
            return Err(ResourceScope::EndpointLease);
        }
        let mut has_empty_slot = false;
        let mut slot_idx = 0usize;
        while slot_idx < usize::from(self.endpoint_lease_capacity) {
            let slot = crate::invariant_some(self.endpoint_lease_slot_by_index(slot_idx));
            if !slot.is_live() {
                has_empty_slot = true;
                break;
            }
            slot_idx += 1;
        }
        if !has_empty_slot {
            let required_slots = usize::from(self.endpoint_lease_capacity)
                .checked_add(1)
                .ok_or(ResourceScope::EndpointLease)?;
            self.ensure_endpoint_lease_capacity(required_slots)?;
        }
        let (slab_ptr, slab_len) = self.slab_ptr_and_len();
        let slab_base = slab_ptr as usize;
        let slab_end = slab_base
            .checked_add(slab_len)
            .ok_or(ResourceScope::EndpointLease)?;
        let lease_base = self.endpoint_leases_ptr() as usize;
        let lease_bytes = usize::from(self.endpoint_lease_capacity)
            .checked_mul(core::mem::size_of::<EndpointLeaseSlot>())
            .ok_or(ResourceScope::EndpointLease)?;
        let lease_end = lease_base
            .checked_add(lease_bytes)
            .ok_or(ResourceScope::EndpointLease)?;
        if lease_base < slab_base || lease_end > slab_end {
            crate::invariant();
        }
        let base = slab_ptr as usize;
        let floor = self.endpoint_lease_floor();
        let mut candidate_end = slab_len;
        loop {
            let mut best_idx = None;
            let mut best_offset = 0usize;
            let mut idx = 0usize;
            while idx < usize::from(self.endpoint_lease_capacity) {
                let slot = crate::invariant_some(self.endpoint_lease_slot_by_index(idx));
                let offset = slot.offset as usize;
                if slot.is_live() && offset < candidate_end && offset >= best_offset {
                    best_offset = offset;
                    best_idx = Some(idx);
                }
                idx += 1;
            }
            let gap_start = match best_idx {
                Some(idx) => {
                    let slot = crate::invariant_some(self.endpoint_lease_slot_by_index(idx));
                    slot.offset as usize + slot.len as usize
                }
                None => floor,
            };
            let gap_end = candidate_end;
            if gap_end >= bytes {
                let offset_base = base
                    .checked_add(
                        gap_end
                            .checked_sub(bytes)
                            .ok_or(ResourceScope::EndpointLease)?,
                    )
                    .ok_or(ResourceScope::EndpointLease)?;
                let offset = Self::align_down(offset_base, align)
                    .checked_sub(base)
                    .ok_or(ResourceScope::EndpointLease)?;
                if offset >= gap_start && offset >= floor {
                    if offset > u32::MAX as usize {
                        return Err(ResourceScope::EndpointLease);
                    }
                    let lease_len = crate::invariant_ok(u32::try_from(bytes));
                    let lease_offset = crate::invariant_ok(u32::try_from(offset));
                    let mut insert_idx = 0usize;
                    while insert_idx < usize::from(self.endpoint_lease_capacity) {
                        let slot = crate::invariant_some(
                            self.endpoint_lease_slot_by_index_mut(insert_idx),
                        );
                        if !slot.is_live() {
                            let generation = Self::next_endpoint_lease_generation(slot);
                            let lease_id =
                                crate::invariant_ok(EndpointLeaseId::try_from(insert_idx));
                            *slot = EndpointLeaseSlot {
                                generation,
                                sid,
                                role,
                                offset: lease_offset,
                                len: lease_len,
                                resident_budget,
                                state: EndpointLeaseState::Live,
                            };
                            return Ok((lease_id, generation, offset, bytes));
                        }
                        insert_idx += 1;
                    }
                    crate::invariant();
                }
            }
            let Some(idx) = best_idx else {
                break;
            };
            candidate_end =
                crate::invariant_some(self.endpoint_lease_slot_by_index(idx)).offset as usize;
        }
        Err(ResourceScope::EndpointLease)
    }
    #[inline]
    pub(crate) fn has_live_endpoint_session_role(&self, sid: SessionId, role: u8) -> bool {
        let mut idx = 0usize;
        while idx < usize::from(self.endpoint_lease_capacity) {
            let slot = crate::invariant_some(self.endpoint_lease_slot_by_index(idx));
            if slot.is_live() && slot.sid == sid && slot.role == role {
                return true;
            }
            idx += 1;
        }
        false
    }
    #[inline]
    pub(crate) fn release_endpoint_lease(
        &mut self,
        lease_slot: EndpointLeaseId,
        generation: u32,
    ) -> Result<(), ResourceScope> {
        let idx = usize::from(lease_slot);
        if idx >= usize::from(self.endpoint_lease_capacity) {
            return Ok(());
        }
        let slot = *crate::invariant_some(self.endpoint_lease_slot_by_index(idx));
        if !slot.is_live() || slot.generation != generation {
            return Ok(());
        }
        if self.routes.route_slots() != 0 {
            let mut required_route_frames = 0usize;
            let mut live_idx = 0usize;
            while live_idx < usize::from(self.endpoint_lease_capacity) {
                if live_idx != idx {
                    let live_slot =
                        crate::invariant_some(self.endpoint_lease_slot_by_index(live_idx));
                    if live_slot.is_live() {
                        required_route_frames = core::cmp::max(
                            required_route_frames,
                            live_slot.resident_budget.route_frame_slots as usize,
                        );
                    }
                }
                live_idx += 1;
            }
            if required_route_frames == 0 {
                self.free_external_persistent_sidecar(
                    self.route_storage.cast::<u8>(),
                    ResourceScope::RouteTable,
                )?;
                self.routes = RouteTable::empty();
                self.route_storage = Sidecar::EMPTY;
            }
        }
        let generation = slot.generation;
        *crate::invariant_some(self.endpoint_lease_slot_by_index_mut(idx)) = EndpointLeaseSlot {
            generation,
            ..EndpointLeaseSlot::EMPTY
        };
        self.recompute_frontier_workspace_bytes();
        Ok(())
    }
}
