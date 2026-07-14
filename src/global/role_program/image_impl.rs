use super::{
    LANE_DOMAIN_BYTES, LANE_DOMAIN_SIZE, MAX_LOCAL_STEP_LANES, MAX_RESIDENT_LANE_BIT_BYTES,
    MAX_RESIDENT_ROW_BOUNDARY_ROWS, MAX_RESIDENT_ROW_LANE_ROWS, MAX_ROUTE_ARM_LANE_ROWS,
    MAX_ROUTE_SCOPE_LANE_ROWS, PackedLaneRange, PackedLocalEventRow, PackedRollScopeRow,
    PackedRouteArmRow, RoleLaneScratch, ScopeEvent, ScopeId, ScopeKind, lane_byte_count,
    lane_byte_index,
};
use crate::global::const_dsl::EffList;
use crate::global::frame_labels::frame_label_at;
use crate::global::typestate::{
    LocalConflict, LocalDependency, PackedEventConflict, PackedLocalDependency,
};
mod blob_image;
mod event_rows;
#[cfg(kani)]
mod kani;
mod lane_image;
mod plan;
mod ref_access;
mod roll_rows;
mod scope_rows;
#[cfg(all(test, hibana_repo_tests))]
mod tests;

use scope_rows::RouteArmProjectionRowInput;

#[inline(always)]
const fn decode_binary_route_arm_index(arm: u8) -> Option<usize> {
    match arm {
        0 => Some(0),
        1 => Some(1),
        2..=u8::MAX => None,
    }
}

#[inline(never)]
const fn binary_route_arm_index(arm: u8) -> usize {
    match decode_binary_route_arm_index(arm) {
        Some(index) => index,
        None => crate::invariant(),
    }
}

#[inline(never)]
const fn route_arm_row_index(slot: usize, arm: u8) -> usize {
    let arm = binary_route_arm_index(arm);
    let Some(base) = slot.checked_mul(2) else {
        crate::invariant();
    };
    let Some(row) = base.checked_add(arm) else {
        crate::invariant();
    };
    row
}

impl RoleLaneScratch {
    const ACTIVE_LANE_NONE: u16 = u16::MAX;

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

    pub(super) const fn scope_markers_contain_kind(
        markers: &[crate::global::const_dsl::ScopeMarker],
        kind: ScopeKind,
    ) -> bool {
        let mut marker_idx = 0usize;
        while marker_idx < markers.len() {
            let marker = markers[marker_idx];
            if let Some(candidate) = marker.scope_id.kind()
                && candidate as u16 == kind as u16
            {
                return true;
            }
            marker_idx += 1;
        }
        false
    }

    const fn fill_dependency_rows(
        &mut self,
        eff_list: &EffList,
        local_step_count: usize,
        role: u8,
        has_route: bool,
    ) {
        let markers = eff_list.scope_markers();
        let mut marker_idx = 0usize;
        while marker_idx < markers.len() {
            let marker = markers[marker_idx];
            if matches!(marker.event, ScopeEvent::Enter)
                && matches!(marker.scope_id.kind(), Some(ScopeKind::Parallel))
            {
                let exit_eff = Self::parallel_exit_for_enter(markers, marker_idx);
                let row =
                    Self::local_step_range_for_eff_range(eff_list, marker.offset(), exit_eff, role);
                let start = row.start();
                let end = row.end();
                if start < end {
                    let parent_parallel_end =
                        Self::nearest_parent_parallel_end(markers, marker_idx, exit_eff);
                    let scope = marker.scope_id;
                    let conflict = if has_route {
                        Self::dependency_conflict_for_scope(markers, eff_list.len(), scope)
                    } else {
                        LocalConflict::Unconditional
                    };
                    let dependency =
                        LocalDependency::with_conflict_range(scope, conflict, start, end);
                    let dependency = PackedLocalDependency::from_dependency(dependency);
                    let mut step = end;
                    while step < local_step_count && step < MAX_LOCAL_STEP_LANES {
                        let current_eff = self.local_step_events[step].eff_index as usize;
                        let current_lane = self.local_step_lanes[step];
                        let dependency_applies = self.local_row_has_lane(row, current_lane)
                            || current_eff >= parent_parallel_end;
                        let current_dependency = self.local_step_dependencies[step];
                        let replaces_current = current_dependency.is_none()
                            || end >= current_dependency.end() as usize;
                        if dependency_applies && replaces_current {
                            self.local_step_dependencies[step] = dependency;
                        }
                        step += 1;
                    }
                }
            }
            marker_idx += 1;
        }
    }

    const fn parallel_exit_for_enter(
        markers: &[crate::global::const_dsl::ScopeMarker],
        enter_idx: usize,
    ) -> usize {
        let marker = markers[enter_idx];
        if !matches!(marker.event, ScopeEvent::Enter)
            || !matches!(marker.scope_id.kind(), Some(ScopeKind::Parallel))
        {
            panic!("parallel scope enter expected");
        }
        let mut depth = 0usize;
        let mut scan = enter_idx + 1;
        while scan < markers.len() {
            let candidate = markers[scan];
            match candidate.event {
                ScopeEvent::Enter => depth += 1,
                ScopeEvent::Exit => {
                    if depth == 0 {
                        return candidate.offset();
                    }
                    depth -= 1;
                }
                ScopeEvent::Split => {}
            }
            scan += 1;
        }
        panic!("parallel scope exit missing");
    }

    const fn nearest_parent_parallel_end(
        markers: &[crate::global::const_dsl::ScopeMarker],
        enter_idx: usize,
        exit_eff: usize,
    ) -> usize {
        let mut depth = 0usize;
        let mut scan = enter_idx;
        while scan > 0 {
            scan -= 1;
            let candidate = markers[scan];
            match candidate.event {
                ScopeEvent::Exit => depth += 1,
                ScopeEvent::Enter => {
                    if depth == 0 {
                        if matches!(candidate.scope_id.kind(), Some(ScopeKind::Parallel)) {
                            return Self::parallel_exit_for_enter(markers, scan);
                        }
                    } else {
                        depth -= 1;
                    }
                }
                ScopeEvent::Split => {}
            }
        }
        exit_eff
    }

    const fn local_step_range_for_eff_range(
        eff_list: &EffList,
        start_eff: usize,
        end_eff: usize,
        role: u8,
    ) -> PackedLaneRange {
        if start_eff >= end_eff {
            return PackedLaneRange::new(0, 0);
        }
        let mut local_step = 0usize;
        let mut local_start = usize::MAX;
        let mut local_len = 0usize;
        let mut eff_idx = 0usize;
        while eff_idx < eff_list.len() {
            let node = eff_list.node_at(eff_idx);
            if matches!(node.kind, crate::eff::EffKind::Atom) {
                let atom = node.atom_data();
                if atom.from == role || atom.to == role {
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
        if row.is_absent_or_zero_len() {
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

    const fn append_lane_bit_row_for_local_range(
        &mut self,
        row: PackedLaneRange,
    ) -> PackedLaneRange {
        if row.is_absent_or_zero_len() {
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
            let lane_plus_one = lane + 1;
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
        let end = start + byte_len;
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
            let offset = row.start() + idx;
            if offset >= MAX_RESIDENT_LANE_BIT_BYTES {
                0
            } else {
                self.lane_bit_rows[offset]
            }
        }
    }

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
        let end = start + byte_len;
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

    const fn push_resident_rows(&mut self, eff_list: &EffList, role: u8) {
        let markers = eff_list.scope_markers();
        let mut current_eff = 0usize;
        let mut marker_idx = 0usize;
        while marker_idx < markers.len() {
            let marker = markers[marker_idx];
            if matches!(marker.event, ScopeEvent::Enter)
                && matches!(marker.scope_id.kind(), Some(ScopeKind::Parallel))
            {
                let mut exit_eff = usize::MAX;
                let mut scan = marker_idx + 1;
                while scan < markers.len() {
                    let candidate = markers[scan];
                    if Self::same_scope(candidate.scope_id, marker.scope_id)
                        && matches!(candidate.event, ScopeEvent::Exit)
                    {
                        exit_eff = candidate.offset();
                        break;
                    }
                    scan += 1;
                }
                if exit_eff == usize::MAX {
                    panic!("parallel scope exit missing");
                }
                self.push_resident_row(Self::local_step_range_for_eff_range(
                    eff_list,
                    current_eff,
                    marker.offset(),
                    role,
                ));
                let parallel_start = if marker.offset() > current_eff {
                    marker.offset()
                } else {
                    current_eff
                };
                self.push_resident_row(Self::local_step_range_for_eff_range(
                    eff_list,
                    parallel_start,
                    exit_eff,
                    role,
                ));
                current_eff = if exit_eff > current_eff {
                    exit_eff
                } else {
                    current_eff
                };
            }
            marker_idx += 1;
        }
        self.push_resident_row(Self::local_step_range_for_eff_range(
            eff_list,
            current_eff,
            eff_list.len(),
            role,
        ));
        if self.resident_row_len == 0 {
            self.push_resident_row(Self::local_step_range_for_eff_range(
                eff_list,
                0,
                eff_list.len(),
                role,
            ));
        }
    }

    #[inline(always)]
    const fn append_route_arm_lane_row(
        &mut self,
        eff_list: &EffList,
        slot: usize,
        arm: usize,
        start_eff: usize,
        end_eff: usize,
        role: u8,
    ) -> (PackedLaneRange, PackedLaneRange) {
        let row_idx = slot * 2 + arm;
        if row_idx >= MAX_ROUTE_ARM_LANE_ROWS {
            panic!("route arm lane row overflow");
        }
        let local_row = Self::local_step_range_for_eff_range(eff_list, start_eff, end_eff, role);
        self.route_arm_lane_rows[row_idx] = self.append_lane_bit_row_for_local_range(local_row);
        let lane_step_row = self.append_route_arm_lane_step_range(local_row);
        (local_row, lane_step_row)
    }

    #[inline(always)]
    const fn local_row_already_contains_lane(
        &self,
        row: PackedLaneRange,
        pos: usize,
        lane: u8,
    ) -> bool {
        let mut scan = row.start();
        while scan < pos && scan < MAX_LOCAL_STEP_LANES {
            if self.local_step_lanes[scan] == lane {
                return true;
            }
            scan += 1;
        }
        false
    }

    #[inline(always)]
    const fn route_arm_lane_step_count(&self, local_row: PackedLaneRange) -> usize {
        let mut len = 0usize;
        let mut pos = local_row.start();
        let end = local_row.end();
        while pos < end && pos < MAX_LOCAL_STEP_LANES {
            let lane = self.local_step_lanes[pos];
            if !self.local_row_already_contains_lane(local_row, pos, lane) {
                len += 1;
            }
            pos += 1;
        }
        len
    }

    #[inline(always)]
    const fn append_route_arm_lane_step_range(
        &mut self,
        local_row: PackedLaneRange,
    ) -> PackedLaneRange {
        let start = self.route_arm_lane_step_row_len as usize;
        let len = self.route_arm_lane_step_count(local_row);
        let end_len = start + len;
        if end_len > u16::MAX as usize {
            panic!("route arm lane step row overflow");
        }
        self.route_arm_lane_step_row_len = end_len as u16;
        PackedLaneRange::new(start, len)
    }

    #[inline(always)]
    const fn route_scope_from_row(row: ScopeId) -> Option<ScopeId> {
        if row.is_none() { None } else { Some(row) }
    }

    #[inline(always)]
    const fn route_slot_for_scope(&self, scope: ScopeId) -> Option<usize> {
        if scope.is_none() {
            return None;
        }
        let mut slot = 0usize;
        while slot < MAX_ROUTE_SCOPE_LANE_ROWS {
            if let Some(candidate) = Self::route_scope_from_row(self.route_scope_rows[slot])
                && candidate.same(scope)
            {
                return Some(slot);
            }
            slot += 1;
        }
        None
    }

    #[inline(always)]
    const fn route_scope_conflict_for_commit(&self, scope: ScopeId) -> PackedEventConflict {
        match self.route_slot_for_scope(scope) {
            Some(slot) => {
                let conflict = self.route_scope_conflicts[slot];
                match conflict.to_conflict() {
                    Some(LocalConflict::RouteArm { scope: parent, .. }) if parent.same(scope) => {
                        PackedEventConflict::none()
                    }
                    Some(_) | None => conflict,
                }
            }
            None => PackedEventConflict::none(),
        }
    }

    pub(crate) const fn route_commit_row_count(&self, slot: usize, arm: u8) -> usize {
        let arm = binary_route_arm_index(arm) as u8;
        let Some(scope) = Self::route_scope_from_row(self.route_scope_rows[slot]) else {
            panic!("route commit scope row missing");
        };
        let mut len = 0usize;
        let mut conflict = PackedEventConflict::route_arm(scope, arm);
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

    pub(crate) const fn route_commit_conflict_at(
        &self,
        slot: usize,
        arm: u8,
        target: usize,
    ) -> PackedEventConflict {
        let arm = binary_route_arm_index(arm) as u8;
        let Some(scope) = Self::route_scope_from_row(self.route_scope_rows[slot]) else {
            panic!("route commit scope row missing");
        };
        let mut depth = 0usize;
        let mut conflict = PackedEventConflict::route_arm(scope, arm);
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
        let row_idx = route_arm_row_index(slot, arm);
        if row_idx >= MAX_ROUTE_ARM_LANE_ROWS {
            panic!("route commit range overflow");
        }
        let len = self.route_commit_row_count(slot, arm);
        let start = self.route_commit_row_len as usize;
        let end = start + len;
        if end > u16::MAX as usize || len > u16::MAX as usize {
            panic!("route commit row overflow");
        }
        self.route_commit_row_len = end as u16;
        self.route_commit_ranges[row_idx] = PackedLaneRange::new(start, len);
    }

    const fn push_route_arm_lane_rows(&mut self, eff_list: &EffList, role: u8) {
        let markers = eff_list.scope_markers();
        if !Self::scope_markers_contain_kind(markers, ScopeKind::Route) {
            return;
        }
        let mut route_slot = 0usize;
        let mut marker_idx = 0usize;
        while marker_idx < markers.len() {
            let marker = markers[marker_idx];
            if Self::first_enter_for_scope(markers, marker_idx)
                && matches!(marker.scope_id.kind(), Some(ScopeKind::Route))
            {
                let scope = marker.scope_id;
                let view_len = eff_list.len();
                let Some(ranges) = Self::route_arm_ranges(markers, scope) else {
                    panic!("route scope missing binary arm ranges");
                };
                if route_slot >= MAX_ROUTE_SCOPE_LANE_ROWS {
                    panic!("route conflict row overflow");
                }
                self.route_scope_rows[route_slot] = scope;
                let conflict = Self::dependency_conflict_for_scope(markers, view_len, scope);
                self.route_scope_conflicts[route_slot] =
                    PackedEventConflict::from_conflict(conflict).with_route_reentry(marker.reentry);
                let mut arm = 0usize;
                while arm < 2 {
                    let (start, end) = ranges[arm];
                    let (local_row, lane_step_row) =
                        self.append_route_arm_lane_row(eff_list, route_slot, arm, start, end, role);
                    self.push_route_arm_projection_row(RouteArmProjectionRowInput {
                        markers,
                        view_len,
                        route_slot,
                        route_scope: scope,
                        arm: arm as u8,
                        local_row,
                        lane_step_row,
                        arm_start: start,
                        arm_end: end,
                    });
                    self.append_route_commit_range(route_slot, arm as u8);
                    arm += 1;
                }
                if route_slot >= MAX_ROUTE_SCOPE_LANE_ROWS {
                    panic!("route offer lane row overflow");
                }
                let row_idx = route_slot * 2;
                let left = self.route_arm_lane_rows[row_idx];
                let right = self.route_arm_lane_rows[row_idx + 1];
                self.route_offer_lane_rows[route_slot] =
                    self.append_lane_bit_union_row(left, right);
                route_slot += 1;
            }
            marker_idx += 1;
        }
    }

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

    pub(crate) const fn from_program(
        eff_list: &EffList,
        logical_lane_count: usize,
        role: u8,
    ) -> Self {
        if logical_lane_count == 0 || logical_lane_count > LANE_DOMAIN_SIZE {
            panic!("role logical lane domain invalid");
        }
        let mut lanes = Self {
            local_step_events: [PackedLocalEventRow::EMPTY; MAX_LOCAL_STEP_LANES],
            local_step_lanes: [0; MAX_LOCAL_STEP_LANES],
            local_step_dependencies: [PackedLocalDependency::none(); MAX_LOCAL_STEP_LANES],
            local_step_conflicts: [PackedEventConflict::none(); MAX_LOCAL_STEP_LANES],
            route_scope_rows: [ScopeId::none(); MAX_ROUTE_SCOPE_LANE_ROWS],
            route_scope_conflicts: [PackedEventConflict::none(); MAX_ROUTE_SCOPE_LANE_ROWS],
            route_arm_rows: [PackedRouteArmRow::EMPTY; MAX_ROUTE_ARM_LANE_ROWS],
            resident_row_boundaries: [0; MAX_RESIDENT_ROW_BOUNDARY_ROWS],
            lane_bit_rows: [0; MAX_RESIDENT_LANE_BIT_BYTES],
            route_arm_lane_rows: [PackedLaneRange::EMPTY; MAX_ROUTE_ARM_LANE_ROWS],
            route_offer_lane_rows: [PackedLaneRange::EMPTY; MAX_ROUTE_SCOPE_LANE_ROWS],
            route_commit_ranges: [PackedLaneRange::EMPTY; MAX_ROUTE_ARM_LANE_ROWS],
            roll_scope_rows: [PackedRollScopeRow::EMPTY; MAX_LOCAL_STEP_LANES],
            active_lane_row: PackedLaneRange::EMPTY,
            resident_row_len: 0,
            dependency_row_len: 0,
            conflict_row_len: 0,
            lane_bit_row_len: 0,
            route_commit_row_len: 0,
            route_arm_lane_step_row_len: 0,
            roll_scope_row_len: 0,
            first_active_lane: Self::ACTIVE_LANE_NONE,
        };
        let markers = eff_list.scope_markers();
        let has_route = Self::scope_markers_contain_kind(markers, ScopeKind::Route);
        let mut step = 0usize;
        let mut idx = 0usize;
        while idx < eff_list.len() {
            let node = eff_list.node_at(idx);
            if matches!(node.kind, crate::eff::EffKind::Atom) {
                let atom = node.atom_data();
                let frame_label = frame_label_at(eff_list, idx, atom);
                if atom.from == role || atom.to == role {
                    let lane = atom.lane as usize;
                    if lane >= logical_lane_count {
                        panic!("local event lane outside role logical domain");
                    }
                    if lane < lanes.first_active_lane as usize {
                        lanes.first_active_lane = lane as u16;
                    }
                    if step >= MAX_LOCAL_STEP_LANES {
                        panic!("role local lane table overflow");
                    }
                    lanes.local_step_events[step] =
                        Self::local_event_row_for_eff(eff_list, idx, frame_label, role);
                    lanes.local_step_lanes[step] = atom.lane;
                    lanes.local_step_conflicts[step] = if has_route {
                        Self::route_conflict_for_eff(markers, idx)
                    } else {
                        PackedEventConflict::none()
                    };
                    step += 1;
                }
            }
            idx += 1;
        }
        lanes.fill_dependency_rows(eff_list, step, role, has_route);
        lanes.compact_event_fact_rows(step);
        lanes.active_lane_row =
            lanes.append_lane_bit_row_for_local_range(PackedLaneRange::new(0, step));
        lanes.push_resident_rows(eff_list, role);
        lanes.push_route_arm_lane_rows(eff_list, role);
        lanes.push_roll_scope_rows(eff_list, role);
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

    #[inline(always)]
    pub(crate) const fn roll_scope_row_len(&self) -> usize {
        self.roll_scope_row_len as usize
    }
}
