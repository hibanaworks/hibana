use core::ops::{Deref, DerefMut};

use super::evidence::RouteArmState;
use super::evidence_store::ScopeEvidenceSlot;
#[cfg(test)]
use super::frontier::OfferEntrySlot;
use super::frontier::{
    ActiveEntrySlot, FrontierObservationSlot, LaneOfferState, RootFrontierState,
};
use super::frontier_state::FrontierState;
use super::inbox::{BindingInbox, PackedIncomingClassification};
use super::route_state::RouteState;
use crate::global::typestate::PhaseCursorState;

pub(super) struct LeasedState<T> {
    ptr: *mut T,
}

impl<T> LeasedState<T> {
    #[inline(always)]
    pub(super) unsafe fn init_from_ptr(dst: *mut Self, ptr: *mut T) {
        unsafe {
            core::ptr::addr_of_mut!((*dst).ptr).write(ptr);
        }
    }
}

impl<T> Deref for LeasedState<T> {
    type Target = T;

    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        debug_assert!(!self.ptr.is_null());
        unsafe { &*self.ptr }
    }
}

impl<T> DerefMut for LeasedState<T> {
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut Self::Target {
        debug_assert!(!self.ptr.is_null());
        unsafe { &mut *self.ptr }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct EndpointArenaSection {
    offset: usize,
    align: usize,
    bytes: usize,
    count: usize,
}

impl EndpointArenaSection {
    #[inline(always)]
    pub(crate) const fn offset(self) -> usize {
        self.offset
    }

    #[cfg(test)]
    #[inline(always)]
    pub(crate) const fn bytes(self) -> usize {
        self.bytes
    }

    #[inline(always)]
    pub(crate) const fn count(self) -> usize {
        self.count
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct EndpointArenaLayout {
    header_align: usize,
    phase_cursor_state: EndpointArenaSection,
    route_state: EndpointArenaSection,
    route_arm_stack: EndpointArenaSection,
    lane_offer_state_slots: EndpointArenaSection,
    frontier_state: EndpointArenaSection,
    frontier_root_rows: EndpointArenaSection,
    frontier_root_active_slots: EndpointArenaSection,
    frontier_root_observed_key_slots: EndpointArenaSection,
    #[cfg(test)]
    frontier_offer_entry_slots: EndpointArenaSection,
    binding_inbox: EndpointArenaSection,
    binding_slots: EndpointArenaSection,
    binding_len: EndpointArenaSection,
    binding_label_masks: EndpointArenaSection,
    scope_evidence_slots: EndpointArenaSection,
    total_bytes: usize,
    total_align: usize,
}

impl EndpointArenaLayout {
    #[inline(always)]
    pub(crate) const fn new(
        active_lane_count: usize,
        logical_lane_count: usize,
        max_route_stack_depth: usize,
        scope_evidence_count: usize,
        frontier_entry_capacity: usize,
    ) -> Self {
        #[cfg(test)]
        let offer_entry_capacity = max_usize(frontier_entry_capacity, TEST_FRONTIER_ENTRY_FLOOR);
        let mut offset = 0usize;
        let mut total_align = 1usize;

        let phase_cursor_state = Self::section::<PhaseCursorState>(offset);
        offset = phase_cursor_state.offset + phase_cursor_state.bytes;
        total_align = max_usize(total_align, phase_cursor_state.align);

        let route_state = Self::section::<RouteState>(offset);
        offset = route_state.offset + route_state.bytes;
        total_align = max_usize(total_align, route_state.align);

        let route_arm_stack =
            Self::section_array::<RouteArmState>(offset, active_lane_count * max_route_stack_depth);
        offset = route_arm_stack.offset + route_arm_stack.bytes;
        total_align = max_usize(total_align, route_arm_stack.align);

        let lane_offer_state_slots =
            Self::section_array::<LaneOfferState>(offset, active_lane_count);
        offset = lane_offer_state_slots.offset + lane_offer_state_slots.bytes;
        total_align = max_usize(total_align, lane_offer_state_slots.align);

        let frontier_state = Self::section::<FrontierState>(offset);
        offset = frontier_state.offset + frontier_state.bytes;
        total_align = max_usize(total_align, frontier_state.align);

        let frontier_root_rows =
            Self::section_array::<RootFrontierState>(offset, active_lane_count);
        offset = frontier_root_rows.offset + frontier_root_rows.bytes;
        total_align = max_usize(total_align, frontier_root_rows.align);

        let frontier_root_active_slots =
            Self::section_array::<ActiveEntrySlot>(offset, frontier_entry_capacity);
        offset = frontier_root_active_slots.offset + frontier_root_active_slots.bytes;
        total_align = max_usize(total_align, frontier_root_active_slots.align);

        let frontier_root_observed_key_slots =
            Self::section_array::<FrontierObservationSlot>(offset, frontier_entry_capacity);
        offset = frontier_root_observed_key_slots.offset + frontier_root_observed_key_slots.bytes;
        total_align = max_usize(total_align, frontier_root_observed_key_slots.align);

        #[cfg(test)]
        let frontier_offer_entry_slots = {
            EndpointArenaSection {
                offset,
                align: core::mem::align_of::<OfferEntrySlot>(),
                bytes: 0,
                count: offer_entry_capacity,
            }
        };

        let binding_inbox = Self::section::<BindingInbox>(offset);
        offset = binding_inbox.offset + binding_inbox.bytes;
        total_align = max_usize(total_align, binding_inbox.align);

        let binding_slots = Self::section_array::<PackedIncomingClassification>(
            offset,
            logical_lane_count * BindingInbox::PER_LANE_CAPACITY,
        );
        offset = binding_slots.offset + binding_slots.bytes;
        total_align = max_usize(total_align, binding_slots.align);

        let binding_len = Self::section_array::<u8>(offset, logical_lane_count);
        offset = binding_len.offset + binding_len.bytes;
        total_align = max_usize(total_align, binding_len.align);

        let binding_label_masks = Self::section_array::<u128>(offset, logical_lane_count);
        offset = binding_label_masks.offset + binding_label_masks.bytes;
        total_align = max_usize(total_align, binding_label_masks.align);

        let scope_evidence_slots =
            Self::section_array::<ScopeEvidenceSlot>(offset, scope_evidence_count);
        offset = scope_evidence_slots.offset + scope_evidence_slots.bytes;
        total_align = max_usize(total_align, scope_evidence_slots.align);

        Self {
            header_align: total_align,
            phase_cursor_state,
            route_state,
            route_arm_stack,
            lane_offer_state_slots,
            frontier_state,
            frontier_root_rows,
            frontier_root_active_slots,
            frontier_root_observed_key_slots,
            #[cfg(test)]
            frontier_offer_entry_slots,
            binding_inbox,
            binding_slots,
            binding_len,
            binding_label_masks,
            scope_evidence_slots,
            total_bytes: offset,
            total_align,
        }
    }

    #[inline(always)]
    pub(crate) const fn header_align(&self) -> usize {
        self.header_align
    }

    #[inline(always)]
    pub(crate) const fn route_state(&self) -> EndpointArenaSection {
        self.route_state
    }

    #[inline(always)]
    pub(crate) const fn route_arm_stack(&self) -> EndpointArenaSection {
        self.route_arm_stack
    }

    #[inline(always)]
    pub(crate) const fn lane_offer_state_slots(&self) -> EndpointArenaSection {
        self.lane_offer_state_slots
    }

    #[inline(always)]
    pub(crate) const fn phase_cursor_state(&self) -> EndpointArenaSection {
        self.phase_cursor_state
    }

    #[inline(always)]
    pub(crate) const fn frontier_state(&self) -> EndpointArenaSection {
        self.frontier_state
    }

    pub(crate) const fn frontier_root_rows(&self) -> EndpointArenaSection {
        self.frontier_root_rows
    }

    #[inline(always)]
    pub(crate) const fn frontier_root_active_slots(&self) -> EndpointArenaSection {
        self.frontier_root_active_slots
    }

    #[inline(always)]
    pub(crate) const fn frontier_root_observed_key_slots(&self) -> EndpointArenaSection {
        self.frontier_root_observed_key_slots
    }

    #[cfg(test)]
    #[inline(always)]
    pub(crate) const fn frontier_offer_entry_slots(&self) -> EndpointArenaSection {
        self.frontier_offer_entry_slots
    }

    #[inline(always)]
    pub(crate) const fn binding_inbox(&self) -> EndpointArenaSection {
        self.binding_inbox
    }

    #[inline(always)]
    pub(crate) const fn binding_slots(&self) -> EndpointArenaSection {
        self.binding_slots
    }

    #[inline(always)]
    pub(crate) const fn binding_len(&self) -> EndpointArenaSection {
        self.binding_len
    }

    #[inline(always)]
    pub(crate) const fn binding_label_masks(&self) -> EndpointArenaSection {
        self.binding_label_masks
    }

    #[inline(always)]
    pub(crate) const fn scope_evidence_slots(&self) -> EndpointArenaSection {
        self.scope_evidence_slots
    }

    #[inline(always)]
    pub(crate) const fn total_bytes(&self) -> usize {
        self.total_bytes
    }

    #[inline(always)]
    pub(crate) const fn total_align(&self) -> usize {
        self.total_align
    }

    #[inline(always)]
    const fn section<T>(offset: usize) -> EndpointArenaSection {
        let align = core::mem::align_of::<T>();
        let bytes = core::mem::size_of::<T>();
        EndpointArenaSection {
            offset: align_up(offset, align),
            align,
            bytes,
            count: 1,
        }
    }

    #[inline(always)]
    const fn section_array<T>(offset: usize, count: usize) -> EndpointArenaSection {
        let align = core::mem::align_of::<T>();
        let bytes = core::mem::size_of::<T>().saturating_mul(count);
        EndpointArenaSection {
            offset: align_up(offset, align),
            align,
            bytes,
            count,
        }
    }
}

#[inline(always)]
const fn max_usize(lhs: usize, rhs: usize) -> usize {
    if lhs > rhs { lhs } else { rhs }
}

#[cfg(test)]
const TEST_FRONTIER_ENTRY_FLOOR: usize = 8;

#[inline(always)]
const fn align_up(value: usize, align: usize) -> usize {
    let mask = align.saturating_sub(1);
    (value + mask) & !mask
}

#[cfg(test)]
mod tests {
    use super::EndpointArenaLayout;

    #[test]
    fn root_frontier_shared_pools_track_max_frontier_entries() {
        let layout = EndpointArenaLayout::new(3, 3, 2, 4, 5);
        assert_eq!(layout.frontier_root_rows().count(), 3);
        assert_eq!(layout.frontier_root_active_slots().count(), 5);
        assert_eq!(layout.frontier_root_observed_key_slots().count(), 5);
        assert_eq!(layout.frontier_offer_entry_slots().count(), 8);
    }
}
