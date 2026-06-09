use super::{
    CompiledProgramImage, LANE_DOMAIN_BYTES, MAX_LOCAL_STEP_LANES, MAX_RESIDENT_LANE_BIT_BYTES,
    MAX_RESIDENT_ROW_BOUNDARY_ROWS, MAX_RESIDENT_ROW_LANE_ROWS, MAX_ROUTE_ARM_LANE_ROWS,
    MAX_ROUTE_SCOPE_LANE_ROWS, PackedLaneRange, PackedLocalEventRow, PackedRouteArmRow,
    RoleLaneScratch, ScopeEvent, ScopeId, ScopeKind, lane_byte_count, lane_byte_index,
};
use crate::global::typestate::{
    LocalConflict, LocalDependency, PackedEventConflict, PackedLocalDependency,
};
mod blob_image;
mod event_rows;
mod lane_image;
mod ref_access;
mod scope_rows;

impl RoleLaneScratch {
    const NO_ACTIVE_LANE: u16 = u16::MAX;
    const ROUTE_SCOPE_ROW_EMPTY: u16 = u16::MAX;
    const ROUTE_SCOPE_ROW_LINGER: u16 = 1 << 15;
    const ROUTE_SCOPE_ROW_ORDINAL_MASK: u16 = Self::ROUTE_SCOPE_ROW_LINGER - 1;

    #[inline(always)]
    const fn local_row_has_lane(&self, row: PackedLaneRange, lane: u8) -> bool {
        let mut pos = row.start();
        let end = row.end();
        while pos < end && pos < MAX_LOCAL_STEP_LANES {
            if self.local_step_lanes[pos] == lane {
                return true;
            }
            pos += 1;
        }
        false
    }

    #[inline(always)]
    const fn fill_dependency_rows<const ROLE: u8>(
        &mut self,
        program: &CompiledProgramImage,
        local_step_effs: &[usize; MAX_LOCAL_STEP_LANES],
        local_step_count: usize,
    ) {
        let view = program.view();
        let markers = view.scope_markers();
        let mut dependency_ends = [0usize; MAX_LOCAL_STEP_LANES];

        let mut parallel_scopes = [ScopeId::none(); MAX_LOCAL_STEP_LANES];
        let mut parallel_starts = [0usize; MAX_LOCAL_STEP_LANES];
        let mut parallel_ends = [usize::MAX; MAX_LOCAL_STEP_LANES];
        let mut parallel_parents = [usize::MAX; MAX_LOCAL_STEP_LANES];
        let mut parallel_stack = [usize::MAX; MAX_LOCAL_STEP_LANES];
        let mut parallel_len = 0usize;
        let mut parallel_depth = 0usize;
        let mut has_route = false;

        let mut marker_idx = 0usize;
        while marker_idx < markers.len() {
            let marker = markers[marker_idx];
            match marker.scope_kind {
                ScopeKind::Parallel => match marker.event {
                    ScopeEvent::Enter => {
                        if parallel_len >= MAX_LOCAL_STEP_LANES
                            || parallel_depth >= MAX_LOCAL_STEP_LANES
                        {
                            panic!("parallel dependency table overflow");
                        }
                        let parent = if parallel_depth == 0 {
                            usize::MAX
                        } else {
                            parallel_stack[parallel_depth - 1]
                        };
                        parallel_scopes[parallel_len] = marker.scope_id;
                        parallel_starts[parallel_len] = marker.offset;
                        parallel_parents[parallel_len] = parent;
                        parallel_stack[parallel_depth] = parallel_len;
                        parallel_len += 1;
                        parallel_depth += 1;
                    }
                    ScopeEvent::Exit => {
                        if parallel_depth == 0 {
                            panic!("parallel scope exit without enter");
                        }
                        parallel_depth -= 1;
                        let parallel_idx = parallel_stack[parallel_depth];
                        if parallel_idx >= parallel_len
                            || !Self::same_scope(parallel_scopes[parallel_idx], marker.scope_id)
                        {
                            panic!("parallel scope markers are not well nested");
                        }
                        parallel_ends[parallel_idx] = marker.offset;
                    }
                },
                ScopeKind::Route => {
                    has_route = true;
                }
                _ => {}
            }
            marker_idx += 1;
        }
        if parallel_depth != 0 {
            panic!("parallel scope enter without exit");
        }

        let mut parallel_idx = 0usize;
        while parallel_idx < parallel_len {
            let exit_eff = parallel_ends[parallel_idx];
            if exit_eff != usize::MAX {
                let row = Self::local_step_range_for_eff_range::<ROLE>(
                    program,
                    parallel_starts[parallel_idx],
                    exit_eff,
                );
                let start = row.start();
                let end = row.end();
                if start < end {
                    let parent_idx = parallel_parents[parallel_idx];
                    let parent_parallel_end = if parent_idx == usize::MAX {
                        exit_eff
                    } else {
                        parallel_ends[parent_idx]
                    };
                    let scope = parallel_scopes[parallel_idx];
                    let conflict = if has_route {
                        Self::dependency_conflict_for_scope(markers, view.len(), scope)
                    } else {
                        LocalConflict::Unconditional
                    };
                    let dependency =
                        LocalDependency::with_conflict_range(scope, conflict, start, end);
                    let dependency = PackedLocalDependency::from_dependency(dependency);
                    let mut step = end;
                    while step < local_step_count && step < MAX_LOCAL_STEP_LANES {
                        let current_eff = local_step_effs[step];
                        let current_lane = self.local_step_lanes[step];
                        let dependency_applies = self.local_row_has_lane(row, current_lane)
                            || current_eff >= parent_parallel_end;
                        if dependency_applies && end >= dependency_ends[step] {
                            self.local_step_dependencies[step] = dependency;
                            dependency_ends[step] = end;
                        }
                        step += 1;
                    }
                }
            }
            parallel_idx += 1;
        }
    }

    #[inline(always)]
    const fn local_step_range_for_eff_range<const ROLE: u8>(
        program: &CompiledProgramImage,
        start_eff: usize,
        end_eff: usize,
    ) -> PackedLaneRange {
        if start_eff >= end_eff {
            return PackedLaneRange::new(0, 0);
        }
        let view = program.view();
        let mut local_step = 0usize;
        let mut local_start = usize::MAX;
        let mut local_len = 0usize;
        let mut eff_idx = 0usize;
        while eff_idx < view.len() {
            if let Some(atom) = view.atom_at(eff_idx) {
                if atom.from == ROLE || atom.to == ROLE {
                    if eff_idx >= start_eff && eff_idx < end_eff {
                        if local_start == usize::MAX {
                            local_start = local_step;
                        }
                        local_len += 1;
                    }
                    local_step += 1;
                }
            }
            eff_idx += 1;
        }
        if local_start == usize::MAX {
            PackedLaneRange::new(0, 0)
        } else {
            PackedLaneRange::new(local_start, local_len)
        }
    }

    #[inline(always)]
    const fn push_resident_row(&mut self, row: PackedLaneRange) {
        if row.len() == 0 {
            return;
        }
        let idx = self.resident_row_len as usize;
        if idx >= MAX_RESIDENT_ROW_LANE_ROWS {
            panic!("role resident row overflow");
        }
        if row.start() > u16::MAX as usize || row.end() > u16::MAX as usize {
            panic!("role resident row range overflow");
        }
        let start = row.start() as u16;
        let end = row.end() as u16;
        if idx == 0 {
            self.resident_row_boundaries[0] = start;
        } else if self.resident_row_boundaries[idx] != start {
            panic!("role resident rows must be contiguous");
        }
        self.resident_row_boundaries[idx + 1] = end;
        self.resident_row_len += 1;
    }

    #[inline(always)]
    const fn append_lane_bit_row_for_local_range(
        &mut self,
        row: PackedLaneRange,
    ) -> PackedLaneRange {
        if row.is_empty() || row.len() == 0 {
            return PackedLaneRange::new(0, 0);
        }
        if row.end() > MAX_LOCAL_STEP_LANES {
            panic!("resident lane bit row exceeds local lane table");
        }

        let mut bytes = [0u8; LANE_DOMAIN_BYTES];
        let mut max_lane_plus_one = 0usize;
        let mut pos = row.start();
        let end = row.end();
        while pos < end {
            let lane = self.local_step_lanes[pos] as usize;
            let (byte_idx, bit) = lane_byte_index(lane);
            bytes[byte_idx] |= bit;
            let lane_plus_one = lane.saturating_add(1);
            if lane_plus_one > max_lane_plus_one {
                max_lane_plus_one = lane_plus_one;
            }
            pos += 1;
        }

        let byte_len = lane_byte_count(max_lane_plus_one);
        if byte_len == 0 {
            return PackedLaneRange::new(0, 0);
        }
        let start = self.lane_bit_row_len as usize;
        let end = start.saturating_add(byte_len);
        if end > MAX_RESIDENT_LANE_BIT_BYTES || end > u16::MAX as usize {
            panic!("resident lane bit row overflow");
        }
        let mut idx = 0usize;
        while idx < byte_len {
            self.lane_bit_rows[start + idx] = bytes[idx];
            idx += 1;
        }
        self.lane_bit_row_len = end as u16;
        PackedLaneRange::new(start, byte_len)
    }

    #[inline(always)]
    const fn lane_bit_row_byte(&self, row: PackedLaneRange, idx: usize) -> u8 {
        if row.is_empty() || idx >= row.len() {
            0
        } else {
            let offset = row.start().saturating_add(idx);
            if offset >= MAX_RESIDENT_LANE_BIT_BYTES {
                0
            } else {
                self.lane_bit_rows[offset]
            }
        }
    }

    #[inline(always)]
    const fn append_lane_bit_union_row(
        &mut self,
        left: PackedLaneRange,
        right: PackedLaneRange,
    ) -> PackedLaneRange {
        let byte_len = if left.len() > right.len() {
            left.len()
        } else {
            right.len()
        };
        if byte_len == 0 {
            return PackedLaneRange::new(0, 0);
        }
        let start = self.lane_bit_row_len as usize;
        let end = start.saturating_add(byte_len);
        if end > MAX_RESIDENT_LANE_BIT_BYTES || end > u16::MAX as usize {
            panic!("resident lane bit union row overflow");
        }
        let mut idx = 0usize;
        while idx < byte_len {
            self.lane_bit_rows[start + idx] =
                self.lane_bit_row_byte(left, idx) | self.lane_bit_row_byte(right, idx);
            idx += 1;
        }
        self.lane_bit_row_len = end as u16;
        PackedLaneRange::new(start, byte_len)
    }

    #[inline(always)]
    const fn push_resident_rows<const ROLE: u8>(&mut self, program: &CompiledProgramImage) {
        let view = program.view();
        let markers = view.scope_markers();
        let mut current_eff = 0usize;
        let mut marker_idx = 0usize;
        while marker_idx < markers.len() {
            let marker = markers[marker_idx];
            if matches!(marker.event, ScopeEvent::Enter)
                && matches!(marker.scope_kind, ScopeKind::Parallel)
            {
                let mut exit_eff = usize::MAX;
                let mut scan = marker_idx + 1;
                while scan < markers.len() {
                    let candidate = markers[scan];
                    if Self::same_scope(candidate.scope_id, marker.scope_id)
                        && matches!(candidate.event, ScopeEvent::Exit)
                    {
                        exit_eff = candidate.offset;
                        break;
                    }
                    scan += 1;
                }
                if exit_eff == usize::MAX {
                    panic!("parallel scope exit missing");
                }
                self.push_resident_row(Self::local_step_range_for_eff_range::<ROLE>(
                    program,
                    current_eff,
                    marker.offset,
                ));
                let parallel_start = if marker.offset > current_eff {
                    marker.offset
                } else {
                    current_eff
                };
                self.push_resident_row(Self::local_step_range_for_eff_range::<ROLE>(
                    program,
                    parallel_start,
                    exit_eff,
                ));
                current_eff = if exit_eff > current_eff {
                    exit_eff
                } else {
                    current_eff
                };
            }
            marker_idx += 1;
        }
        self.push_resident_row(Self::local_step_range_for_eff_range::<ROLE>(
            program,
            current_eff,
            view.len(),
        ));
        if self.resident_row_len == 0 {
            self.push_resident_row(Self::local_step_range_for_eff_range::<ROLE>(
                program,
                0,
                view.len(),
            ));
        }
    }

    #[inline(always)]
    const fn append_route_arm_lane_row<const ROLE: u8>(
        &mut self,
        program: &CompiledProgramImage,
        slot: usize,
        arm: usize,
        start_eff: usize,
        end_eff: usize,
    ) -> PackedLaneRange {
        let row_idx = slot.saturating_mul(2).saturating_add(arm);
        if row_idx >= MAX_ROUTE_ARM_LANE_ROWS {
            panic!("route arm lane row overflow");
        }
        let local_row = Self::local_step_range_for_eff_range::<ROLE>(program, start_eff, end_eff);
        self.route_arm_lane_rows[row_idx] = self.append_lane_bit_row_for_local_range(local_row);
        local_row
    }

    #[inline(always)]
    const fn route_scope_ordinal_from_row(row: u16) -> Option<u16> {
        if row == Self::ROUTE_SCOPE_ROW_EMPTY {
            None
        } else {
            Some(row & Self::ROUTE_SCOPE_ROW_ORDINAL_MASK)
        }
    }

    #[inline(always)]
    const fn route_slot_for_scope(&self, scope: ScopeId) -> Option<usize> {
        if scope.is_none() {
            return None;
        }
        let target = scope.local_ordinal();
        let mut slot = 0usize;
        while slot < MAX_ROUTE_SCOPE_LANE_ROWS {
            if let Some(ordinal) = Self::route_scope_ordinal_from_row(self.route_scope_rows[slot]) {
                if ordinal == target {
                    return Some(slot);
                }
            }
            slot += 1;
        }
        None
    }

    #[inline(always)]
    const fn route_scope_conflict_for_commit(&self, scope: ScopeId) -> PackedEventConflict {
        match self.route_slot_for_scope(scope) {
            Some(slot) => self.route_scope_conflicts[slot],
            None => PackedEventConflict::none(),
        }
    }

    #[inline(always)]
    pub(crate) const fn route_commit_row_count(&self, slot: usize, arm: u8) -> usize {
        if arm >= 2 {
            panic!("route commit arm overflow");
        }
        let Some(ordinal) = Self::route_scope_ordinal_from_row(self.route_scope_rows[slot]) else {
            panic!("route commit scope row missing");
        };
        let mut len = 0usize;
        let mut conflict = PackedEventConflict::route_arm(ScopeId::route(ordinal), arm);
        while len < MAX_ROUTE_SCOPE_LANE_ROWS + 1 {
            let Some(LocalConflict::RouteArm { scope, .. }) = conflict.to_conflict() else {
                return len;
            };
            if scope.is_none() {
                panic!("route commit scope missing");
            }
            len += 1;
            conflict = self.route_scope_conflict_for_commit(scope);
        }
        if len == MAX_ROUTE_SCOPE_LANE_ROWS + 1
            && matches!(conflict.to_conflict(), Some(LocalConflict::RouteArm { .. }))
        {
            panic!("route commit rows overflow");
        }
        len
    }

    #[inline(always)]
    pub(crate) const fn route_commit_conflict_at(
        &self,
        slot: usize,
        arm: u8,
        target: usize,
    ) -> PackedEventConflict {
        if arm >= 2 {
            panic!("route commit arm overflow");
        }
        let Some(ordinal) = Self::route_scope_ordinal_from_row(self.route_scope_rows[slot]) else {
            panic!("route commit scope row missing");
        };
        let mut depth = 0usize;
        let mut conflict = PackedEventConflict::route_arm(ScopeId::route(ordinal), arm);
        while depth <= target && depth < MAX_ROUTE_SCOPE_LANE_ROWS + 1 {
            let Some(LocalConflict::RouteArm { scope, .. }) = conflict.to_conflict() else {
                panic!("route commit row missing");
            };
            if scope.is_none() {
                panic!("route commit scope missing");
            }
            if depth == target {
                return conflict;
            }
            conflict = self.route_scope_conflict_for_commit(scope);
            depth += 1;
        }
        panic!("route commit row overflow");
    }

    #[inline(always)]
    const fn append_route_commit_range(&mut self, slot: usize, arm: u8) {
        if arm >= 2 {
            panic!("route commit arm overflow");
        }
        let row_idx = slot.saturating_mul(2).saturating_add(arm as usize);
        if row_idx >= MAX_ROUTE_ARM_LANE_ROWS {
            panic!("route commit range overflow");
        }
        let len = self.route_commit_row_count(slot, arm);
        let start = self.route_commit_row_len as usize;
        let end = start.saturating_add(len);
        if end > u16::MAX as usize || len > u16::MAX as usize {
            panic!("route commit row overflow");
        }
        self.route_commit_row_len = end as u16;
        self.route_commit_ranges[row_idx] = PackedLaneRange::new(start, len);
    }

    #[inline(always)]
    const fn push_route_arm_lane_rows<const ROLE: u8>(&mut self, program: &CompiledProgramImage) {
        let view = program.view();
        let markers = view.scope_markers();
        let mut route_slot = 0usize;
        let mut marker_idx = 0usize;
        while marker_idx < markers.len() {
            let marker = markers[marker_idx];
            if Self::first_enter_for_scope(markers, marker_idx)
                && matches!(marker.scope_kind, ScopeKind::Route)
            {
                let scope = marker.scope_id;
                let view_len = view.len();
                let Some(ranges) = Self::route_arm_ranges(markers, scope) else {
                    panic!("route scope missing binary arm ranges");
                };
                if route_slot >= MAX_ROUTE_SCOPE_LANE_ROWS {
                    panic!("route conflict row overflow");
                }
                let ordinal = scope.local_ordinal();
                if ordinal > Self::ROUTE_SCOPE_ROW_ORDINAL_MASK {
                    panic!("route scope ordinal overflow");
                }
                self.route_scope_rows[route_slot] = ordinal
                    | if marker.linger {
                        Self::ROUTE_SCOPE_ROW_LINGER
                    } else {
                        0
                    };
                let conflict = Self::dependency_conflict_for_scope(markers, view_len, scope);
                self.route_scope_conflicts[route_slot] =
                    PackedEventConflict::from_conflict(conflict);
                let mut arm = 0usize;
                while arm < 2 {
                    let (start, end) = ranges[arm];
                    let local_row = self
                        .append_route_arm_lane_row::<ROLE>(program, route_slot, arm, start, end);
                    self.push_route_arm_projection_row(
                        markers, view_len, route_slot, scope, arm as u8, local_row, start, end,
                    );
                    self.append_route_commit_range(route_slot, arm as u8);
                    arm += 1;
                }
                if route_slot >= MAX_ROUTE_SCOPE_LANE_ROWS {
                    panic!("route offer lane row overflow");
                }
                let left = self.route_arm_lane_rows[route_slot.saturating_mul(2)];
                let right =
                    self.route_arm_lane_rows[route_slot.saturating_mul(2).saturating_add(1)];
                self.route_offer_lane_rows[route_slot] =
                    self.append_lane_bit_union_row(left, right);
                route_slot += 1;
            }
            marker_idx += 1;
        }
    }

    #[inline(always)]
    const fn compact_event_fact_rows(&mut self, local_step_count: usize) {
        let mut dependency_len = 0usize;
        let mut conflict_len = 0usize;
        let mut idx = 0usize;
        while idx < local_step_count {
            let dependency = self.local_step_dependencies[idx];
            if !dependency.is_none() {
                self.local_step_dependencies[dependency_len] = dependency;
                self.local_step_events[idx] =
                    self.local_step_events[idx].with_dependency_row(dependency_len);
                dependency_len += 1;
            }

            let conflict = self.local_step_conflicts[idx];
            if !conflict.is_none() {
                self.local_step_conflicts[conflict_len] = conflict;
                self.local_step_events[idx] =
                    self.local_step_events[idx].with_conflict_row(conflict_len);
                conflict_len += 1;
            }
            idx += 1;
        }
        if dependency_len > u16::MAX as usize || conflict_len > u16::MAX as usize {
            panic!("local event fact row count overflow");
        }
        self.dependency_row_len = dependency_len as u16;
        self.conflict_row_len = conflict_len as u16;
    }

    #[inline(always)]
    pub(crate) const fn from_program<const ROLE: u8>(
        program: &CompiledProgramImage,
        logical_lane_count: usize,
    ) -> Self {
        let mut lanes = Self {
            local_step_events: [PackedLocalEventRow::EMPTY; MAX_LOCAL_STEP_LANES],
            local_step_lanes: [0; MAX_LOCAL_STEP_LANES],
            local_step_dependencies: [PackedLocalDependency::none(); MAX_LOCAL_STEP_LANES],
            local_step_conflicts: [PackedEventConflict::none(); MAX_LOCAL_STEP_LANES],
            route_scope_rows: [Self::ROUTE_SCOPE_ROW_EMPTY; MAX_ROUTE_SCOPE_LANE_ROWS],
            route_scope_conflicts: [PackedEventConflict::none(); MAX_ROUTE_SCOPE_LANE_ROWS],
            route_arm_rows: [PackedRouteArmRow::EMPTY; MAX_ROUTE_ARM_LANE_ROWS],
            resident_row_boundaries: [0; MAX_RESIDENT_ROW_BOUNDARY_ROWS],
            lane_bit_rows: [0; MAX_RESIDENT_LANE_BIT_BYTES],
            route_arm_lane_rows: [PackedLaneRange::EMPTY; MAX_ROUTE_ARM_LANE_ROWS],
            route_offer_lane_rows: [PackedLaneRange::EMPTY; MAX_ROUTE_SCOPE_LANE_ROWS],
            route_commit_ranges: [PackedLaneRange::EMPTY; MAX_ROUTE_ARM_LANE_ROWS],
            active_lane_row: PackedLaneRange::EMPTY,
            resident_row_len: 0,
            dependency_row_len: 0,
            conflict_row_len: 0,
            lane_bit_row_len: 0,
            route_commit_row_len: 0,
            first_active_lane: Self::NO_ACTIVE_LANE,
        };
        let view = program.view();
        let markers = view.scope_markers();
        let mut local_step_effs = [usize::MAX; MAX_LOCAL_STEP_LANES];
        let mut frame_key_targets = [0u8; MAX_LOCAL_STEP_LANES];
        let mut frame_key_lanes = [0u8; MAX_LOCAL_STEP_LANES];
        let mut frame_key_counts = [0u16; MAX_LOCAL_STEP_LANES];
        let mut frame_key_len = 0usize;
        let mut step = 0usize;
        let mut idx = 0usize;
        while idx < view.len() {
            if let Some(atom) = view.atom_at(idx) {
                let mut frame_key_idx = 0usize;
                let mut frame_label = 0u8;
                let mut frame_key_found = false;
                while frame_key_idx < frame_key_len {
                    if frame_key_targets[frame_key_idx] == atom.to
                        && frame_key_lanes[frame_key_idx] == atom.lane
                    {
                        let frame_count = frame_key_counts[frame_key_idx];
                        if frame_count > u8::MAX as u16 {
                            panic!("frame label universe overflow");
                        }
                        frame_label = frame_count as u8;
                        frame_key_counts[frame_key_idx] = frame_count + 1;
                        frame_key_found = true;
                        break;
                    }
                    frame_key_idx += 1;
                }
                if !frame_key_found {
                    if frame_key_len >= MAX_LOCAL_STEP_LANES {
                        panic!("frame label key table overflow");
                    }
                    frame_key_targets[frame_key_len] = atom.to;
                    frame_key_lanes[frame_key_len] = atom.lane;
                    frame_key_counts[frame_key_len] = 1;
                    frame_key_len += 1;
                }
                if atom.from == ROLE || atom.to == ROLE {
                    let lane = atom.lane as usize;
                    if lane < logical_lane_count {
                        if lane < lanes.first_active_lane as usize {
                            lanes.first_active_lane = lane as u16;
                        }
                        if step >= MAX_LOCAL_STEP_LANES {
                            panic!("role local lane table overflow");
                        }
                        lanes.local_step_events[step] =
                            Self::local_event_row_for_eff::<ROLE>(program, idx, frame_label);
                        lanes.local_step_lanes[step] = atom.lane;
                        local_step_effs[step] = idx;
                        lanes.local_step_conflicts[step] =
                            Self::route_conflict_for_eff(markers, idx);
                    }
                    step += 1;
                }
            }
            idx += 1;
        }
        lanes.fill_dependency_rows::<ROLE>(program, &local_step_effs, step);
        lanes.compact_event_fact_rows(step);
        lanes.active_lane_row =
            lanes.append_lane_bit_row_for_local_range(PackedLaneRange::new(0, step));
        lanes.push_resident_rows::<ROLE>(program);
        lanes.push_route_arm_lane_rows::<ROLE>(program);
        lanes
    }
}

impl RoleLaneScratch {
    #[inline(always)]
    pub(crate) const fn resident_boundary_count(&self) -> usize {
        if self.resident_row_len == 0 {
            0
        } else {
            self.resident_row_len as usize + 1
        }
    }

    #[inline(always)]
    pub(crate) const fn lane_bit_row_len(&self) -> usize {
        self.lane_bit_row_len as usize
    }

    #[inline(always)]
    pub(crate) const fn dependency_row_len(&self) -> usize {
        self.dependency_row_len as usize
    }

    #[inline(always)]
    pub(crate) const fn conflict_row_len(&self) -> usize {
        self.conflict_row_len as usize
    }

    #[inline(always)]
    pub(crate) const fn route_commit_row_len(&self) -> usize {
        self.route_commit_row_len as usize
    }
}
