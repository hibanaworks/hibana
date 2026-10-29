use super::super::{
    BlobPtr, LaneSetView, PackedLaneRange, PackedRollScopeRow, RoleCompiledCounts,
    RoleImageColumns, RoleLaneImage, RuntimeRoleFacts, RuntimeRoleFootprint,
    compact_local_step_count, lane_byte_count,
};
use super::lane_image::invalid_resident_descriptor;
use super::metadata::{
    derive_active_lane_metadata, lane_columns_are_coherent, passive_parent_rows_are_coherent,
    roll_scope_columns_are_coherent, route_commit_capacity_is_exact, route_index_is_coherent,
    route_slot,
};
use crate::global::{
    compiled::images::CompiledProgramRef,
    typestate::{LocalAction, LocalDependency, LocalNode, PackedEventConflict},
};

mod entry_resources;
mod operation_index;
mod parallel_masks;
mod route_topology;
mod steps;
#[cfg(all(test, hibana_repo_tests))]
mod tests;
mod validation;

/// Immutable role image whose constructor validates every resident event.
/// Private fields prevent a checked image from being changed after publication.
pub(crate) struct RoleImageRef {
    program: &'static CompiledProgramRef,
    role: u8,
    facts: RuntimeRoleFacts,
    columns: &'static RoleImageColumns,
    blob: BlobPtr,
    first_active_lane: u16,
    route_index_start: u32,
    lookup_indexes: u8,
}

#[inline(always)]
const fn compact_count(value: usize) -> u16 {
    if value > u16::MAX as usize {
        panic!("role descriptor fact overflow");
    }
    value as u16
}

impl RuntimeRoleFacts {
    const fn active_lane_row(self) -> PackedLaneRange {
        let footprint = self.footprint();
        PackedLaneRange::new(
            0,
            if footprint.active_lane_count == 0 {
                0
            } else {
                lane_byte_count(footprint.logical_lane_count)
            },
        )
    }

    const MAX_ROUTE_COMMIT_COUNT: usize = 0;
    const LOCAL_STEP_COUNT: usize = 1;
    const ROUTE_SCOPE_COUNT: usize = 2;
    const ACTIVE_LANE_COUNT: usize = 3;
    const ENDPOINT_LANE_SLOT_COUNT: usize = 4;
    const LOGICAL_LANE_COUNT: usize = 5;

    #[inline(always)]
    pub(crate) const fn from_counts(counts: RoleCompiledCounts) -> Self {
        Self {
            words: [
                compact_count(counts.max_route_commit_count),
                compact_local_step_count(counts.local_step_count),
                compact_count(counts.route_scope_count),
                compact_count(counts.active_lane_count),
                compact_count(counts.endpoint_lane_slot_count),
                compact_count(counts.logical_lane_count),
            ],
        }
    }

    #[inline(always)]
    pub(crate) const fn footprint(self) -> RuntimeRoleFootprint {
        RuntimeRoleFootprint {
            max_route_commit_count: self.words[Self::MAX_ROUTE_COMMIT_COUNT] as usize,
            route_arm_state_capacity: 0,
            local_step_count: self.words[Self::LOCAL_STEP_COUNT] as usize,
            roll_scope_count: 0,
            borrow_epoch_count: 0,
            route_scope_count: self.words[Self::ROUTE_SCOPE_COUNT] as usize,
            active_lane_count: self.words[Self::ACTIVE_LANE_COUNT] as usize,
            endpoint_lane_slot_count: self.words[Self::ENDPOINT_LANE_SLOT_COUNT] as usize,
            logical_lane_count: self.words[Self::LOGICAL_LANE_COUNT] as usize,
        }
    }
}

const PARALLEL_REENTRY_INDEX: u8 = 4;
const REENTRY_INDEX: u8 = 8;

impl RoleImageRef {
    pub(crate) const fn new<const N: usize>(
        program: &'static crate::global::compiled::images::CompiledProgramRef,
        role: u8,
        facts: RuntimeRoleFacts,
        columns: &'static RoleImageColumns,
        bytes: &'static [u8; N],
    ) -> Self {
        columns.validate_blob_bound();
        let blob = BlobPtr::from_array(bytes, columns.blob_len());
        let footprint = facts.footprint();
        let active_lane_row = facts.active_lane_row();
        let metadata = match derive_active_lane_metadata(bytes, columns.lane_bits, active_lane_row)
        {
            Some(metadata) => metadata,
            None => invalid_resident_descriptor(),
        };
        if !route_index_is_coherent(bytes, *columns) {
            invalid_resident_descriptor();
        }
        if !passive_parent_rows_are_coherent(bytes, *columns) {
            invalid_resident_descriptor();
        }
        let mut image = Self {
            program,
            role,
            facts,
            columns,
            blob,
            first_active_lane: metadata.first_active_lane,
            route_index_start: columns.route_index().start as u32,
            lookup_indexes: 0,
        };
        if metadata.active_lane_count != footprint.active_lane_count
            || metadata.logical_lane_count != footprint.logical_lane_count
            || metadata.logical_lane_count != footprint.endpoint_lane_slot_count
        {
            invalid_resident_descriptor();
        }
        if !lane_columns_are_coherent(bytes, *columns, active_lane_row, footprint) {
            invalid_resident_descriptor();
        }
        if !roll_scope_columns_are_coherent(bytes, columns.roll_scopes, footprint.local_step_count)
        {
            invalid_resident_descriptor();
        }
        if !route_commit_capacity_is_exact(
            bytes,
            columns.route_commit_ranges,
            columns.route_commit_rows.len as usize,
            footprint.max_route_commit_count,
        ) {
            invalid_resident_descriptor();
        }
        image.validate_resident_arity();
        let count = image.local_step_count();
        if count <= 32 {
            image.validate_resident_data::<32>();
        } else if count <= 128 {
            image.validate_resident_data::<128>();
        } else if count <= 512 {
            image.validate_resident_data::<512>();
        } else if count <= 2048 {
            image.validate_resident_data::<2048>();
        } else {
            image.validate_resident_data::<{ u16::MAX as usize / super::super::ROLE_IMAGE_EVENT_STRIDE }>();
        }
        if image.columns.roll_scopes.len != 0 {
            image.lookup_indexes |= REENTRY_INDEX;
        } else {
            let mut slot = 0;
            while slot < image.columns.route_scopes.len as usize {
                if image.lanes().route_scope_reentry_by_slot(slot) {
                    image.lookup_indexes |= REENTRY_INDEX;
                    break;
                }
                slot += 1;
            }
        }
        image
    }

    const fn validate_resident_data<const E: usize>(&mut self) {
        let rows = validation::ValidationRows::<E>::new(self);
        self.validate_resident_events(&rows);
        self.validate_route_commit_chains();
        self.validate_parallel_borrow_masks(&rows);
        if self.parallel_reentry_is_borrowable(&rows) {
            self.lookup_indexes |= PARALLEL_REENTRY_INDEX;
        }
        self.validate_route_topology(&rows);
    }

    #[inline(always)]
    pub(crate) const fn program(&self) -> &'static CompiledProgramRef {
        self.program
    }

    #[inline(always)]
    pub(crate) const fn role(&self) -> u8 {
        self.role
    }

    #[cfg(all(test, hibana_repo_tests))]
    pub(crate) const fn facts(&self) -> RuntimeRoleFacts {
        self.facts
    }

    #[cfg(all(test, hibana_repo_tests))]
    pub(crate) const fn columns(&self) -> &'static RoleImageColumns {
        self.columns
    }

    #[cfg(all(test, hibana_repo_tests))]
    pub(crate) const fn blob(&self) -> BlobPtr {
        self.blob
    }

    pub(crate) const fn active_lane_row(&self) -> PackedLaneRange {
        self.facts.active_lane_row()
    }

    #[inline(always)]
    pub(crate) const fn has_reentry_scopes(&self) -> bool {
        self.lookup_indexes & REENTRY_INDEX != 0
    }

    #[inline(always)]
    pub(crate) const fn lanes(&self) -> RoleLaneImage<'_> {
        RoleLaneImage::new(self.columns, self.blob)
    }

    #[inline(always)]
    pub(crate) const fn footprint(&self) -> RuntimeRoleFootprint {
        let mut footprint = self.facts.footprint();
        footprint.route_arm_state_capacity = self.columns.route_arm_lane_step_rows.len as usize;
        footprint.roll_scope_count = self.columns.roll_scopes.len as usize;
        footprint.borrow_epoch_count = if self.lookup_indexes & PARALLEL_REENTRY_INDEX != 0 {
            footprint.roll_scope_count
        } else {
            0
        };
        footprint
    }

    #[inline(always)]
    pub(crate) const fn local_step_count(&self) -> usize {
        self.facts.words[RuntimeRoleFacts::LOCAL_STEP_COUNT] as usize
    }

    #[inline(always)]
    pub(crate) const fn active_lane_set(&self) -> LaneSetView<'static> {
        let footprint = self.footprint();
        self.lanes()
            .lane_bit_view(self.active_lane_row(), footprint.lane_word_count())
    }

    #[cfg(all(test, hibana_repo_tests))]
    #[inline(always)]
    pub(crate) const fn resident_row_min_start(&self, idx: usize) -> Option<u16> {
        self.lanes().resident_row_min_start(idx)
    }

    #[cfg(all(test, hibana_repo_tests))]
    pub(crate) const fn reference_resident_lane_count(
        &self,
        idx: usize,
        lane_idx: usize,
    ) -> Option<u16> {
        if lane_idx >= self.footprint().logical_lane_count {
            return None;
        }
        let lanes = self.lanes();
        if idx >= lanes.resident_row_count() {
            return None;
        }
        let row = lanes.resident_row_range(idx);
        if row.end() > self.local_step_count() {
            invalid_resident_descriptor();
        }
        let mut pos = row.start();
        let end = row.end();
        let mut len = 0usize;
        while pos < end {
            if matches!(self.local_step_lane(pos), Some(lane) if lane as usize == lane_idx) {
                len += 1;
            }
            pos += 1;
        }
        if len == 0 {
            return None;
        }
        if len > u16::MAX as usize {
            invalid_resident_descriptor();
        } else {
            Some(len as u16)
        }
    }

    #[inline(always)]
    pub(crate) const fn route_scope_conflict_by_slot(&self, slot: usize) -> PackedEventConflict {
        self.lanes().route_scope_conflict_by_slot(slot)
    }

    #[inline(always)]
    pub(crate) const fn route_commit_range_by_slot(&self, slot: usize, arm: u8) -> PackedLaneRange {
        // The constructor checks each immutable chain once, using the ID index.
        self.lanes().certified_route_commit_range_by_slot(slot, arm)
    }

    #[inline(always)]
    #[cfg(all(test, hibana_repo_tests))]
    pub(crate) const fn route_commit_row_at(&self, idx: usize) -> PackedEventConflict {
        self.lanes().route_commit_row_at(idx)
    }

    pub(crate) fn route_commit_bytes(&self, range: PackedLaneRange) -> &'static [u8] {
        if range.is_absent_or_zero_len() {
            return &[];
        }
        let column = self.columns.route_commit_rows;
        if range.end() > usize::from(column.len) {
            invalid_resident_descriptor();
        }
        // The published image seals these contiguous two-byte rows. Borrow
        // the span once so iteration needs no column lookup for each row.
        let start = column.offset as usize + range.start() * 2;
        let stop = column.offset as usize + range.end() * 2;
        &self.blob.as_slice()[start..stop]
    }

    #[inline(always)]
    pub(crate) const fn roll_scope_row(&self, slot: usize) -> Option<PackedRollScopeRow> {
        self.lanes().roll_scope_row(slot)
    }

    pub(crate) const fn roll_entry_range(&self, slot: usize) -> PackedLaneRange {
        self.lanes().roll_entry_range(slot)
    }

    pub(crate) const fn roll_entry_view(&self, index: usize) -> (u8, u16) {
        self.lanes().roll_entry_view(index)
    }

    pub(crate) const fn roll_entry_receipts(&self, index: usize) -> PackedLaneRange {
        self.lanes().roll_entry_receipts(index)
    }

    pub(crate) const fn owned_receipt(&self, index: usize) -> u16 {
        self.lanes().owned_receipt(index)
    }

    #[inline(always)]
    pub(crate) const fn route_scope_by_slot(
        &self,
        slot: usize,
    ) -> Option<crate::global::const_dsl::ScopeId> {
        self.lanes().route_scope_by_slot(slot)
    }

    #[inline]
    pub(crate) const fn route_scope_slot(
        &self,
        scope: crate::global::const_dsl::ScopeId,
    ) -> Option<usize> {
        route_slot(
            self.blob,
            self.route_index_start as usize
                ..self.route_index_start as usize + self.columns.route_index_len as usize * 2,
            scope,
        )
    }

    #[inline(always)]
    pub(crate) const fn route_scope_reentry_by_slot(&self, slot: usize) -> bool {
        self.lanes().route_scope_reentry_by_slot(slot)
    }

    #[inline(always)]
    pub(crate) const fn passive_arm_child_ordinal_by_slot(
        &self,
        slot: usize,
        arm: u8,
    ) -> Option<u16> {
        self.lanes()
            .certified_passive_arm_child_ordinal_by_slot(slot, arm)
    }

    #[inline(always)]
    pub(crate) const fn route_arm_event_row_by_slot(
        &self,
        slot: usize,
        arm: u8,
    ) -> PackedLaneRange {
        self.lanes()
            .certified_route_arm_event_row_by_slot(slot, arm)
    }

    #[inline]
    pub(crate) const fn local_step_node(&self, step_idx: usize) -> Option<LocalNode> {
        if step_idx >= self.local_step_count() {
            None
        } else {
            let node = match self.lanes().local_step_node(step_idx, self.program) {
                Some(node) => node,
                None => invalid_resident_descriptor(),
            };
            let Some(expected_lane) = self.local_step_lane(step_idx) else {
                invalid_resident_descriptor();
            };
            let actual_lane = match node.action() {
                LocalAction::Send { lane, .. }
                | LocalAction::Recv { lane, .. }
                | LocalAction::Local { lane, .. } => lane,
                #[cfg(all(test, hibana_repo_tests))]
                LocalAction::Terminate => invalid_resident_descriptor(),
            };
            if actual_lane != expected_lane {
                invalid_resident_descriptor();
            }
            Some(node)
        }
    }

    #[cfg(all(test, hibana_repo_tests))]
    pub(crate) const fn reference_resident_lane_step_at(
        &self,
        idx: usize,
        lane_idx: usize,
        ordinal: usize,
    ) -> Option<u16> {
        if lane_idx >= self.footprint().logical_lane_count {
            return None;
        }
        let lanes = self.lanes();
        if idx >= lanes.resident_row_count() {
            return None;
        }
        let row = lanes.resident_row_range(idx);
        if row.end() > self.local_step_count() {
            invalid_resident_descriptor();
        }
        let mut pos = row.start();
        let end = row.end();
        let mut seen = 0usize;
        while pos < end {
            if matches!(self.local_step_lane(pos), Some(lane) if lane as usize == lane_idx) {
                if seen == ordinal {
                    if pos > u16::MAX as usize {
                        invalid_resident_descriptor();
                    }
                    return Some(pos as u16);
                }
                seen += 1;
            }
            pos += 1;
        }
        None
    }

    #[inline(always)]
    pub(crate) const fn first_active_lane(&self) -> Option<usize> {
        if self.first_active_lane == u16::MAX {
            None
        } else {
            Some(self.first_active_lane as usize)
        }
    }

    #[inline(always)]
    pub(crate) const fn route_scope_arm_lane_set_by_slot(
        &self,
        slot: usize,
        arm: u8,
    ) -> LaneSetView<'static> {
        self.lanes()
            .route_scope_arm_lane_set_by_slot(slot, arm, self.footprint().lane_word_count())
    }

    #[inline(always)]
    pub(crate) const fn route_scope_offer_lane_set_by_slot(
        &self,
        slot: usize,
    ) -> LaneSetView<'static> {
        self.lanes()
            .route_scope_offer_lane_set_by_slot(slot, self.footprint().lane_word_count())
    }

    #[inline(always)]
    pub(crate) const fn route_arm_lane_first_step_by_slot(
        &self,
        slot: usize,
        arm: u8,
        lane: u8,
    ) -> Option<u16> {
        self.lanes().route_arm_lane_first_step_by_slot(
            slot,
            arm,
            lane,
            self.footprint().logical_lane_count,
        )
    }

    #[inline(always)]
    pub(crate) const fn route_arm_lane_last_step_by_slot(
        &self,
        slot: usize,
        arm: u8,
        lane: u8,
    ) -> Option<u16> {
        self.lanes().route_arm_lane_last_step_by_slot(
            slot,
            arm,
            lane,
            self.footprint().logical_lane_count,
        )
    }
}
