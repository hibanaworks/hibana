use super::{
    EndpointLeaseId, EndpointLeaseSlot, EndpointLeaseState, EndpointResidentBudget, Rendezvous,
    Transport,
};
use crate::{session::cluster::error::ResourceScope, session::types::SessionId};
mod placement;
mod session_binding;
#[cfg(kani)]
pub(in crate::rendezvous::core) use placement::endpoint_offset_in_gap;

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
        if bytes > u32::MAX as usize || self.endpoint_lease_generation.get() == u32::MAX {
            return Err(ResourceScope::EndpointLease);
        }
        let slot_count = self.endpoint_lease_slot_count();
        let mut slot_idx = 0usize;
        while slot_idx < slot_count {
            let slot = crate::invariant_some(self.endpoint_lease_slot_by_index(slot_idx));
            if !slot.is_occupied() {
                self.require_empty_endpoint_waiter_slot(slot_idx);
                break;
            }
            slot_idx += 1;
        }
        let required_slots = if slot_idx == slot_count {
            slot_count
                .checked_add(1)
                .ok_or(ResourceScope::EndpointLease)?
        } else {
            slot_count
        };
        let capacity_plan = self.plan_endpoint_lease_capacity(required_slots)?;
        let endpoint_floor = self.endpoint_lease_floor_after_capacity_plan(&capacity_plan)?;
        let (slab_ptr, slab_len) = self.slab_ptr_and_len();
        let slab_base = slab_ptr as usize;
        let slab_end = crate::invariant_some(slab_base.checked_add(slab_len));
        let lease_storage = self.endpoint_lease_storage.get();
        if !lease_storage.is_empty() {
            let lease_base = lease_storage.ptr() as usize;
            let lease_end = crate::invariant_some(lease_base.checked_add(lease_storage.bytes()));
            if lease_base < slab_base || lease_end > slab_end {
                crate::invariant();
            }
        }
        let offset = self.plan_endpoint_lease_offset(bytes, align, endpoint_floor)?;
        let lease_len = u32::try_from(bytes).map_err(|_| ResourceScope::EndpointLease)?;
        let lease_offset = u32::try_from(offset).map_err(|_| ResourceScope::EndpointLease)?;
        let lease_id =
            EndpointLeaseId::try_from(slot_idx).map_err(|_| ResourceScope::EndpointLease)?;

        self.commit_endpoint_lease_capacity(capacity_plan);
        let slot = crate::invariant_some(self.endpoint_lease_slot_by_index(slot_idx));
        if slot.is_occupied() {
            crate::invariant();
        }
        self.require_empty_endpoint_waiter_slot(slot_idx);
        let generation = crate::invariant_some(self.next_endpoint_lease_generation());
        self.write_endpoint_lease_slot(
            slot_idx,
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
        Ok((lease_id, generation, offset, bytes))
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

    #[inline]
    pub(crate) fn abort_endpoint_lease_reservation(
        &self,
        lease_slot: EndpointLeaseId,
        generation: u32,
    ) {
        let slot =
            crate::invariant_some(self.endpoint_lease_slot_by_index(usize::from(lease_slot)));
        if slot.state != EndpointLeaseState::Reserved
            || slot.generation != generation
            || self.endpoint_lease_generation.get() != generation
        {
            crate::invariant();
        }
        let previous_generation = crate::invariant_some(generation.checked_sub(1));
        self.release_endpoint_lease(lease_slot, generation);
        self.endpoint_lease_generation.set(previous_generation);
    }
}
