use super::{
    CompiledProgramImage, LANE_DOMAIN_BYTES, LaneSetView, LaneSteps, MAX_LOCAL_STEP_LANES,
    MAX_RESIDENT_LANE_BIT_BYTES, MAX_RESIDENT_ROW_BOUNDARY_ROWS, MAX_RESIDENT_ROW_LANE_ROWS,
    MAX_ROUTE_ARM_LANE_ROWS, MAX_ROUTE_SCOPE_LANE_ROWS, PackedLaneRange, RoleCompiledCounts,
    RoleFacts, RoleFootprint, RoleImage, RoleImageRef, RoleImageSource, RoleLaneImage, ScopeEvent,
    ScopeId, ScopeKind, ScopeMarker, lane_byte_count, lane_byte_index, lane_word_count,
};
use crate::global::typestate::{
    LocalConflict, LocalDependency, PackedEventConflict, PackedLocalDependency,
};
mod ref_access;

impl RoleLaneImage {
    const NO_ACTIVE_LANE: u16 = u16::MAX;

    #[inline(always)]
    pub(crate) const fn local_step_lane(&self, step_idx: usize) -> Option<u8> {
        if step_idx >= MAX_LOCAL_STEP_LANES {
            None
        } else {
            Some(self.local_step_lanes[step_idx])
        }
    }

    #[inline(always)]
    const fn same_scope(left: ScopeId, right: ScopeId) -> bool {
        !left.is_none() && left.canonical_raw() == right.canonical_raw()
    }

    #[inline(always)]
    const fn first_enter_for_scope(markers: &[ScopeMarker], marker_idx: usize) -> bool {
        let marker = markers[marker_idx];
        if !matches!(marker.event, ScopeEvent::Enter) {
            return false;
        }
        let mut idx = 0usize;
        while idx < marker_idx {
            let candidate = markers[idx];
            if matches!(candidate.event, ScopeEvent::Enter)
                && Self::same_scope(candidate.scope_id, marker.scope_id)
            {
                return false;
            }
            idx += 1;
        }
        true
    }

    #[inline(always)]
    const fn route_arm_ranges(
        markers: &[ScopeMarker],
        route: ScopeId,
    ) -> Option<[(usize, usize); 2]> {
        if route.is_none() {
            return None;
        }
        let mut starts = [usize::MAX; 2];
        let mut ends = [usize::MAX; 2];
        let mut enter_len = 0usize;
        let mut exit_len = 0usize;
        let mut idx = 0usize;
        while idx < markers.len() {
            let marker = markers[idx];
            if Self::same_scope(marker.scope_id, route)
                && matches!(marker.scope_kind, ScopeKind::Route)
            {
                match marker.event {
                    ScopeEvent::Enter => {
                        if enter_len < 2 {
                            starts[enter_len] = marker.offset;
                        }
                        enter_len += 1;
                    }
                    ScopeEvent::Exit => {
                        if exit_len < 2 {
                            ends[exit_len] = marker.offset;
                        }
                        exit_len += 1;
                    }
                }
            }
            idx += 1;
        }
        if enter_len == 2 && exit_len == 2 {
            Some([(starts[0], ends[0]), (starts[1], ends[1])])
        } else {
            None
        }
    }

    #[inline(always)]
    const fn scope_segment_end(
        markers: &[ScopeMarker],
        enter_idx: usize,
        default_end: usize,
    ) -> usize {
        let marker = markers[enter_idx];
        let mut scan = enter_idx + 1;
        while scan < markers.len() {
            let candidate = markers[scan];
            if Self::same_scope(candidate.scope_id, marker.scope_id)
                && matches!(candidate.event, ScopeEvent::Exit)
            {
                return candidate.offset;
            }
            scan += 1;
        }
        default_end
    }

    #[inline(always)]
    const fn first_scope_segment_bounds(
        markers: &[ScopeMarker],
        default_end: usize,
        scope_id: ScopeId,
    ) -> Option<(ScopeKind, usize, usize)> {
        if scope_id.is_none() {
            return None;
        }
        let mut idx = 0usize;
        while idx < markers.len() {
            let marker = markers[idx];
            if matches!(marker.event, ScopeEvent::Enter)
                && Self::same_scope(marker.scope_id, scope_id)
            {
                return Some((
                    marker.scope_kind,
                    marker.offset,
                    Self::scope_segment_end(markers, idx, default_end),
                ));
            }
            idx += 1;
        }
        None
    }

    #[inline(always)]
    const fn route_arm_for_scope_start(
        markers: &[ScopeMarker],
        route: ScopeId,
        start: usize,
    ) -> Option<u8> {
        let Some(ranges) = Self::route_arm_ranges(markers, route) else {
            return None;
        };
        let mut arm = 0usize;
        while arm < 2 {
            let (arm_start, arm_end) = ranges[arm];
            if arm_start <= start && start < arm_end {
                return Some(arm as u8);
            }
            arm += 1;
        }
        None
    }

    #[inline(always)]
    const fn nearest_route_for_scope(
        markers: &[ScopeMarker],
        default_end: usize,
        scope_id: ScopeId,
    ) -> Option<ScopeId> {
        let Some((_, target_start, target_end)) =
            Self::first_scope_segment_bounds(markers, default_end, scope_id)
        else {
            return None;
        };
        let mut best = ScopeId::none();
        let mut best_span = usize::MAX;
        let mut best_start = 0usize;
        let mut idx = 0usize;
        while idx < markers.len() {
            let marker = markers[idx];
            if matches!(marker.event, ScopeEvent::Enter)
                && matches!(marker.scope_kind, ScopeKind::Route)
                && !Self::same_scope(marker.scope_id, scope_id)
            {
                let start = marker.offset;
                let end = match Self::route_arm_ranges(markers, marker.scope_id) {
                    Some(ranges) => {
                        let left_end = ranges[0].1;
                        let right_end = ranges[1].1;
                        if left_end > right_end {
                            left_end
                        } else {
                            right_end
                        }
                    }
                    None => Self::scope_segment_end(markers, idx, default_end),
                };
                if start <= target_start && target_end <= end {
                    let span = end.saturating_sub(start);
                    if best.is_none()
                        || span < best_span
                        || (span == best_span && start > best_start)
                    {
                        best = marker.scope_id;
                        best_span = span;
                        best_start = start;
                    }
                }
            }
            idx += 1;
        }
        if best.is_none() { None } else { Some(best) }
    }

    #[inline(always)]
    const fn route_conflict_for_eff(
        markers: &[ScopeMarker],
        eff_idx: usize,
    ) -> PackedEventConflict {
        let mut best = ScopeId::none();
        let mut best_arm = 0u8;
        let mut best_span = usize::MAX;
        let mut best_start = 0usize;
        let mut idx = 0usize;
        while idx < markers.len() {
            let marker = markers[idx];
            if Self::first_enter_for_scope(markers, idx)
                && matches!(marker.scope_kind, ScopeKind::Route)
                && let Some(ranges) = Self::route_arm_ranges(markers, marker.scope_id)
            {
                let mut arm = 0usize;
                while arm < 2 {
                    let (start, end) = ranges[arm];
                    if start <= eff_idx && eff_idx < end {
                        let span = end.saturating_sub(start);
                        if best.is_none()
                            || span < best_span
                            || (span == best_span && start > best_start)
                        {
                            best = marker.scope_id;
                            best_arm = arm as u8;
                            best_span = span;
                            best_start = start;
                        }
                    }
                    arm += 1;
                }
            }
            idx += 1;
        }
        if best.is_none() {
            PackedEventConflict::none()
        } else {
            PackedEventConflict::route_arm(best, best_arm)
        }
    }

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
    const fn dependency_conflict_for_scope(
        markers: &[ScopeMarker],
        view_len: usize,
        scope: ScopeId,
    ) -> LocalConflict {
        match Self::nearest_route_for_scope(markers, view_len, scope) {
            Some(route) => {
                let Some((_, start, _)) =
                    Self::first_scope_segment_bounds(markers, view_len, scope)
                else {
                    return LocalConflict::SharedRoute;
                };
                LocalConflict::route_arm(
                    route,
                    Self::route_arm_for_scope_start(markers, route, start),
                )
            }
            None => LocalConflict::Unconditional,
        }
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
    ) {
        let row_idx = slot.saturating_mul(2).saturating_add(arm);
        if row_idx >= MAX_ROUTE_ARM_LANE_ROWS {
            panic!("route arm lane row overflow");
        }
        let local_row = Self::local_step_range_for_eff_range::<ROLE>(program, start_eff, end_eff);
        self.route_arm_lane_rows[row_idx] = self.append_lane_bit_row_for_local_range(local_row);
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
                let Some(ranges) = Self::route_arm_ranges(markers, marker.scope_id) else {
                    panic!("route scope missing binary arm ranges");
                };
                if route_slot >= MAX_ROUTE_SCOPE_LANE_ROWS {
                    panic!("route conflict row overflow");
                }
                let conflict =
                    Self::dependency_conflict_for_scope(markers, view.len(), marker.scope_id);
                self.route_scope_conflicts[route_slot] =
                    PackedEventConflict::from_conflict(conflict);
                let mut arm = 0usize;
                while arm < 2 {
                    let (start, end) = ranges[arm];
                    self.append_route_arm_lane_row::<ROLE>(program, route_slot, arm, start, end);
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
    pub(crate) const fn from_program<const ROLE: u8>(
        program: &CompiledProgramImage,
        logical_lane_count: usize,
    ) -> Self {
        let mut lanes = Self {
            local_step_lanes: [0; MAX_LOCAL_STEP_LANES],
            local_step_dependencies: [PackedLocalDependency::none(); MAX_LOCAL_STEP_LANES],
            local_step_conflicts: [PackedEventConflict::none(); MAX_LOCAL_STEP_LANES],
            route_scope_conflicts: [PackedEventConflict::none(); MAX_ROUTE_SCOPE_LANE_ROWS],
            resident_row_boundaries: [0; MAX_RESIDENT_ROW_BOUNDARY_ROWS],
            lane_bit_rows: [0; MAX_RESIDENT_LANE_BIT_BYTES],
            route_arm_lane_rows: [PackedLaneRange::EMPTY; MAX_ROUTE_ARM_LANE_ROWS],
            route_offer_lane_rows: [PackedLaneRange::EMPTY; MAX_ROUTE_SCOPE_LANE_ROWS],
            active_lane_row: PackedLaneRange::EMPTY,
            resident_row_len: 0,
            lane_bit_row_len: 0,
            first_active_lane: Self::NO_ACTIVE_LANE,
        };
        let view = program.view();
        let markers = view.scope_markers();
        let mut local_step_effs = [usize::MAX; MAX_LOCAL_STEP_LANES];
        let mut step = 0usize;
        let mut idx = 0usize;
        while idx < view.len() {
            if let Some(atom) = view.atom_at(idx) {
                if atom.from == ROLE || atom.to == ROLE {
                    let lane = atom.lane as usize;
                    if lane < logical_lane_count {
                        if lane < lanes.first_active_lane as usize {
                            lanes.first_active_lane = lane as u16;
                        }
                        if step >= MAX_LOCAL_STEP_LANES {
                            panic!("role local lane table overflow");
                        }
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
        lanes.active_lane_row =
            lanes.append_lane_bit_row_for_local_range(PackedLaneRange::new(0, step));
        lanes.push_resident_rows::<ROLE>(program);
        lanes.push_route_arm_lane_rows::<ROLE>(program);
        lanes
    }

    #[inline(always)]
    const fn lane_bit_view(&self, range: PackedLaneRange, word_len: usize) -> LaneSetView<'_> {
        if range.is_empty() || range.len() == 0 {
            LaneSetView::from_bytes(core::ptr::null(), 0, word_len)
        } else {
            if range.end() > MAX_RESIDENT_LANE_BIT_BYTES {
                panic!("resident lane bit range exceeds lane bit table");
            }
            LaneSetView::from_bytes(
                /* SAFETY: the offset was checked against the backing allocation before pointer arithmetic. */
                unsafe { self.lane_bit_rows.as_ptr().add(range.start()) },
                range.len(),
                word_len,
            )
        }
    }

    #[inline(always)]
    const fn active_lane_set(&self, word_len: usize) -> LaneSetView<'_> {
        self.lane_bit_view(self.active_lane_row, word_len)
    }

    #[inline(always)]
    const fn resident_row_min_start(&self, idx: usize) -> Option<u16> {
        if idx >= self.resident_row_len as usize {
            return None;
        }
        let row = self.resident_row_range(idx);
        if row.is_empty() || row.len() == 0 {
            None
        } else if row.start() > u16::MAX as usize {
            panic!("resident row start exceeds descriptor capacity");
        } else {
            Some(row.start() as u16)
        }
    }

    #[inline(always)]
    pub(crate) const fn resident_row_lane_steps(
        &self,
        idx: usize,
        lane_idx: usize,
    ) -> Option<LaneSteps> {
        if lane_idx > u8::MAX as usize {
            return None;
        }
        if idx >= self.resident_row_len as usize {
            return None;
        }
        let row = self.resident_row_range(idx);
        let mut pos = row.start();
        let end = row.end();
        let mut first = usize::MAX;
        let mut len = 0usize;
        let mut sparse = false;
        while pos < end && pos < MAX_LOCAL_STEP_LANES {
            if self.local_step_lanes[pos] as usize == lane_idx {
                if first == usize::MAX {
                    first = pos;
                } else if pos != first.saturating_add(len) {
                    sparse = true;
                }
                len += 1;
            }
            pos += 1;
        }
        if len == 0 {
            None
        } else if first > u16::MAX as usize || len > u16::MAX as usize {
            panic!("resident row lane steps exceed descriptor capacity");
        } else {
            Some(LaneSteps {
                start: first as u16,
                len: len as u16,
                sparse,
            })
        }
    }

    #[inline(always)]
    pub(crate) const fn dependency_for_index(&self, current_idx: usize) -> Option<LocalDependency> {
        if current_idx >= MAX_LOCAL_STEP_LANES {
            return None;
        }
        self.local_step_dependencies[current_idx].to_dependency()
    }

    #[inline(always)]
    pub(crate) const fn event_conflict_for_index(&self, current_idx: usize) -> PackedEventConflict {
        if current_idx >= MAX_LOCAL_STEP_LANES {
            PackedEventConflict::none()
        } else {
            self.local_step_conflicts[current_idx]
        }
    }

    #[inline(always)]
    pub(crate) const fn route_scope_conflict_by_slot(&self, slot: usize) -> PackedEventConflict {
        if slot >= MAX_ROUTE_SCOPE_LANE_ROWS {
            PackedEventConflict::none()
        } else {
            self.route_scope_conflicts[slot]
        }
    }

    #[inline(always)]
    pub(crate) const fn resident_row_lane_step_at(
        &self,
        idx: usize,
        lane_idx: usize,
        ordinal: usize,
    ) -> Option<u16> {
        if lane_idx > u8::MAX as usize {
            return None;
        }
        if idx >= self.resident_row_len as usize {
            return None;
        }
        let row = self.resident_row_range(idx);
        let mut pos = row.start();
        let end = row.end();
        let mut seen = 0usize;
        while pos < end && pos < MAX_LOCAL_STEP_LANES {
            if self.local_step_lanes[pos] as usize == lane_idx {
                if seen == ordinal {
                    if pos > u16::MAX as usize {
                        panic!("resident row lane step index exceeds descriptor capacity");
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
    const fn resident_row_lane_step_ordinal(
        &self,
        idx: usize,
        lane_idx: usize,
        step_idx: usize,
    ) -> Option<u16> {
        if lane_idx > u8::MAX as usize {
            return None;
        }
        if idx >= self.resident_row_len as usize {
            return None;
        }
        let row = self.resident_row_range(idx);
        if step_idx < row.start() || step_idx >= row.end() || step_idx >= MAX_LOCAL_STEP_LANES {
            return None;
        }
        let mut pos = row.start();
        let end = row.end();
        let mut ordinal = 0usize;
        while pos < end && pos < MAX_LOCAL_STEP_LANES {
            if self.local_step_lanes[pos] as usize == lane_idx {
                if pos == step_idx {
                    if ordinal > u16::MAX as usize {
                        panic!("resident row lane step ordinal exceeds descriptor capacity");
                    }
                    return Some(ordinal as u16);
                }
                ordinal += 1;
            }
            pos += 1;
        }
        None
    }

    #[inline(always)]
    const fn first_active_lane(&self) -> Option<usize> {
        if self.first_active_lane == Self::NO_ACTIVE_LANE {
            None
        } else {
            Some(self.first_active_lane as usize)
        }
    }

    #[inline(always)]
    const fn resident_row_range(&self, idx: usize) -> PackedLaneRange {
        if idx >= self.resident_row_len as usize {
            return PackedLaneRange::EMPTY;
        }
        let start = self.resident_row_boundaries[idx] as usize;
        let end = self.resident_row_boundaries[idx + 1] as usize;
        PackedLaneRange::new(start, end.saturating_sub(start))
    }

    #[inline(always)]
    const fn route_scope_arm_lane_set_by_slot(
        &self,
        slot: usize,
        arm: u8,
        logical_lane_word_count: usize,
    ) -> Option<LaneSetView<'_>> {
        if arm >= 2 {
            return None;
        }
        let row_idx = slot.saturating_mul(2).saturating_add(arm as usize);
        if row_idx >= MAX_ROUTE_ARM_LANE_ROWS {
            return None;
        }
        let row = self.route_arm_lane_rows[row_idx];
        if row.is_empty() {
            return None;
        }
        Some(self.lane_bit_view(row, logical_lane_word_count))
    }

    #[inline(always)]
    const fn route_scope_offer_lane_set_by_slot(
        &self,
        slot: usize,
        logical_lane_word_count: usize,
    ) -> Option<LaneSetView<'_>> {
        if slot >= MAX_ROUTE_SCOPE_LANE_ROWS {
            return None;
        }
        let row = self.route_offer_lane_rows[slot];
        if row.is_empty() {
            return None;
        }
        Some(self.lane_bit_view(row, logical_lane_word_count))
    }
}
