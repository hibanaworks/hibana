//! Scope registry owners for typestate lowering.

use super::{
    builder::encode_typestate_len,
    facts::{StateIndex, state_index_to_usize},
};
use crate::{
    eff,
    eff::EffIndex,
    global::{
        const_dsl::{CompactScopeId, ScopeId, ScopeKind},
        role_program::{LaneSetView, LaneWord, lane_word_index},
    },
    transport::FrameLabelMask,
};

/// Marker for dispatch entries where frame-label continuation is arm-agnostic.
pub(crate) const ARM_SHARED: u8 = 0xFF;
pub(crate) const MAX_FIRST_RECV_DISPATCH: usize = 16;
pub(crate) const CONTROLLER_ROLE_NONE: u8 = u8::MAX;
pub(crate) const SCOPE_LINK_NONE: u16 = u16::MAX;
pub(crate) const ROUTE_PARENT_ARM_NONE: u8 = u8::MAX;
pub(crate) const ROUTE_DISPATCH_SHAPE_NONE: u16 = u16::MAX;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ScopeRegion {
    pub scope_id: ScopeId,
    pub kind: ScopeKind,
    pub start: usize,
    pub end: usize,
    pub range: u16,
    pub nest: u16,
    pub linger: bool,
    pub controller_role: Option<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ScopeRecord {
    pub scope_id: CompactScopeId,
    pub kind: ScopeKind,
    pub linger: bool,
    pub start: StateIndex,
    pub end: StateIndex,
    pub range: u16,
    pub nest: u16,
    pub parent: u16,
    pub control_parent: u16,
    pub route_parent: u16,
    pub route_parent_arm: u8,
    pub parallel_root: u16,
    pub enclosing_loop: u16,
    pub arm_entry: [StateIndex; 2],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RouteScopeRecord {
    pub route_recv: [StateIndex; 2],
    pub passive_arm_jump: [StateIndex; 2],
    pub offer_lane_word_start: u16,
    pub offer_entry: StateIndex,
    pub route_arm_lane_word_start: u16,
    pub dispatch_shape: u16,
    pub dispatch_target_start: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RouteDispatchEntry {
    pub frame_label: u8,
    pub lane: u8,
    pub arm: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RouteDispatchShape {
    pub first_recv_frame_label_mask: FrameLabelMask,
    pub first_recv_dispatch_arm_frame_label_masks: [FrameLabelMask; 2],
    pub entries_start: u16,
    pub entries_len: u8,
    pub first_recv_dispatch_arm_mask: u8,
    pub first_recv_dispatch_lane_mask: [u8; 2],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RouteScopeScratchRecord {
    pub route_recv: [StateIndex; 2],
    pub passive_arm_jump: [StateIndex; 2],
    pub lane_word_start: u16,
    pub offer_entry: StateIndex,
    pub first_recv_dispatch: [(u8, u8, u8, StateIndex); MAX_FIRST_RECV_DISPATCH],
    pub first_recv_len: u8,
    pub first_recv_frame_label_mask: FrameLabelMask,
    pub first_recv_dispatch_arm_frame_label_masks: [FrameLabelMask; 2],
    pub first_recv_dispatch_arm_mask: u8,
    pub first_recv_dispatch_lane_mask: [u8; 2],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ScopeRegistry {
    pub(super) records: *const ScopeRecord,
    pub(super) len: u16,
    pub(super) slots_by_scope: *const u16,
    pub(super) route_dense_by_slot: *const u16,
    pub(super) route_records: *const RouteScopeRecord,
    pub(super) route_scope_len: u16,
    pub(super) route_offer_lane_words: *const LaneWord,
    pub(super) route_arm0_lane_words: *const LaneWord,
    pub(super) route_arm1_lane_words: *const LaneWord,
    pub(super) route_lane_word_len: u16,
    pub(super) route_dispatch_shapes: *const RouteDispatchShape,
    pub(super) route_dispatch_shape_len: u16,
    pub(super) route_dispatch_entries: *const RouteDispatchEntry,
    pub(super) route_dispatch_entry_len: u16,
    pub(super) route_dispatch_targets: *const StateIndex,
    pub(super) route_dispatch_target_len: u16,
    pub(super) lane_slot_count: u16,
    pub(super) scope_lane_first_eff: *const EffIndex,
    pub(super) scope_lane_last_eff: *const EffIndex,
    pub(super) route_arm0_lane_last_eff_by_route: *const EffIndex,
    pub(super) frontier_entry_capacity_value: u8,
}

#[inline(always)]
pub(super) fn insert_offer_lane(words: &mut [LaneWord], lane: u8) {
    let (word_idx, bit) = lane_word_index(lane as usize);
    if word_idx < words.len() {
        words[word_idx] |= bit;
    }
}

impl ScopeRecord {
    pub(crate) const EMPTY: Self = Self {
        scope_id: CompactScopeId::none(),
        kind: ScopeKind::Generic,
        linger: false,
        start: StateIndex::ZERO,
        end: StateIndex::ZERO,
        range: 0,
        nest: 0,
        parent: SCOPE_LINK_NONE,
        control_parent: SCOPE_LINK_NONE,
        route_parent: SCOPE_LINK_NONE,
        route_parent_arm: ROUTE_PARENT_ARM_NONE,
        parallel_root: SCOPE_LINK_NONE,
        enclosing_loop: SCOPE_LINK_NONE,
        arm_entry: [StateIndex::MAX, StateIndex::MAX],
    };

    const fn region(&self) -> ScopeRegion {
        ScopeRegion {
            scope_id: self.scope_id.to_scope_id(),
            kind: self.kind,
            start: state_index_to_usize(self.start),
            end: state_index_to_usize(self.end),
            range: self.range,
            nest: self.nest,
            linger: self.linger,
            controller_role: None,
        }
    }
}

impl RouteScopeRecord {
    pub(crate) const EMPTY: Self = Self {
        route_recv: [StateIndex::MAX, StateIndex::MAX],
        passive_arm_jump: [StateIndex::MAX, StateIndex::MAX],
        offer_lane_word_start: 0,
        offer_entry: StateIndex::MAX,
        route_arm_lane_word_start: 0,
        dispatch_shape: ROUTE_DISPATCH_SHAPE_NONE,
        dispatch_target_start: 0,
    };

    #[inline(always)]
    pub(crate) const fn route_recv_count(&self) -> u8 {
        if self.route_recv[0].is_max() {
            0
        } else if self.route_recv[1].is_max() {
            1
        } else {
            2
        }
    }

    #[inline(always)]
    const fn route_recv_state(&self, arm: u8) -> Option<StateIndex> {
        if arm >= 2 {
            return None;
        }
        if arm >= self.route_recv_count() {
            return None;
        }
        Some(self.route_recv[arm as usize])
    }

    #[inline(always)]
    pub(crate) const fn offer_lane_word_start(&self) -> usize {
        self.offer_lane_word_start as usize
    }

    #[inline(always)]
    pub(crate) const fn offer_entry(&self) -> StateIndex {
        self.offer_entry
    }

    #[inline(always)]
    pub(crate) const fn route_arm_lane_word_start(&self) -> usize {
        self.route_arm_lane_word_start as usize
    }
}

impl RouteDispatchShape {
    #[inline(always)]
    const fn entries_start(&self) -> usize {
        self.entries_start as usize
    }

    #[inline(always)]
    pub(crate) const fn entries_len(&self) -> usize {
        self.entries_len as usize
    }
}

impl RouteScopeScratchRecord {
    pub(crate) const EMPTY: Self = Self {
        route_recv: [StateIndex::MAX, StateIndex::MAX],
        passive_arm_jump: [StateIndex::MAX, StateIndex::MAX],
        lane_word_start: 0,
        offer_entry: StateIndex::MAX,
        first_recv_dispatch: [(0, 0, 0, StateIndex::MAX); MAX_FIRST_RECV_DISPATCH],
        first_recv_len: 0,
        first_recv_frame_label_mask: FrameLabelMask::EMPTY,
        first_recv_dispatch_arm_frame_label_masks: [FrameLabelMask::EMPTY; 2],
        first_recv_dispatch_arm_mask: 0,
        first_recv_dispatch_lane_mask: [0; 2],
    };

    #[inline(always)]
    pub(crate) const fn route_recv_count(&self) -> u8 {
        if self.route_recv[0].is_max() {
            0
        } else if self.route_recv[1].is_max() {
            1
        } else {
            2
        }
    }

    #[inline(always)]
    pub(crate) const fn lane_word_start(&self) -> usize {
        self.lane_word_start as usize
    }
}

impl ScopeRegistry {
    #[inline(always)]
    fn records(&self) -> &[ScopeRecord] {
        if self.len == 0 {
            &[]
        } else {
            unsafe { core::slice::from_raw_parts(self.records, self.len as usize) }
        }
    }

    #[inline(always)]
    fn slots(&self) -> &[u16] {
        if self.len == 0 {
            &[]
        } else {
            unsafe { core::slice::from_raw_parts(self.slots_by_scope, self.len as usize) }
        }
    }

    #[inline(always)]
    fn route_dense_by_slot(&self) -> &[u16] {
        if self.len == 0 {
            &[]
        } else {
            unsafe { core::slice::from_raw_parts(self.route_dense_by_slot, self.len as usize) }
        }
    }

    #[inline(always)]
    pub(crate) fn route_dense_by_slot_table(&self) -> &[u16] {
        self.route_dense_by_slot()
    }

    #[inline(always)]
    fn route_records(&self) -> &[RouteScopeRecord] {
        if self.route_scope_len == 0 {
            &[]
        } else {
            unsafe {
                core::slice::from_raw_parts(self.route_records, self.route_scope_len as usize)
            }
        }
    }

    #[inline(always)]
    pub(crate) fn route_records_table(&self) -> &[RouteScopeRecord] {
        self.route_records()
    }

    #[inline(always)]
    pub(crate) fn route_offer_lane_words_table(&self) -> &[LaneWord] {
        let len = (self.route_scope_len as usize).saturating_mul(self.route_lane_word_len());
        if len == 0 {
            &[]
        } else {
            unsafe { core::slice::from_raw_parts(self.route_offer_lane_words, len) }
        }
    }

    #[inline(always)]
    pub(crate) fn route_arm0_lane_words_table(&self) -> &[LaneWord] {
        let len = (self.route_scope_len as usize).saturating_mul(self.route_lane_word_len());
        if len == 0 {
            &[]
        } else {
            unsafe { core::slice::from_raw_parts(self.route_arm0_lane_words, len) }
        }
    }

    #[inline(always)]
    pub(crate) fn route_arm1_lane_words_table(&self) -> &[LaneWord] {
        let len = (self.route_scope_len as usize).saturating_mul(self.route_lane_word_len());
        if len == 0 {
            &[]
        } else {
            unsafe { core::slice::from_raw_parts(self.route_arm1_lane_words, len) }
        }
    }

    #[inline(always)]
    fn route_dispatch_shapes(&self) -> &[RouteDispatchShape] {
        if self.route_dispatch_shape_len == 0 {
            &[]
        } else {
            unsafe {
                core::slice::from_raw_parts(
                    self.route_dispatch_shapes,
                    self.route_dispatch_shape_len as usize,
                )
            }
        }
    }

    #[inline(always)]
    fn route_dispatch_entries(&self) -> &[RouteDispatchEntry] {
        if self.route_dispatch_entry_len == 0 {
            &[]
        } else {
            unsafe {
                core::slice::from_raw_parts(
                    self.route_dispatch_entries,
                    self.route_dispatch_entry_len as usize,
                )
            }
        }
    }

    #[inline(always)]
    fn route_dispatch_targets(&self) -> &[StateIndex] {
        if self.route_dispatch_target_len == 0 {
            &[]
        } else {
            unsafe {
                core::slice::from_raw_parts(
                    self.route_dispatch_targets,
                    self.route_dispatch_target_len as usize,
                )
            }
        }
    }

    #[inline(always)]
    pub(crate) fn route_lane_word_len(&self) -> usize {
        self.route_lane_word_len as usize
    }

    #[inline(always)]
    #[cfg(test)]
    fn route_offer_lane_set_for(&self, route: &RouteScopeRecord) -> LaneSetView {
        let word_len = self.route_lane_word_len();
        if word_len == 0 {
            LaneSetView::EMPTY
        } else {
            LaneSetView::from_parts(
                unsafe {
                    self.route_offer_lane_words
                        .add(route.offer_lane_word_start())
                },
                word_len,
            )
        }
    }

    #[inline(always)]
    fn route_arm1_lane_set_for(&self, route: &RouteScopeRecord) -> LaneSetView {
        let word_len = self.route_lane_word_len();
        if word_len == 0 {
            LaneSetView::EMPTY
        } else {
            LaneSetView::from_parts(
                unsafe {
                    self.route_arm1_lane_words
                        .add(route.route_arm_lane_word_start())
                },
                word_len,
            )
        }
    }

    #[inline(always)]
    fn lane_slot_count(&self) -> usize {
        self.lane_slot_count as usize
    }

    #[inline(always)]
    fn lane_row(&self, base: *const EffIndex, slot: usize, row_count: usize) -> &[EffIndex] {
        let lane_slot_count = self.lane_slot_count();
        if lane_slot_count == 0 || slot >= row_count {
            &[]
        } else {
            unsafe {
                core::slice::from_raw_parts(base.add(slot * lane_slot_count), lane_slot_count)
            }
        }
    }

    #[inline(always)]
    pub(super) fn scope_lane_first_row(&self, slot: usize) -> &[EffIndex] {
        self.lane_row(self.scope_lane_first_eff, slot, self.len as usize)
    }

    #[inline(always)]
    pub(super) fn scope_lane_last_row(&self, slot: usize) -> &[EffIndex] {
        self.lane_row(self.scope_lane_last_eff, slot, self.len as usize)
    }

    #[inline(always)]
    pub(super) fn route_arm0_lane_last_row(&self, dense: usize) -> &[EffIndex] {
        self.lane_row(
            self.route_arm0_lane_last_eff_by_route,
            dense,
            self.route_scope_len as usize,
        )
    }

    #[inline(always)]
    fn linked_record(&self, link: u16) -> Option<&ScopeRecord> {
        if link == SCOPE_LINK_NONE {
            None
        } else {
            self.records().get(link as usize)
        }
    }

    #[inline(always)]
    fn linked_scope_id(&self, link: u16) -> Option<ScopeId> {
        self.linked_record(link)
            .map(|record| record.scope_id.to_scope_id())
    }

    #[inline(always)]
    pub(super) fn record_count(&self) -> usize {
        self.len as usize
    }

    #[inline(always)]
    pub(super) fn record_at(&self, idx: usize) -> &ScopeRecord {
        &self.records()[idx]
    }

    fn lookup_slot(&self, scope_id: ScopeId) -> Option<usize> {
        if scope_id.is_none() {
            return None;
        }
        let canonical = scope_id.canonical();
        let target_raw = canonical.raw();
        let mut lo = 0usize;
        let mut hi = self.len as usize;
        while lo < hi {
            let mid = lo + ((hi - lo) / 2);
            let slot = self.slots()[mid];
            let raw = self.records()[slot as usize].scope_id.canonical().raw();
            if raw < target_raw {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        if lo >= self.len as usize {
            return None;
        }
        let slot = self.slots()[lo];
        let slot_idx = slot as usize;
        let record = &self.records()[slot_idx];
        if record.scope_id.canonical().raw() != target_raw {
            return None;
        }
        Some(slot_idx)
    }

    #[inline]
    fn lookup_record(&self, scope_id: ScopeId) -> Option<&ScopeRecord> {
        match self.lookup_slot(scope_id) {
            Some(slot) => Some(&self.records()[slot]),
            None => None,
        }
    }

    #[inline]
    pub(super) fn route_payload_at_slot(&self, slot: usize) -> Option<&RouteScopeRecord> {
        if slot >= self.len as usize {
            return None;
        }
        let dense = self.route_dense_by_slot()[slot];
        if dense == u16::MAX || dense as usize >= self.route_scope_len as usize {
            return None;
        }
        Some(&self.route_records()[dense as usize])
    }

    #[inline]
    fn lookup_route_record(&self, scope_id: ScopeId) -> Option<(&ScopeRecord, &RouteScopeRecord)> {
        let slot = self.lookup_slot(scope_id)?;
        let record = &self.records()[slot];
        if record.kind != ScopeKind::Route {
            return None;
        }
        Some((record, self.route_payload_at_slot(slot)?))
    }

    #[inline(always)]
    fn route_dispatch_shape_for<'a>(
        &'a self,
        route: &RouteScopeRecord,
    ) -> Option<&'a RouteDispatchShape> {
        if route.dispatch_shape == ROUTE_DISPATCH_SHAPE_NONE {
            None
        } else {
            self.route_dispatch_shapes()
                .get(route.dispatch_shape as usize)
        }
    }

    #[inline(always)]
    fn route_dispatch_entries_for<'a>(
        &'a self,
        shape: &RouteDispatchShape,
    ) -> &'a [RouteDispatchEntry] {
        let start = shape.entries_start();
        let len = shape.entries_len();
        let entries = self.route_dispatch_entries();
        if start > entries.len() || len > entries.len().saturating_sub(start) {
            &[]
        } else {
            &entries[start..start + len]
        }
    }

    #[inline(always)]
    fn route_dispatch_targets_for<'a>(
        &'a self,
        route: &RouteScopeRecord,
        shape: &RouteDispatchShape,
    ) -> &'a [StateIndex] {
        let start = route.dispatch_target_start as usize;
        let len = shape.entries_len();
        let targets = self.route_dispatch_targets();
        if start > targets.len() || len > targets.len().saturating_sub(start) {
            &[]
        } else {
            &targets[start..start + len]
        }
    }

    pub(super) fn parent_of(&self, scope_id: ScopeId) -> Option<ScopeId> {
        self.lookup_record(scope_id)
            .and_then(|record| self.linked_scope_id(record.parent))
    }

    pub(super) fn control_parent_of(&self, scope_id: ScopeId) -> Option<ScopeId> {
        self.lookup_record(scope_id)
            .and_then(|record| self.linked_scope_id(record.control_parent))
    }

    pub(super) fn route_parent_of(&self, scope_id: ScopeId) -> Option<ScopeId> {
        self.lookup_record(scope_id)
            .and_then(|record| self.linked_scope_id(record.route_parent))
    }

    pub(super) fn route_parent_arm_of(&self, scope_id: ScopeId) -> Option<u8> {
        let record = self.lookup_record(scope_id)?;
        if record.route_parent == SCOPE_LINK_NONE
            || record.route_parent_arm == ROUTE_PARENT_ARM_NONE
        {
            None
        } else {
            Some(record.route_parent_arm)
        }
    }

    pub(super) fn parallel_root_of(&self, scope_id: ScopeId) -> Option<ScopeId> {
        self.lookup_record(scope_id)
            .and_then(|record| self.linked_scope_id(record.parallel_root))
    }

    pub(super) fn enclosing_loop_of(&self, scope_id: ScopeId) -> Option<ScopeId> {
        self.lookup_record(scope_id)
            .and_then(|record| self.linked_scope_id(record.enclosing_loop))
    }

    pub(super) fn lookup_region(&self, scope_id: ScopeId) -> Option<ScopeRegion> {
        self.lookup_record(scope_id).map(ScopeRecord::region)
    }

    pub(super) fn route_recv_state(&self, scope_id: ScopeId, arm: u8) -> Option<StateIndex> {
        let (_record, route) = self.lookup_route_record(scope_id)?;
        route.route_recv_state(arm)
    }

    pub(super) fn route_arm_count(&self, scope_id: ScopeId) -> Option<u16> {
        let (_record, route) = self.lookup_route_record(scope_id)?;
        Some(route.route_recv_count() as u16)
    }

    #[cfg(test)]
    pub(super) fn route_offer_lane_set(&self, scope_id: ScopeId) -> Option<LaneSetView> {
        let (_record, route) = self.lookup_route_record(scope_id)?;
        Some(self.route_offer_lane_set_for(route))
    }

    #[cfg(test)]
    #[inline(always)]
    pub(super) fn route_arm1_lane_set(&self, scope_id: ScopeId) -> Option<LaneSetView> {
        let (_record, route) = self.lookup_route_record(scope_id)?;
        Some(self.route_arm1_lane_set_for(route))
    }

    #[inline]
    pub(super) fn route_scope_slot(&self, scope_id: ScopeId) -> Option<usize> {
        let slot = self.lookup_slot(scope_id)?;
        let record = &self.records()[slot];
        if record.kind != ScopeKind::Route {
            return None;
        }
        Some(slot)
    }

    #[inline]
    #[cfg(test)]
    pub(super) fn route_scope_dense_ordinal(&self, slot: usize) -> Option<usize> {
        if slot >= self.len as usize {
            return None;
        }
        let dense = self.route_dense_by_slot()[slot];
        if dense == u16::MAX {
            None
        } else {
            Some(dense as usize)
        }
    }

    pub(super) fn route_scope_count(&self) -> usize {
        self.route_scope_len as usize
    }

    #[inline(always)]
    pub(super) fn route_dispatch_shape_count(&self) -> usize {
        self.route_dispatch_shape_len as usize
    }

    #[inline(always)]
    pub(super) fn route_dispatch_entry_count(&self) -> usize {
        self.route_dispatch_entry_len as usize
    }

    #[inline(always)]
    pub(super) fn route_dispatch_target_count(&self) -> usize {
        self.route_dispatch_target_len as usize
    }

    #[inline(always)]
    pub(super) unsafe fn relocate_compact_route_payload(
        &mut self,
        route_records: *const RouteScopeRecord,
        route_offer_lane_words: *const LaneWord,
        route_arm0_lane_words: *const LaneWord,
        route_arm1_lane_words: *const LaneWord,
        route_dispatch_shapes: *const RouteDispatchShape,
        route_dispatch_shape_len: usize,
        route_dispatch_entries: *const RouteDispatchEntry,
        route_dispatch_entry_len: usize,
        route_dispatch_targets: *const StateIndex,
        route_dispatch_target_len: usize,
        route_arm0_lane_last_eff_by_route: *const EffIndex,
    ) {
        self.route_records = route_records;
        self.route_offer_lane_words = route_offer_lane_words;
        self.route_arm0_lane_words = route_arm0_lane_words;
        self.route_arm1_lane_words = route_arm1_lane_words;
        self.route_dispatch_shapes = route_dispatch_shapes;
        self.route_dispatch_shape_len = encode_typestate_len(route_dispatch_shape_len);
        self.route_dispatch_entries = route_dispatch_entries;
        self.route_dispatch_entry_len = encode_typestate_len(route_dispatch_entry_len);
        self.route_dispatch_targets = route_dispatch_targets;
        self.route_dispatch_target_len = encode_typestate_len(route_dispatch_target_len);
        self.route_arm0_lane_last_eff_by_route = route_arm0_lane_last_eff_by_route;
    }

    pub(super) fn frontier_entry_capacity(&self) -> usize {
        self.frontier_entry_capacity_value as usize
    }

    pub(super) fn derive_max_offer_entries(&self) -> usize {
        let records = self.records();
        let mut max_entries = 0usize;
        let mut slot = 0usize;
        while slot < self.len as usize {
            let record = &records[slot];
            if record.kind != ScopeKind::Route {
                slot += 1;
                continue;
            }
            let route = match self.route_payload_at_slot(slot) {
                Some(route) => route,
                None => {
                    slot += 1;
                    continue;
                }
            };

            let mut unique = [StateIndex::MAX; eff::meta::MAX_EFF_NODES];
            let mut unique_len = 0usize;
            let mut push_unique = |state: StateIndex| {
                if state == StateIndex::MAX {
                    return;
                }
                let mut idx = 0usize;
                while idx < unique_len {
                    if unique[idx] == state {
                        return;
                    }
                    idx += 1;
                }
                unique[unique_len] = state;
                unique_len += 1;
            };

            push_unique(record.start);
            push_unique(route.offer_entry);
            let mut parent = record.parent;
            while let Some(parent_record) = self.linked_record(parent) {
                if parent_record.kind == ScopeKind::Route {
                    let Some(parent_route) = self.route_payload_at_slot(parent as usize) else {
                        break;
                    };
                    push_unique(parent_record.start);
                    push_unique(parent_route.offer_entry);
                }
                parent = parent_record.parent;
            }

            if unique_len > max_entries {
                max_entries = unique_len;
            }
            slot += 1;
        }
        max_entries
    }

    pub(super) fn max_route_stack_depth(&self) -> usize {
        let depth = self.derive_max_route_stack_depth();
        if depth == 0 {
            0
        } else {
            depth.saturating_add(1)
        }
    }

    pub(super) fn derive_max_route_stack_depth(&self) -> usize {
        let mut max_depth = 0usize;
        let records = self.records();
        let mut slot = 0usize;
        while slot < self.len as usize {
            let record = &records[slot];
            if record.kind != ScopeKind::Route {
                slot += 1;
                continue;
            }

            let mut depth = 1usize;
            let mut parent = record.parent;
            while let Some(parent_record) = self.linked_record(parent) {
                if parent_record.kind == ScopeKind::Route {
                    depth += 1;
                }
                if parent_record.parent == SCOPE_LINK_NONE {
                    break;
                }
                parent = parent_record.parent;
            }
            if depth > max_depth {
                max_depth = depth;
            }
            slot += 1;
        }
        max_depth
    }

    pub(super) fn max_loop_stack_depth(&self) -> usize {
        self.derive_max_loop_stack_depth()
    }

    pub(super) fn derive_max_loop_stack_depth(&self) -> usize {
        let mut max_depth = 0usize;
        let records = self.records();
        let mut slot = 0usize;
        while slot < self.len as usize {
            let record = &records[slot];
            if record.kind != ScopeKind::Loop {
                slot += 1;
                continue;
            }

            let mut depth = 1usize;
            let mut parent = record.parent;
            while let Some(parent_record) = self.linked_record(parent) {
                if parent_record.kind == ScopeKind::Loop {
                    depth += 1;
                }
                if parent_record.parent == SCOPE_LINK_NONE {
                    break;
                }
                parent = parent_record.parent;
            }
            if depth > max_depth {
                max_depth = depth;
            }
            slot += 1;
        }
        max_depth
    }

    fn scope_lane_eff_in_slot(
        &self,
        base: *const EffIndex,
        slot: usize,
        lane: u8,
    ) -> Option<EffIndex> {
        let lane_idx = lane as usize;
        if slot >= self.len as usize || lane_idx >= self.lane_slot_count() {
            return None;
        }
        let eff_index = self.lane_row(base, slot, self.len as usize)[lane_idx];
        if eff_index == EffIndex::MAX {
            None
        } else {
            Some(eff_index)
        }
    }

    pub(super) fn scope_lane_first_eff(&self, scope_id: ScopeId, lane: u8) -> Option<EffIndex> {
        let slot = self.lookup_slot(scope_id)?;
        self.scope_lane_eff_in_slot(self.scope_lane_first_eff, slot, lane)
    }

    pub(super) fn scope_lane_last_eff(&self, scope_id: ScopeId, lane: u8) -> Option<EffIndex> {
        let slot = self.lookup_slot(scope_id)?;
        self.scope_lane_eff_in_slot(self.scope_lane_last_eff, slot, lane)
    }

    pub(super) fn scope_lane_last_eff_for_arm(
        &self,
        scope_id: ScopeId,
        arm: u8,
        lane: u8,
    ) -> Option<EffIndex> {
        let (_record, route) = self.lookup_route_record(scope_id)?;
        let slot = self.lookup_slot(scope_id)?;
        if arm >= 2 {
            return None;
        }
        let lane_idx = lane as usize;
        if lane_idx >= self.lane_slot_count() {
            return None;
        }
        if arm == 0 {
            let dense = self.route_dense_by_slot()[slot];
            if dense == u16::MAX {
                return None;
            }
            let eff_index = self.route_arm0_lane_last_row(dense as usize)[lane_idx];
            if eff_index == EffIndex::MAX {
                None
            } else {
                Some(eff_index)
            }
        } else if !self.route_arm1_lane_set_for(route).contains(lane as usize) {
            None
        } else {
            self.scope_lane_eff_in_slot(self.scope_lane_last_eff, slot, lane)
        }
    }

    pub(super) fn controller_arm_entry(&self, scope_id: ScopeId, arm: u8) -> Option<StateIndex> {
        let record = match self.lookup_record(scope_id) {
            Some(record) => record,
            None => return None,
        };
        if arm < 2 && record.arm_entry[arm as usize].raw() != StateIndex::MAX.raw() {
            Some(record.arm_entry[arm as usize])
        } else {
            None
        }
    }
    pub(super) fn passive_arm_jump(&self, scope_id: ScopeId, arm: u8) -> Option<StateIndex> {
        if arm >= 2 {
            return None;
        }
        let (_record, route) = self.lookup_route_record(scope_id)?;
        let target = route.passive_arm_jump[arm as usize];
        if target == StateIndex::MAX {
            None
        } else {
            Some(target)
        }
    }

    pub(super) fn passive_arm_entry(&self, scope_id: ScopeId, arm: u8) -> Option<StateIndex> {
        if arm >= 2 {
            return None;
        }
        let record = self.lookup_record(scope_id)?;
        let target = record.arm_entry[arm as usize];
        if target == StateIndex::MAX {
            None
        } else {
            Some(target)
        }
    }

    #[cfg(test)]
    #[inline]
    pub(super) fn first_recv_dispatch_entry(
        &self,
        scope_id: ScopeId,
        idx: usize,
    ) -> Option<(u8, u8, u8, StateIndex)> {
        let (_record, route) = match self.lookup_route_record(scope_id) {
            Some(pair) => pair,
            None => return None,
        };
        let shape = self.route_dispatch_shape_for(route)?;
        let entries = self.route_dispatch_entries_for(shape);
        let targets = self.route_dispatch_targets_for(route, shape);
        let entry = *entries.get(idx)?;
        let target = *targets.get(idx)?;
        Some((entry.frame_label, entry.lane, entry.arm, target))
    }

    #[inline]
    pub(super) fn first_recv_dispatch_table(
        &self,
        scope_id: ScopeId,
    ) -> Option<([(u8, u8, u8, StateIndex); MAX_FIRST_RECV_DISPATCH], u8)> {
        let (_record, route) = match self.lookup_route_record(scope_id) {
            Some(pair) => pair,
            None => return None,
        };
        let shape = self.route_dispatch_shape_for(route)?;
        let entries = self.route_dispatch_entries_for(shape);
        let targets = self.route_dispatch_targets_for(route, shape);
        let mut table = [(0, 0, 0, StateIndex::MAX); MAX_FIRST_RECV_DISPATCH];
        let mut idx = 0usize;
        while idx < entries.len() && idx < targets.len() && idx < MAX_FIRST_RECV_DISPATCH {
            let entry = entries[idx];
            table[idx] = (entry.frame_label, entry.lane, entry.arm, targets[idx]);
            idx += 1;
        }
        Some((table, shape.entries_len as u8))
    }

    #[inline]
    pub(super) fn first_recv_dispatch_frame_label_mask(
        &self,
        scope_id: ScopeId,
    ) -> Option<FrameLabelMask> {
        let (_record, route) = self.lookup_route_record(scope_id)?;
        Some(
            self.route_dispatch_shape_for(route)
                .map_or(FrameLabelMask::EMPTY, |shape| {
                    shape.first_recv_frame_label_mask
                }),
        )
    }

    #[inline]
    pub(super) fn first_recv_dispatch_arm_mask(&self, scope_id: ScopeId) -> Option<u8> {
        let (_record, route) = self.lookup_route_record(scope_id)?;
        Some(
            self.route_dispatch_shape_for(route)
                .map_or(0, |shape| shape.first_recv_dispatch_arm_mask),
        )
    }

    #[inline]
    pub(super) fn first_recv_dispatch_lane_mask(&self, scope_id: ScopeId, arm: u8) -> Option<u8> {
        let (_record, route) = self.lookup_route_record(scope_id)?;
        let shape = self.route_dispatch_shape_for(route);
        if arm >= 2 {
            None
        } else {
            Some(shape.map_or(0, |shape| shape.first_recv_dispatch_lane_mask[arm as usize]))
        }
    }

    #[inline]
    pub(super) fn first_recv_dispatch_arm_frame_label_mask(
        &self,
        scope_id: ScopeId,
        arm: u8,
    ) -> Option<FrameLabelMask> {
        let (_record, route) = self.lookup_route_record(scope_id)?;
        let shape = self.route_dispatch_shape_for(route);
        if arm >= 2 {
            None
        } else {
            Some(shape.map_or(FrameLabelMask::EMPTY, |shape| {
                shape.first_recv_dispatch_arm_frame_label_masks[arm as usize]
            }))
        }
    }

    pub(super) fn first_recv_dispatch_target_for_lane_frame_label(
        &self,
        scope_id: ScopeId,
        lane: u8,
        frame_label: u8,
    ) -> Option<(u8, StateIndex)> {
        let (_record, route) = self.lookup_route_record(scope_id)?;
        let shape = self.route_dispatch_shape_for(route)?;
        let entries = self.route_dispatch_entries_for(shape);
        let targets = self.route_dispatch_targets_for(route, shape);
        let mut idx = 0usize;
        while idx < entries.len() && idx < targets.len() {
            let entry = entries[idx];
            let target = targets[idx];
            if entry.frame_label == frame_label && entry.lane == lane && !target.is_max() {
                return Some((entry.arm, target));
            }
            idx += 1;
        }
        None
    }
}
