//! Caller-owned storage for the visited positions of one offer search.
use super::frontier::FrontierVisitSet;

pub(super) struct FrontierStateStorage {
    pub(super) visited_position_bits: *mut u8,
}
pub(super) struct FrontierStateCapacity {
    pub(super) visited_position_count: usize,
}

pub(super) struct FrontierState {
    visited_position_bits: *mut u8,
    visited_position_count: usize,
}

impl FrontierState {
    pub(super) unsafe fn init_empty(
        dst: *mut Self,
        storage: FrontierStateStorage,
        capacity: FrontierStateCapacity,
    ) {
        if capacity.visited_position_count > u16::MAX as usize + 1
            || (capacity.visited_position_count != 0 && storage.visited_position_bits.is_null())
        {
            crate::invariant();
        }
        unsafe {
            // SAFETY: initialization owns this unpublished arena section.
            core::ptr::addr_of_mut!((*dst).visited_position_bits)
                .write(storage.visited_position_bits);
            core::ptr::addr_of_mut!((*dst).visited_position_count)
                .write(capacity.visited_position_count);
        }
    }

    #[inline]
    pub(super) fn empty_frontier_visit_set(&mut self) -> FrontierVisitSet {
        /* SAFETY: `visited_position_bits` is the endpoint-owned arena section with
        one bit for every cursor position that one route-bearing endpoint can
        visit. One public offer operation owns the returned bitmap until
        completion. */
        unsafe {
            FrontierVisitSet::from_parts(self.visited_position_bits, self.visited_position_count)
        }
    }
}
