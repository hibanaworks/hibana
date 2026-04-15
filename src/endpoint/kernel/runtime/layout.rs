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
use super::route_state::{RouteScopeSelectedArmSlot, RouteState};
use crate::global::role_program::{LaneWord, RoleFootprint, lane_word_count};
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
pub(crate) struct RouteFrontierArenaLayout {
    route_arm_stack: EndpointArenaSection,
    lane_offer_state_slots: EndpointArenaSection,
    frontier_state: EndpointArenaSection,
    frontier_root_rows: EndpointArenaSection,
    frontier_root_active_slots: EndpointArenaSection,
    frontier_root_observed_key_slots: EndpointArenaSection,
    frontier_root_observed_offer_lanes: EndpointArenaSection,
    frontier_root_observed_binding_nonempty_lanes: EndpointArenaSection,
    #[cfg(test)]
    frontier_offer_entry_slots: EndpointArenaSection,
    scope_evidence_slots: EndpointArenaSection,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct EndpointArenaLayout {
    header_align: usize,
    phase_cursor_state: EndpointArenaSection,
    phase_cursor_lane_cursors: EndpointArenaSection,
    phase_cursor_current_step_labels: EndpointArenaSection,
    route_state: EndpointArenaSection,
    route_state_lane_dense_by_lane: EndpointArenaSection,
    route_state_lane_route_arm_lens: EndpointArenaSection,
    route_state_lane_linger_counts: EndpointArenaSection,
    route_state_scope_selected_arms: EndpointArenaSection,
    route_state_active_route_lanes: EndpointArenaSection,
    route_state_lane_linger_lanes: EndpointArenaSection,
    route_state_lane_offer_linger_lanes: EndpointArenaSection,
    route_state_active_offer_lanes: EndpointArenaSection,
    frontier: RouteFrontierArenaLayout,
    binding_inbox: EndpointArenaSection,
    binding_lane_dense_by_lane: EndpointArenaSection,
    binding_slots: EndpointArenaSection,
    binding_len: EndpointArenaSection,
    binding_label_masks: EndpointArenaSection,
    binding_nonempty_lanes: EndpointArenaSection,
    total_bytes: usize,
    total_align: usize,
}

impl EndpointArenaLayout {
    #[inline(always)]
    pub(crate) const fn from_footprint(footprint: RoleFootprint) -> Self {
        Self::from_footprint_with_binding(footprint, true)
    }

    #[inline(always)]
    pub(crate) const fn from_footprint_with_binding(
        footprint: RoleFootprint,
        binding_enabled: bool,
    ) -> Self {
        #[cfg(test)]
        let offer_entry_capacity =
            max_usize(footprint.frontier_entry_count, TEST_FRONTIER_ENTRY_FLOOR);
        let mut offset = 0usize;
        let mut total_align = 1usize;

        let phase_cursor_state = Self::section::<PhaseCursorState>(offset);
        offset = phase_cursor_state.offset + phase_cursor_state.bytes;
        total_align = max_usize(total_align, phase_cursor_state.align);

        let phase_cursor_lane_cursors =
            Self::section_array::<u16>(offset, footprint.logical_lane_count);
        offset = phase_cursor_lane_cursors.offset + phase_cursor_lane_cursors.bytes;
        total_align = max_usize(total_align, phase_cursor_lane_cursors.align);

        let phase_cursor_current_step_labels =
            Self::section_array::<u8>(offset, footprint.logical_lane_count);
        offset = phase_cursor_current_step_labels.offset + phase_cursor_current_step_labels.bytes;
        total_align = max_usize(total_align, phase_cursor_current_step_labels.align);

        let route_state = Self::section::<RouteState>(offset);
        offset = route_state.offset + route_state.bytes;
        total_align = max_usize(total_align, route_state.align);

        let route_state_lane_dense_by_lane =
            Self::section_array::<u8>(offset, footprint.logical_lane_count);
        offset = route_state_lane_dense_by_lane.offset + route_state_lane_dense_by_lane.bytes;
        total_align = max_usize(total_align, route_state_lane_dense_by_lane.align);

        let route_state_lane_route_arm_lens =
            Self::section_array::<u8>(offset, footprint.active_lane_count);
        offset = route_state_lane_route_arm_lens.offset + route_state_lane_route_arm_lens.bytes;
        total_align = max_usize(total_align, route_state_lane_route_arm_lens.align);

        let route_state_lane_linger_counts =
            Self::section_array::<u8>(offset, footprint.active_lane_count);
        offset = route_state_lane_linger_counts.offset + route_state_lane_linger_counts.bytes;
        total_align = max_usize(total_align, route_state_lane_linger_counts.align);

        let route_state_scope_selected_arms = Self::section_array::<RouteScopeSelectedArmSlot>(
            offset,
            footprint.scope_evidence_count,
        );
        offset = route_state_scope_selected_arms.offset + route_state_scope_selected_arms.bytes;
        total_align = max_usize(total_align, route_state_scope_selected_arms.align);

        let route_state_lane_word_count = footprint.logical_lane_word_count;
        let route_state_active_route_lanes =
            Self::section_array::<LaneWord>(offset, route_state_lane_word_count);
        offset = route_state_active_route_lanes.offset + route_state_active_route_lanes.bytes;
        total_align = max_usize(total_align, route_state_active_route_lanes.align);

        let route_state_lane_linger_lanes =
            Self::section_array::<LaneWord>(offset, route_state_lane_word_count);
        offset = route_state_lane_linger_lanes.offset + route_state_lane_linger_lanes.bytes;
        total_align = max_usize(total_align, route_state_lane_linger_lanes.align);

        let route_state_lane_offer_linger_lanes =
            Self::section_array::<LaneWord>(offset, route_state_lane_word_count);
        offset =
            route_state_lane_offer_linger_lanes.offset + route_state_lane_offer_linger_lanes.bytes;
        total_align = max_usize(total_align, route_state_lane_offer_linger_lanes.align);

        let route_state_active_offer_lanes =
            Self::section_array::<LaneWord>(offset, route_state_lane_word_count);
        offset = route_state_active_offer_lanes.offset + route_state_active_offer_lanes.bytes;
        total_align = max_usize(total_align, route_state_active_offer_lanes.align);

        let route_arm_stack = Self::section_array::<RouteArmState>(
            offset,
            footprint
                .active_lane_count
                .saturating_mul(footprint.max_route_stack_depth),
        );
        offset = route_arm_stack.offset + route_arm_stack.bytes;
        total_align = max_usize(total_align, route_arm_stack.align);

        let lane_offer_state_slots =
            Self::section_array::<LaneOfferState>(offset, footprint.active_lane_count);
        offset = lane_offer_state_slots.offset + lane_offer_state_slots.bytes;
        total_align = max_usize(total_align, lane_offer_state_slots.align);

        let frontier_state = Self::section::<FrontierState>(offset);
        offset = frontier_state.offset + frontier_state.bytes;
        total_align = max_usize(total_align, frontier_state.align);

        let frontier_root_rows =
            Self::section_array::<RootFrontierState>(offset, footprint.active_lane_count);
        offset = frontier_root_rows.offset + frontier_root_rows.bytes;
        total_align = max_usize(total_align, frontier_root_rows.align);

        let frontier_root_active_slots =
            Self::section_array::<ActiveEntrySlot>(offset, footprint.frontier_entry_count);
        offset = frontier_root_active_slots.offset + frontier_root_active_slots.bytes;
        total_align = max_usize(total_align, frontier_root_active_slots.align);

        let frontier_root_observed_key_slots =
            Self::section_array::<FrontierObservationSlot>(offset, footprint.frontier_entry_count);
        offset = frontier_root_observed_key_slots.offset + frontier_root_observed_key_slots.bytes;
        total_align = max_usize(total_align, frontier_root_observed_key_slots.align);

        let frontier_root_observed_offer_lanes = Self::section_array::<LaneWord>(
            offset,
            footprint
                .active_lane_count
                .saturating_mul(footprint.logical_lane_word_count),
        );
        offset =
            frontier_root_observed_offer_lanes.offset + frontier_root_observed_offer_lanes.bytes;
        total_align = max_usize(total_align, frontier_root_observed_offer_lanes.align);

        let frontier_root_observed_binding_nonempty_lanes = Self::section_array::<LaneWord>(
            offset,
            footprint
                .active_lane_count
                .saturating_mul(footprint.logical_lane_word_count),
        );
        offset = frontier_root_observed_binding_nonempty_lanes.offset
            + frontier_root_observed_binding_nonempty_lanes.bytes;
        total_align = max_usize(
            total_align,
            frontier_root_observed_binding_nonempty_lanes.align,
        );

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

        let binding_lane_count = if binding_enabled {
            footprint.logical_lane_count
        } else {
            0
        };
        let binding_lane_dense_by_lane = Self::section_array::<u8>(offset, binding_lane_count);
        offset = binding_lane_dense_by_lane.offset + binding_lane_dense_by_lane.bytes;
        total_align = max_usize(total_align, binding_lane_dense_by_lane.align);

        let binding_slots = Self::section_array::<PackedIncomingClassification>(
            offset,
            binding_lane_count * BindingInbox::PER_LANE_CAPACITY,
        );
        offset = binding_slots.offset + binding_slots.bytes;
        total_align = max_usize(total_align, binding_slots.align);

        let binding_len = Self::section_array::<u8>(offset, binding_lane_count);
        offset = binding_len.offset + binding_len.bytes;
        total_align = max_usize(total_align, binding_len.align);

        let binding_label_masks = Self::section_array::<u128>(offset, binding_lane_count);
        offset = binding_label_masks.offset + binding_label_masks.bytes;
        total_align = max_usize(total_align, binding_label_masks.align);

        let binding_nonempty_lanes =
            Self::section_array::<LaneWord>(offset, lane_word_count(binding_lane_count));
        offset = binding_nonempty_lanes.offset + binding_nonempty_lanes.bytes;
        total_align = max_usize(total_align, binding_nonempty_lanes.align);

        let scope_evidence_slots =
            Self::section_array::<ScopeEvidenceSlot>(offset, footprint.scope_evidence_count);
        offset = scope_evidence_slots.offset + scope_evidence_slots.bytes;
        total_align = max_usize(total_align, scope_evidence_slots.align);

        Self {
            header_align: total_align,
            phase_cursor_state,
            phase_cursor_lane_cursors,
            phase_cursor_current_step_labels,
            route_state,
            route_state_lane_dense_by_lane,
            route_state_lane_route_arm_lens,
            route_state_lane_linger_counts,
            route_state_scope_selected_arms,
            route_state_active_route_lanes,
            route_state_lane_linger_lanes,
            route_state_lane_offer_linger_lanes,
            route_state_active_offer_lanes,
            frontier: RouteFrontierArenaLayout {
                route_arm_stack,
                lane_offer_state_slots,
                frontier_state,
                frontier_root_rows,
                frontier_root_active_slots,
                frontier_root_observed_key_slots,
                frontier_root_observed_offer_lanes,
                frontier_root_observed_binding_nonempty_lanes,
                #[cfg(test)]
                frontier_offer_entry_slots,
                scope_evidence_slots,
            },
            binding_inbox,
            binding_lane_dense_by_lane,
            binding_slots,
            binding_len,
            binding_label_masks,
            binding_nonempty_lanes,
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
        self.frontier.route_arm_stack
    }

    #[inline(always)]
    pub(crate) const fn lane_offer_state_slots(&self) -> EndpointArenaSection {
        self.frontier.lane_offer_state_slots
    }

    #[inline(always)]
    pub(crate) const fn phase_cursor_state(&self) -> EndpointArenaSection {
        self.phase_cursor_state
    }

    #[inline(always)]
    pub(crate) const fn phase_cursor_lane_cursors(&self) -> EndpointArenaSection {
        self.phase_cursor_lane_cursors
    }

    #[inline(always)]
    pub(crate) const fn phase_cursor_current_step_labels(&self) -> EndpointArenaSection {
        self.phase_cursor_current_step_labels
    }

    #[inline(always)]
    pub(crate) const fn frontier_state(&self) -> EndpointArenaSection {
        self.frontier.frontier_state
    }

    #[inline(always)]
    pub(crate) const fn route_state_lane_dense_by_lane(&self) -> EndpointArenaSection {
        self.route_state_lane_dense_by_lane
    }

    #[inline(always)]
    pub(crate) const fn route_state_lane_route_arm_lens(&self) -> EndpointArenaSection {
        self.route_state_lane_route_arm_lens
    }

    #[inline(always)]
    pub(crate) const fn route_state_lane_linger_counts(&self) -> EndpointArenaSection {
        self.route_state_lane_linger_counts
    }

    #[inline(always)]
    pub(crate) const fn route_state_scope_selected_arms(&self) -> EndpointArenaSection {
        self.route_state_scope_selected_arms
    }

    #[inline(always)]
    pub(crate) const fn route_state_active_route_lanes(&self) -> EndpointArenaSection {
        self.route_state_active_route_lanes
    }

    #[inline(always)]
    pub(crate) const fn route_state_lane_linger_lanes(&self) -> EndpointArenaSection {
        self.route_state_lane_linger_lanes
    }

    #[inline(always)]
    pub(crate) const fn route_state_lane_offer_linger_lanes(&self) -> EndpointArenaSection {
        self.route_state_lane_offer_linger_lanes
    }

    #[inline(always)]
    pub(crate) const fn route_state_active_offer_lanes(&self) -> EndpointArenaSection {
        self.route_state_active_offer_lanes
    }

    pub(crate) const fn frontier_root_rows(&self) -> EndpointArenaSection {
        self.frontier.frontier_root_rows
    }

    #[inline(always)]
    pub(crate) const fn frontier_root_active_slots(&self) -> EndpointArenaSection {
        self.frontier.frontier_root_active_slots
    }

    #[inline(always)]
    pub(crate) const fn frontier_root_observed_key_slots(&self) -> EndpointArenaSection {
        self.frontier.frontier_root_observed_key_slots
    }

    #[cfg(test)]
    #[inline(always)]
    pub(crate) const fn frontier_offer_entry_slots(&self) -> EndpointArenaSection {
        self.frontier.frontier_offer_entry_slots
    }

    #[inline(always)]
    pub(crate) const fn frontier_root_observed_offer_lanes(&self) -> EndpointArenaSection {
        self.frontier.frontier_root_observed_offer_lanes
    }

    #[inline(always)]
    pub(crate) const fn frontier_root_observed_binding_nonempty_lanes(
        &self,
    ) -> EndpointArenaSection {
        self.frontier.frontier_root_observed_binding_nonempty_lanes
    }

    #[inline(always)]
    pub(crate) const fn binding_inbox(&self) -> EndpointArenaSection {
        self.binding_inbox
    }

    #[inline(always)]
    pub(crate) const fn binding_lane_dense_by_lane(&self) -> EndpointArenaSection {
        self.binding_lane_dense_by_lane
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
    pub(crate) const fn binding_nonempty_lanes(&self) -> EndpointArenaSection {
        self.binding_nonempty_lanes
    }

    #[inline(always)]
    pub(crate) const fn scope_evidence_slots(&self) -> EndpointArenaSection {
        self.frontier.scope_evidence_slots
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
    use crate::global::role_program::RoleFootprint;

    #[test]
    fn root_frontier_shared_pools_track_max_frontier_entries() {
        let footprint = RoleFootprint::for_endpoint_layout(3, 65, 65, 2, 4, 5);
        let layout = EndpointArenaLayout::from_footprint(footprint);
        assert_eq!(layout.frontier_root_rows().count(), 3);
        assert_eq!(layout.frontier_root_active_slots().count(), 5);
        assert_eq!(layout.frontier_root_observed_key_slots().count(), 5);
        assert_eq!(
            layout.frontier_root_observed_offer_lanes().count(),
            footprint
                .active_lane_count
                .saturating_mul(footprint.logical_lane_word_count)
        );
        assert_eq!(
            layout
                .frontier_root_observed_binding_nonempty_lanes()
                .count(),
            footprint
                .active_lane_count
                .saturating_mul(footprint.logical_lane_word_count)
        );
        assert_eq!(layout.frontier_offer_entry_slots().count(), 8);
    }
}
