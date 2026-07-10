use super::{
    EndpointLeaseId, EndpointLeaseSlot, EndpointLeaseState, EndpointResidentBudget, Rendezvous,
    Transport,
};
use crate::{session::cluster::error::ResourceScope, session::types::SessionId};
impl<'rv, 'cfg, T: Transport> Rendezvous<'rv, 'cfg, T>
where
    'cfg: 'rv,
{
    #[inline]
    pub(crate) fn allocate_endpoint_lease(
        &self,
        sid: SessionId,
        role: u8,
        bytes: usize,
        align: usize,
        resident_budget: EndpointResidentBudget,
    ) -> Result<(EndpointLeaseId, u32, usize, usize), ResourceScope> {
        if bytes > u32::MAX as usize {
            return Err(ResourceScope::EndpointLease);
        }
        let mut slot_count = self.endpoint_lease_slot_count();
        let mut has_empty_slot = false;
        let mut slot_idx = 0usize;
        while slot_idx < slot_count {
            let slot = crate::invariant_some(self.endpoint_lease_slot_by_index(slot_idx));
            if !slot.is_occupied() {
                self.require_empty_endpoint_waiter_slot(slot_idx);
                has_empty_slot = true;
                break;
            }
            slot_idx += 1;
        }
        if !has_empty_slot {
            let required_slots = slot_count
                .checked_add(1)
                .ok_or(ResourceScope::EndpointLease)?;
            self.ensure_endpoint_lease_capacity(required_slots)?;
            slot_count = required_slots;
        }
        let (slab_ptr, slab_len) = self.slab_ptr_and_len();
        let slab_base = slab_ptr as usize;
        let slab_end = slab_base
            .checked_add(slab_len)
            .ok_or(ResourceScope::EndpointLease)?;
        let lease_storage = self.endpoint_lease_storage.get();
        let lease_base = lease_storage.ptr() as usize;
        let lease_bytes = lease_storage.bytes();
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
            while idx < slot_count {
                let slot = crate::invariant_some(self.endpoint_lease_slot_by_index(idx));
                let offset = slot.offset as usize;
                if slot.is_occupied() && offset < candidate_end && offset >= best_offset {
                    best_offset = offset;
                    best_idx = Some(idx);
                }
                idx += 1;
            }
            let gap_start = match best_idx {
                Some(idx) => {
                    let slot = crate::invariant_some(self.endpoint_lease_slot_by_index(idx));
                    crate::invariant_some((slot.offset as usize).checked_add(slot.len as usize))
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
                    while insert_idx < slot_count {
                        let slot =
                            crate::invariant_some(self.endpoint_lease_slot_by_index(insert_idx));
                        if !slot.is_occupied() {
                            self.require_empty_endpoint_waiter_slot(insert_idx);
                            let generation = self.next_endpoint_lease_generation();
                            let lease_id =
                                crate::invariant_ok(EndpointLeaseId::try_from(insert_idx));
                            self.write_endpoint_lease_slot(
                                insert_idx,
                                EndpointLeaseSlot {
                                    generation,
                                    sid,
                                    role,
                                    offset: lease_offset,
                                    len: lease_len,
                                    resident_budget,
                                    state: EndpointLeaseState::Reserved,
                                },
                            );
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
    pub(crate) fn has_endpoint_session_role(&self, sid: SessionId, role: u8) -> bool {
        let slot_count = self.endpoint_lease_slot_count();
        let mut idx = 0usize;
        while idx < slot_count {
            let slot = crate::invariant_some(self.endpoint_lease_slot_by_index(idx));
            if slot.is_occupied() && slot.sid == sid && slot.role == role {
                return true;
            }
            idx += 1;
        }
        false
    }

    #[inline]
    pub(crate) fn publish_endpoint_lease(&self, lease_slot: EndpointLeaseId, generation: u32) {
        let idx = usize::from(lease_slot);
        let slot = crate::invariant_some(self.endpoint_lease_slot_by_index(idx));
        if slot.generation != generation || slot.state != EndpointLeaseState::Reserved {
            crate::invariant();
        }
        self.write_endpoint_lease_slot(
            idx,
            EndpointLeaseSlot {
                state: EndpointLeaseState::Published,
                ..slot
            },
        );
    }

    #[inline]
    pub(crate) fn release_endpoint_lease(&self, lease_slot: EndpointLeaseId, generation: u32) {
        let idx = usize::from(lease_slot);
        let slot_count = self.endpoint_lease_slot_count();
        if idx >= slot_count {
            crate::invariant();
        }
        let slot = crate::invariant_some(self.endpoint_lease_slot_by_index(idx));
        if !slot.is_occupied() || slot.generation != generation {
            crate::invariant();
        }
        self.require_empty_endpoint_waiter_slot(idx);
        self.write_endpoint_lease_slot(
            idx,
            EndpointLeaseSlot {
                generation: slot.generation,
                ..EndpointLeaseSlot::EMPTY
            },
        );
        let required_route_frames = self.resident_route_frame_slots_floor();
        let required_route_lanes = self.resident_route_lane_slots_floor();
        if self.routes.route_slots() == 0 {
            if required_route_frames != 0 {
                crate::invariant();
            }
        } else {
            self.shrink_route_table_capacity(required_route_frames, required_route_lanes);
        }
        self.shrink_assoc_table_capacity(self.active_lane_attachment_count());
        self.shrink_lane_range(self.assoc.active_lane_slots().max(required_route_lanes));
        self.shrink_endpoint_lease_capacity();
        self.recompute_frontier_workspace_bytes();
    }
}
