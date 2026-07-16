use core::ops::{Deref, DerefMut};

use super::decision_state::{RouteCommitRowSetBuilder, RouteScopeSelectedArmSlot, RouteState};
use super::evidence::RouteArmState;
use super::evidence_store::ScopeEvidenceSlot;
use super::frontier::{ActiveEntrySlot, LaneOfferState, RootFrontierState};
use super::frontier_state::FrontierState;
use crate::global::role_program::{DenseLaneOrdinal, LaneWord, RuntimeRoleFootprint};
use crate::global::typestate::EventCursorState;

pub(super) struct LeasedState<T> {
    ptr: *mut T,
}

impl<T> LeasedState<T> {
    #[inline(always)]
    pub(super) unsafe fn init_from_ptr(dst: *mut Self, ptr: *mut T) {
        /* SAFETY: endpoint arena initialization writes this unpublished
        `LeasedState` cell with the arena section pointer that will back the
        state for the endpoint lifetime. */
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
        /* SAFETY: `LeasedState` stores an endpoint-arena state pointer written
        during initialization; shared deref is tied to `&self` and only reads
        the pinned state section. */
        unsafe { &*self.ptr }
    }
}

impl<T> DerefMut for LeasedState<T> {
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut Self::Target {
        if self.ptr.is_null() {
            crate::invariant();
        }
        /* SAFETY: `&mut self` is the endpoint state's mutation token, so the
        returned reference is the only mutable borrow of the pinned state
        section for this operation. */
        unsafe { &mut *self.ptr }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct EndpointArenaSection {
    offset: u32,
    bytes: u32,
    count: u32,
    align: u16,
}

impl EndpointArenaSection {
    #[inline(always)]
    pub(crate) const fn offset(self) -> usize {
        self.offset as usize
    }

    #[inline(always)]
    pub(crate) const fn count(self) -> usize {
        self.count as usize
    }

    #[inline(always)]
    pub(crate) const fn bytes(self) -> usize {
        self.bytes as usize
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RouteFrontierArenaLayout {
    route_arm_history: EndpointArenaSection,
    lane_offer_state_slots: EndpointArenaSection,
    frontier_state: EndpointArenaSection,
    frontier_root_rows: EndpointArenaSection,
    frontier_root_active_slots: EndpointArenaSection,
    frontier_visited_entries: EndpointArenaSection,
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
    route_state_lane_route_arm_lengths: EndpointArenaSection,
    route_state_scope_selected_arms: EndpointArenaSection,
    route_state_lane_reentry_lanes: EndpointArenaSection,
    route_state_lane_offer_reentry_lanes: EndpointArenaSection,
    route_state_active_offer_lanes: EndpointArenaSection,
    route_commit_row_set_builder: EndpointArenaSection,
    frontier: RouteFrontierArenaLayout,
    total_bytes: usize,
    total_align: usize,
}

impl EndpointArenaLayout {
    pub(crate) const fn from_footprint(footprint: RuntimeRoleFootprint) -> Self {
        let mut offset = 0usize;
        let mut total_align = 1usize;

        let event_cursor_state = Self::section::<EventCursorState>(offset);
        offset = event_cursor_state.end_offset();
        total_align = max_usize(total_align, event_cursor_state.align());

        let event_cursor_lane_cursors =
            Self::section_array::<u16>(offset, footprint.logical_lane_count);
        offset = event_cursor_lane_cursors.end_offset();
        total_align = max_usize(total_align, event_cursor_lane_cursors.align());

        let event_cursor_current_step_label_codes =
            Self::section_array::<u16>(offset, footprint.logical_lane_count);
        offset = event_cursor_current_step_label_codes.end_offset();
        total_align = max_usize(total_align, event_cursor_current_step_label_codes.align());

        let event_cursor_completed_event_words =
            Self::section_array::<u32>(offset, bit_word_count(footprint.local_step_count));
        offset = event_cursor_completed_event_words.end_offset();
        total_align = max_usize(total_align, event_cursor_completed_event_words.align());

        let decision_state = Self::section::<RouteState>(offset);
        offset = decision_state.end_offset();
        total_align = max_usize(total_align, decision_state.align());

        let route_state_lane_dense_by_lane =
            Self::section_array::<DenseLaneOrdinal>(offset, footprint.logical_lane_count);
        offset = route_state_lane_dense_by_lane.end_offset();
        total_align = max_usize(total_align, route_state_lane_dense_by_lane.align());

        let route_state_lane_route_arm_lengths =
            Self::section_array::<u16>(offset, footprint.active_lane_count);
        offset = route_state_lane_route_arm_lengths.end_offset();
        total_align = max_usize(total_align, route_state_lane_route_arm_lengths.align());

        let route_state_scope_selected_arms = Self::section_array::<RouteScopeSelectedArmSlot>(
            offset,
            footprint.scope_evidence_count(),
        );
        offset = route_state_scope_selected_arms.end_offset();
        total_align = max_usize(total_align, route_state_scope_selected_arms.align());

        let route_state_lane_word_count = footprint.lane_word_count();
        let route_state_lane_reentry_lanes =
            Self::section_array::<LaneWord>(offset, route_state_lane_word_count);
        offset = route_state_lane_reentry_lanes.end_offset();
        total_align = max_usize(total_align, route_state_lane_reentry_lanes.align());

        let route_state_lane_offer_reentry_lanes =
            Self::section_array::<LaneWord>(offset, route_state_lane_word_count);
        offset = route_state_lane_offer_reentry_lanes.end_offset();
        total_align = max_usize(total_align, route_state_lane_offer_reentry_lanes.align());

        let route_state_active_offer_lanes =
            Self::section_array::<LaneWord>(offset, route_state_lane_word_count);
        offset = route_state_active_offer_lanes.end_offset();
        total_align = max_usize(total_align, route_state_active_offer_lanes.align());

        let route_commit_row_set_builder = Self::section::<RouteCommitRowSetBuilder>(offset);
        offset = route_commit_row_set_builder.end_offset();
        total_align = max_usize(total_align, route_commit_row_set_builder.align());

        let route_arm_history =
            Self::section_array::<RouteArmState>(offset, footprint.route_arm_state_capacity);
        offset = route_arm_history.end_offset();
        total_align = max_usize(total_align, route_arm_history.align());

        let lane_offer_state_slots =
            Self::section_array::<LaneOfferState>(offset, footprint.active_lane_count);
        offset = lane_offer_state_slots.end_offset();
        total_align = max_usize(total_align, lane_offer_state_slots.align());

        let frontier_state = Self::section::<FrontierState>(offset);
        offset = frontier_state.end_offset();
        total_align = max_usize(total_align, frontier_state.align());

        let frontier_root_rows =
            Self::section_array::<RootFrontierState>(offset, footprint.active_lane_count);
        offset = frontier_root_rows.end_offset();
        total_align = max_usize(total_align, frontier_root_rows.align());

        let frontier_root_active_slots =
            Self::section_array::<ActiveEntrySlot>(offset, footprint.frontier_entry_count());
        offset = frontier_root_active_slots.end_offset();
        total_align = max_usize(total_align, frontier_root_active_slots.align());

        let frontier_visited_entries = Self::section_array::<crate::global::typestate::StateIndex>(
            offset,
            footprint.frontier_entry_count(),
        );
        offset = frontier_visited_entries.end_offset();
        total_align = max_usize(total_align, frontier_visited_entries.align());

        let scope_evidence_slots =
            Self::section_array::<ScopeEvidenceSlot>(offset, footprint.scope_evidence_count());
        offset = scope_evidence_slots.end_offset();
        total_align = max_usize(total_align, scope_evidence_slots.align());

        Self {
            header_align: total_align,
            event_cursor_state,
            event_cursor_lane_cursors,
            event_cursor_current_step_label_codes,
            event_cursor_completed_event_words,
            decision_state,
            route_state_lane_dense_by_lane,
            route_state_lane_route_arm_lengths,
            route_state_scope_selected_arms,
            route_state_lane_reentry_lanes,
            route_state_lane_offer_reentry_lanes,
            route_state_active_offer_lanes,
            route_commit_row_set_builder,
            frontier: RouteFrontierArenaLayout {
                route_arm_history,
                lane_offer_state_slots,
                frontier_state,
                frontier_root_rows,
                frontier_root_active_slots,
                frontier_visited_entries,
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
    pub(crate) const fn route_arm_history(&self) -> EndpointArenaSection {
        self.frontier.route_arm_history
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
    pub(crate) const fn route_state_lane_route_arm_lengths(&self) -> EndpointArenaSection {
        self.route_state_lane_route_arm_lengths
    }

    #[inline(always)]
    pub(crate) const fn route_state_scope_selected_arms(&self) -> EndpointArenaSection {
        self.route_state_scope_selected_arms
    }

    #[inline(always)]
    pub(crate) const fn route_state_lane_reentry_lanes(&self) -> EndpointArenaSection {
        self.route_state_lane_reentry_lanes
    }

    #[inline(always)]
    pub(crate) const fn route_state_lane_offer_reentry_lanes(&self) -> EndpointArenaSection {
        self.route_state_lane_offer_reentry_lanes
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
    pub(crate) const fn frontier_visited_entries(&self) -> EndpointArenaSection {
        self.frontier.frontier_visited_entries
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
            offset: narrow_u32(align_up(offset, align)),
            bytes: narrow_u32(bytes),
            count: 1,
            align: narrow_u16(align),
        }
    }

    #[inline(always)]
    const fn section_array<T>(offset: usize, count: usize) -> EndpointArenaSection {
        let align = core::mem::align_of::<T>();
        let bytes = checked_usize_mul(core::mem::size_of::<T>(), count);
        EndpointArenaSection {
            offset: narrow_u32(align_up(offset, align)),
            bytes: narrow_u32(bytes),
            count: narrow_u32(count),
            align: narrow_u16(align),
        }
    }
}

impl EndpointArenaSection {
    #[inline(always)]
    const fn align(self) -> usize {
        self.align as usize
    }

    #[inline(always)]
    const fn end_offset(self) -> usize {
        checked_usize_add(self.offset(), self.bytes())
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

#[inline(always)]
const fn checked_usize_add(lhs: usize, rhs: usize) -> usize {
    if lhs > usize::MAX - rhs {
        crate::invariant();
    }
    lhs + rhs
}

#[inline(always)]
const fn narrow_u32(value: usize) -> u32 {
    if value > u32::MAX as usize {
        crate::invariant();
    }
    value as u32
}

#[inline(always)]
const fn narrow_u16(value: usize) -> u16 {
    if value > u16::MAX as usize {
        crate::invariant();
    }
    value as u16
}

#[cfg(test)]
mod tests {
    use super::{EndpointArenaLayout, EndpointArenaSection};
    use crate::global::role_program::RuntimeRoleFootprint;

    const fn test_footprint(
        active_lane_count: usize,
        endpoint_lane_slot_count: usize,
        logical_lane_count: usize,
        route_arm_state_capacity: usize,
        route_scope_count: usize,
    ) -> RuntimeRoleFootprint {
        RuntimeRoleFootprint {
            max_route_commit_count: route_scope_count,
            route_arm_state_capacity,
            local_step_count: 0,
            route_scope_count,
            active_lane_count,
            endpoint_lane_slot_count,
            logical_lane_count,
        }
    }

    #[test]
    fn endpoint_arena_layout_records_stay_compact() {
        assert_eq!(core::mem::size_of::<EndpointArenaSection>(), 16);
        assert!(
            core::mem::size_of::<EndpointArenaLayout>() <= 384,
            "endpoint arena layout record must stay compact for attach stack"
        );
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
            layout.frontier_visited_entries().count(),
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
    fn route_history_tracks_descriptor_relations_not_lane_depth_product() {
        let footprint = test_footprint(200, 256, 256, 17, 300);
        let layout = EndpointArenaLayout::from_footprint(footprint);

        assert_eq!(layout.route_arm_history().count(), 17);
        assert_eq!(
            layout.route_arm_history().bytes(),
            17 * core::mem::size_of::<super::RouteArmState>()
        );
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
            scoped_layout.route_state_lane_reentry_lanes().count(),
            base_layout.route_state_lane_reentry_lanes().count()
        );
        assert_eq!(
            scoped_layout.route_state_lane_offer_reentry_lanes().count(),
            base_layout.route_state_lane_offer_reentry_lanes().count()
        );
        assert_eq!(
            scoped_layout.route_state_active_offer_lanes().count(),
            base_layout.route_state_active_offer_lanes().count()
        );
    }
}
