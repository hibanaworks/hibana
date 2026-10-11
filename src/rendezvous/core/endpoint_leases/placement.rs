use super::{Rendezvous, Transport};
use crate::session::cluster::error::ResourceScope;

pub(in crate::rendezvous::core) fn endpoint_offset_in_gap(
    base: usize,
    gap_start: usize,
    gap_end: usize,
    bytes: usize,
    align: usize,
) -> Option<usize> {
    if !align.is_power_of_two() {
        crate::invariant();
    }
    let unaligned = base.checked_add(gap_end.checked_sub(bytes)?)?;
    let offset = (unaligned & !(align - 1)).checked_sub(base)?;
    if offset < gap_start {
        return None;
    }
    Some(offset)
}

impl<'rv, 'cfg, T: Transport> Rendezvous<'rv, 'cfg, T>
where
    'cfg: 'rv,
{
    pub(super) fn plan_endpoint_lease_offset(
        &self,
        bytes: usize,
        align: usize,
        floor: usize,
    ) -> Result<usize, ResourceScope> {
        let (slab_ptr, slab_len) = self.slab_ptr_and_len();
        let base = slab_ptr as usize;
        let slot_count = self.endpoint_lease_slot_count();
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
            if let Some(offset) =
                endpoint_offset_in_gap(base, gap_start, candidate_end, bytes, align)
                && offset >= floor
            {
                return Ok(offset);
            }
            let Some(idx) = best_idx else {
                return Err(ResourceScope::EndpointLease);
            };
            candidate_end =
                crate::invariant_some(self.endpoint_lease_slot_by_index(idx)).offset as usize;
        }
    }
}
