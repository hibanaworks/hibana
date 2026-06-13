use core::ops::{Deref, DerefMut};

use super::decision_state::{RouteCommitRowSetBuilder, RouteScopeSelectedArmSlot, RouteState};
use super::evidence::RouteArmState;
use super::evidence_store::ScopeEvidenceSlot;
use super::frontier::OfferEntrySlot;
use super::frontier::{
    ActiveEntrySlot, FrontierObservationSlot, LaneOfferState, RootFrontierState,
};
use super::frontier_state::FrontierState;
use crate::global::role_program::{DenseLaneOrdinal, LaneWord, RuntimeRoleFootprint};
use crate::global::typestate::EventCursorState;

pub(super) struct LeasedState<T> {
    ptr: *mut T,
}

impl<T> LeasedState<T> {
    #[inline(always)]
    pub(super) unsafe fn init_from_ptr(dst: *mut Self, ptr: *mut T) {
        /* SAFETY: initialization owns exclusive writable storage for this field and writes it exactly once before exposure. */
        unsafe {
            core::ptr::addr_of_mut!((*dst).ptr).write(ptr);
        }
    }
}

impl<T> Deref for LeasedState<T> {
    type Target = T;

    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        if self.ptr.is_null() {
            crate::invariant();
        }
        /* SAFETY: the pointer comes from pinned owner storage and this path only creates a shared borrow. */
        unsafe { &*self.ptr }
    }
}

impl<T> DerefMut for LeasedState<T> {
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut Self::Target {
        if self.ptr.is_null() {
            crate::invariant();
        }
        /* SAFETY: the pointer comes from pinned owner storage and this path holds unique mutable access for the borrow. */
        unsafe { &mut *self.ptr }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct EndpointArenaSection {
    offset: usize,
    align: usize,
    pub(crate) bytes: usize,
    count: usize,
}

impl EndpointArenaSection {
    #[inline(always)]
    pub(crate) const fn offset(self) -> usize {
        self.offset
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
    frontier_offer_entry_slots: EndpointArenaSection,
    scope_evidence_slots: EndpointArenaSection,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct EndpointArenaLayout {
    header_align: usize,
    event_cursor_state: EndpointArenaSection,
    event_cursor_lane_cursors: EndpointArenaSection,
    event_cursor_current_step_label_codes: EndpointArenaSection,
    event_cursor_completed_event_words: EndpointArenaSection,
    decision_state: EndpointArenaSection,
    route_state_lane_dense_by_lane: EndpointArenaSection,
    route_state_lane_route_arm_lens: EndpointArenaSection,
    route_state_lane_linger_counts: EndpointArenaSection,
    route_state_scope_selected_arms: EndpointArenaSection,
    route_state_lane_linger_lanes: EndpointArenaSection,
    route_state_lane_offer_linger_lanes: EndpointArenaSection,
    route_state_active_offer_lanes: EndpointArenaSection,
    route_commit_row_set_builder: EndpointArenaSection,
    frontier: RouteFrontierArenaLayout,
    total_bytes: usize,
    total_align: usize,
}

impl EndpointArenaLayout {
    #[inline(always)]
    pub(crate) const fn from_footprint(footprint: RuntimeRoleFootprint) -> Self {
        let mut offset = 0usize;
        let mut total_align = 1usize;

        let event_cursor_state = Self::section::<EventCursorState>(offset);
        offset = event_cursor_state.offset + event_cursor_state.bytes;
        total_align = max_usize(total_align, event_cursor_state.align);

        let event_cursor_lane_cursors =
            Self::section_array::<u16>(offset, footprint.logical_lane_count);
        offset = event_cursor_lane_cursors.offset + event_cursor_lane_cursors.bytes;
        total_align = max_usize(total_align, event_cursor_lane_cursors.align);

        let event_cursor_current_step_label_codes =
            Self::section_array::<u16>(offset, footprint.logical_lane_count);
        offset = event_cursor_current_step_label_codes.offset
            + event_cursor_current_step_label_codes.bytes;
        total_align = max_usize(total_align, event_cursor_current_step_label_codes.align);

        let event_cursor_completed_event_words =
            Self::section_array::<u32>(offset, bit_word_count(footprint.local_step_count));
        offset =
            event_cursor_completed_event_words.offset + event_cursor_completed_event_words.bytes;
        total_align = max_usize(total_align, event_cursor_completed_event_words.align);

        let decision_state = Self::section::<RouteState>(offset);
        offset = decision_state.offset + decision_state.bytes;
        total_align = max_usize(total_align, decision_state.align);

        let route_state_lane_dense_by_lane =
            Self::section_array::<DenseLaneOrdinal>(offset, footprint.logical_lane_count);
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
            footprint.scope_evidence_count(),
        );
        offset = route_state_scope_selected_arms.offset + route_state_scope_selected_arms.bytes;
        total_align = max_usize(total_align, route_state_scope_selected_arms.align);

        let route_state_lane_word_count = footprint.lane_word_count();
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

        let route_commit_row_set_builder = Self::section::<RouteCommitRowSetBuilder>(offset);
        offset = route_commit_row_set_builder.offset + route_commit_row_set_builder.bytes;
        total_align = max_usize(total_align, route_commit_row_set_builder.align);

        let route_arm_stack = Self::section_array::<RouteArmState>(
            offset,
            checked_usize_mul(footprint.active_lane_count, footprint.max_route_stack_depth),
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
            Self::section_array::<ActiveEntrySlot>(offset, footprint.frontier_entry_count());
        offset = frontier_root_active_slots.offset + frontier_root_active_slots.bytes;
        total_align = max_usize(total_align, frontier_root_active_slots.align);

        let frontier_root_observed_key_slots = Self::section_array::<FrontierObservationSlot>(
            offset,
            footprint.frontier_entry_count(),
        );
        offset = frontier_root_observed_key_slots.offset + frontier_root_observed_key_slots.bytes;
        total_align = max_usize(total_align, frontier_root_observed_key_slots.align);

        let frontier_root_observed_offer_lanes = Self::section_array::<LaneWord>(
            offset,
            checked_usize_mul(footprint.active_lane_count, footprint.lane_word_count()),
        );
        offset =
            frontier_root_observed_offer_lanes.offset + frontier_root_observed_offer_lanes.bytes;
        total_align = max_usize(total_align, frontier_root_observed_offer_lanes.align);

        let frontier_offer_entry_slots =
            Self::section_array::<OfferEntrySlot>(offset, footprint.frontier_entry_count());
        offset = frontier_offer_entry_slots.offset + frontier_offer_entry_slots.bytes;
        total_align = max_usize(total_align, frontier_offer_entry_slots.align);

        let scope_evidence_slots =
            Self::section_array::<ScopeEvidenceSlot>(offset, footprint.scope_evidence_count());
        offset = scope_evidence_slots.offset + scope_evidence_slots.bytes;
        total_align = max_usize(total_align, scope_evidence_slots.align);

        Self {
            header_align: total_align,
            event_cursor_state,
            event_cursor_lane_cursors,
            event_cursor_current_step_label_codes,
            event_cursor_completed_event_words,
            decision_state,
            route_state_lane_dense_by_lane,
            route_state_lane_route_arm_lens,
            route_state_lane_linger_counts,
            route_state_scope_selected_arms,
            route_state_lane_linger_lanes,
            route_state_lane_offer_linger_lanes,
            route_state_active_offer_lanes,
            route_commit_row_set_builder,
            frontier: RouteFrontierArenaLayout {
                route_arm_stack,
                lane_offer_state_slots,
                frontier_state,
                frontier_root_rows,
                frontier_root_active_slots,
                frontier_root_observed_key_slots,
                frontier_root_observed_offer_lanes,
                frontier_offer_entry_slots,
                scope_evidence_slots,
            },
            total_bytes: offset,
            total_align,
        }
    }

    #[inline(always)]
    pub(crate) const fn header_align(&self) -> usize {
        self.header_align
    }

    #[inline(always)]
    pub(crate) const fn decision_state(&self) -> EndpointArenaSection {
        self.decision_state
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
    pub(crate) const fn event_cursor_state(&self) -> EndpointArenaSection {
        self.event_cursor_state
    }

    #[inline(always)]
    pub(crate) const fn event_cursor_lane_cursors(&self) -> EndpointArenaSection {
        self.event_cursor_lane_cursors
    }

    #[inline(always)]
    pub(crate) const fn event_cursor_current_step_label_codes(&self) -> EndpointArenaSection {
        self.event_cursor_current_step_label_codes
    }

    #[inline(always)]
    pub(crate) const fn event_cursor_completed_event_words(&self) -> EndpointArenaSection {
        self.event_cursor_completed_event_words
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

    #[inline(always)]
    pub(crate) const fn route_commit_row_set_builder(&self) -> EndpointArenaSection {
        self.route_commit_row_set_builder
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

    #[inline(always)]
    pub(crate) const fn frontier_offer_entry_slots(&self) -> EndpointArenaSection {
        self.frontier.frontier_offer_entry_slots
    }

    #[inline(always)]
    pub(crate) const fn frontier_root_observed_offer_lanes(&self) -> EndpointArenaSection {
        self.frontier.frontier_root_observed_offer_lanes
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
        let bytes = checked_usize_mul(core::mem::size_of::<T>(), count);
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

#[inline(always)]
const fn bit_word_count(bits: usize) -> usize {
    let pad = u32::BITS as usize - 1;
    if bits > usize::MAX - pad {
        crate::invariant();
    }
    (bits + pad) / u32::BITS as usize
}

#[inline(always)]
const fn align_up(value: usize, align: usize) -> usize {
    if align == 0 {
        crate::invariant();
    }
    let mask = align - 1;
    if value > usize::MAX - mask {
        crate::invariant();
    }
    (value + mask) & !mask
}

#[inline(always)]
const fn checked_usize_mul(lhs: usize, rhs: usize) -> usize {
    if lhs != 0 && rhs > usize::MAX / lhs {
        crate::invariant();
    }
    lhs * rhs
}

#[cfg(test)]
mod tests {
    use super::EndpointArenaLayout;
    use crate::global::role_program::RuntimeRoleFootprint;

    const fn test_footprint(
        active_lane_count: usize,
        endpoint_lane_slot_count: usize,
        logical_lane_count: usize,
        max_route_stack_depth: usize,
        route_scope_count: usize,
    ) -> RuntimeRoleFootprint {
        RuntimeRoleFootprint {
            max_route_stack_depth,
            local_step_count: 0,
            route_scope_count,
            active_lane_count,
            endpoint_lane_slot_count,
            logical_lane_count,
        }
    }

    #[test]
    fn root_frontier_shared_pools_track_max_frontier_entries() {
        let footprint = test_footprint(3, 65, 65, 3, 4);
        let layout = EndpointArenaLayout::from_footprint(footprint);
        assert_eq!(layout.frontier_root_rows().count(), 3);
        assert_eq!(
            layout.frontier_root_active_slots().count(),
            footprint.frontier_entry_count()
        );
        assert_eq!(
            layout.frontier_root_observed_key_slots().count(),
            footprint.frontier_entry_count()
        );
        assert_eq!(
            layout.frontier_root_observed_offer_lanes().count(),
            footprint
                .active_lane_count
                .checked_mul(footprint.lane_word_count())
                .expect("layout test overflow")
        );
        assert_eq!(
            layout.frontier_offer_entry_slots().count(),
            footprint.frontier_entry_count()
        );
    }

    #[test]
    fn route_commit_row_set_builder_no_longer_allocates_row_storage() {
        let mut footprint = test_footprint(1, 1, 1, 1, 70);
        footprint.route_scope_count = 70;
        let layout = EndpointArenaLayout::from_footprint(footprint);

        assert_eq!(layout.route_commit_row_set_builder().count(), 1);
    }

    #[test]
    fn endpoint_arena_does_not_contain_route_scope_lane_cache() {
        let mut base = test_footprint(4, 65, 65, 2, 16);
        base.route_scope_count = 0;
        let base_layout = EndpointArenaLayout::from_footprint(base);

        let mut scoped = base;
        scoped.route_scope_count = 16;
        let scoped_layout = EndpointArenaLayout::from_footprint(scoped);

        assert_eq!(base_layout.route_state_scope_selected_arms().count(), 0);
        assert_eq!(base_layout.scope_evidence_slots().count(), 0);
        assert_eq!(scoped_layout.route_state_scope_selected_arms().count(), 16);
        assert_eq!(scoped_layout.scope_evidence_slots().count(), 16);
        assert_eq!(
            scoped_layout.route_state_lane_linger_lanes().count(),
            base_layout.route_state_lane_linger_lanes().count()
        );
        assert_eq!(
            scoped_layout.route_state_lane_offer_linger_lanes().count(),
            base_layout.route_state_lane_offer_linger_lanes().count()
        );
        assert_eq!(
            scoped_layout.route_state_active_offer_lanes().count(),
            base_layout.route_state_active_offer_lanes().count()
        );
        assert_eq!(
            scoped_layout.frontier_root_observed_offer_lanes().count(),
            base_layout.frontier_root_observed_offer_lanes().count()
        );
    }
}
